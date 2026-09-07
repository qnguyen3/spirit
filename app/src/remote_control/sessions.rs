use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use remote_control::auth::{
    AccessToken, DeviceId, PairedDevice, SessionHash, SessionId, device_label_from_user_agent,
    hash_session_id,
};
use remote_control::limits::{PAIR_FAILURES_PER_MINUTE, SESSION_IDLE_DAYS};
use remote_control::protocol::DeviceSummary;

use super::now_ms;

const MILLIS_PER_MINUTE: i64 = 60 * 1000;
const MILLIS_PER_DAY: i64 = 24 * 60 * MILLIS_PER_MINUTE;

#[derive(Clone, Debug)]
pub(crate) struct SessionEntry {
    pub device_id: DeviceId,
    pub label: String,
    pub created_ts: i64,
    pub last_seen_ts: i64,
}

#[derive(Default)]
struct FailureCounter {
    attempts: HashMap<IpAddr, Vec<i64>>,
}

impl FailureCounter {
    fn prune(&mut self, now: i64) {
        self.attempts.retain(|_, timestamps| {
            timestamps.retain(|ts| now - *ts < MILLIS_PER_MINUTE);
            !timestamps.is_empty()
        });
    }

    fn is_blocked(&mut self, address: IpAddr, now: i64) -> bool {
        self.prune(now);
        self.attempts
            .get(&address)
            .is_some_and(|timestamps| timestamps.len() as u64 >= PAIR_FAILURES_PER_MINUTE)
    }

    fn record_failure(&mut self, address: IpAddr, now: i64) {
        self.prune(now);
        self.attempts.entry(address).or_default().push(now);
    }

    fn clear(&mut self, address: IpAddr) {
        self.attempts.remove(&address);
    }
}

#[derive(Default)]
struct StoreInner {
    sessions: HashMap<SessionHash, SessionEntry>,
    failures: FailureCounter,
    persist_dirty: bool,
    last_persist_ts: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PairFailure {
    BadToken,
    RateLimited,
}

pub(crate) struct PairedSession {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    pub label: String,
}

#[derive(Clone, Default)]
pub(crate) struct SessionStore {
    inner: Arc<Mutex<StoreInner>>,
}

impl SessionStore {
    pub fn new(stored: Vec<PairedDevice>) -> Self {
        let now = now_ms();
        let cutoff = now - SESSION_IDLE_DAYS as i64 * MILLIS_PER_DAY;
        let sessions = stored
            .into_iter()
            .filter(|device| device.last_seen_ts >= cutoff)
            .map(|device| {
                (
                    device.session_hash,
                    SessionEntry {
                        device_id: device.id,
                        label: device.label,
                        created_ts: device.created_ts,
                        last_seen_ts: device.last_seen_ts,
                    },
                )
            })
            .collect();
        Self {
            inner: Arc::new(Mutex::new(StoreInner {
                sessions,
                failures: FailureCounter::default(),
                persist_dirty: false,
                last_persist_ts: now,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn pair(
        &self,
        token: &AccessToken,
        attempt: &str,
        remote_address: Option<IpAddr>,
        user_agent: Option<&str>,
    ) -> Result<PairedSession, PairFailure> {
        let now = now_ms();
        let mut inner = self.lock();
        if let Some(address) = remote_address
            && inner.failures.is_blocked(address, now)
        {
            return Err(PairFailure::RateLimited);
        }
        if !token.constant_time_eq(attempt) {
            if let Some(address) = remote_address {
                inner.failures.record_failure(address, now);
            }
            return Err(PairFailure::BadToken);
        }
        if let Some(address) = remote_address {
            inner.failures.clear(address);
        }
        let session_id = SessionId::generate();
        let label = device_label_from_user_agent(user_agent);
        let device_id = DeviceId::generate();
        inner.sessions.insert(
            hash_session_id(&session_id),
            SessionEntry {
                device_id: device_id.clone(),
                label: label.clone(),
                created_ts: now,
                last_seen_ts: now,
            },
        );
        inner.persist_dirty = true;
        Ok(PairedSession {
            session_id,
            device_id,
            label,
        })
    }

    pub fn touch(&self, session_id: &str) -> Option<SessionEntry> {
        let parsed = SessionId::parse(session_id).ok()?;
        let hash = hash_session_id(&parsed);
        let now = now_ms();
        let mut inner = self.lock();
        let entry = inner.sessions.get_mut(&hash)?;
        entry.last_seen_ts = now;
        let entry = entry.clone();
        if now - inner.last_persist_ts > MILLIS_PER_MINUTE {
            inner.persist_dirty = true;
        }
        Some(entry)
    }

    pub fn revoke_session(&self, session_id: &str) -> bool {
        let Ok(parsed) = SessionId::parse(session_id) else {
            return false;
        };
        let mut inner = self.lock();
        let removed = inner.sessions.remove(&hash_session_id(&parsed)).is_some();
        inner.persist_dirty |= removed;
        removed
    }

    pub fn revoke_device(&self, device_id: &str) -> bool {
        let mut inner = self.lock();
        let before = inner.sessions.len();
        inner
            .sessions
            .retain(|_, entry| entry.device_id.as_str() != device_id);
        let removed = inner.sessions.len() != before;
        inner.persist_dirty |= removed;
        removed
    }

    pub fn rename_device(&self, device_id: &str, label: &str) -> bool {
        let mut inner = self.lock();
        let mut renamed = false;
        for entry in inner.sessions.values_mut() {
            if entry.device_id.as_str() == device_id {
                entry.label = label.to_owned();
                renamed = true;
            }
        }
        inner.persist_dirty |= renamed;
        renamed
    }

    pub fn devices(&self, current_device: Option<&DeviceId>) -> Vec<DeviceSummary> {
        let inner = self.lock();
        let mut devices: Vec<DeviceSummary> = inner
            .sessions
            .values()
            .map(|entry| DeviceSummary {
                id: entry.device_id.as_str().to_owned(),
                label: entry.label.clone(),
                created_ts: entry.created_ts,
                last_seen_ts: entry.last_seen_ts,
                connected: false,
                current: current_device.is_some_and(|current| *current == entry.device_id),
            })
            .collect();
        devices.sort_by(|left, right| {
            right
                .last_seen_ts
                .cmp(&left.last_seen_ts)
                .then_with(|| left.id.cmp(&right.id))
        });
        devices
    }

    pub fn sweep_idle(&self) {
        let cutoff = now_ms() - SESSION_IDLE_DAYS as i64 * MILLIS_PER_DAY;
        let mut inner = self.lock();
        let before = inner.sessions.len();
        inner
            .sessions
            .retain(|_, entry| entry.last_seen_ts >= cutoff);
        inner.persist_dirty |= inner.sessions.len() != before;
    }

    pub fn take_persistable(&self) -> Option<Vec<PairedDevice>> {
        let mut inner = self.lock();
        if !inner.persist_dirty {
            return None;
        }
        inner.persist_dirty = false;
        inner.last_persist_ts = now_ms();
        Some(
            inner
                .sessions
                .iter()
                .map(|(hash, entry)| PairedDevice {
                    id: entry.device_id.clone(),
                    session_hash: hash.clone(),
                    label: entry.label.clone(),
                    created_ts: entry.created_ts,
                    last_seen_ts: entry.last_seen_ts,
                })
                .collect(),
        )
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
