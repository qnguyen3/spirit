use std::net::{IpAddr, Ipv4Addr};

use remote_control::auth::{AccessToken, DeviceId, PairedDevice, SessionId, hash_session_id};

use super::{PairFailure, SessionStore};
use crate::remote_control::now_ms;

fn address() -> Option<IpAddr> {
    Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)))
}

#[test]
fn pairing_with_the_right_token_creates_a_session() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    let paired = store
        .pair(&token, token.reveal(), address(), Some("curl/8"))
        .expect("the correct token pairs");
    assert!(store.touch(paired.session_id.reveal()).is_some());
}

#[test]
fn pairing_with_a_wrong_token_is_refused() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    assert_eq!(
        store
            .pair(&token, "nope", address(), None)
            .err()
            .expect("a wrong token is refused"),
        PairFailure::BadToken
    );
}

#[test]
fn repeated_failures_from_one_address_are_rate_limited() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    for _ in 0..5 {
        assert_eq!(
            store.pair(&token, "nope", address(), None).err(),
            Some(PairFailure::BadToken)
        );
    }
    assert_eq!(
        store.pair(&token, "nope", address(), None).err(),
        Some(PairFailure::RateLimited)
    );
    assert_eq!(
        store.pair(&token, token.reveal(), address(), None).err(),
        Some(PairFailure::RateLimited)
    );
}

#[test]
fn a_different_address_is_not_blocked_by_another_ones_failures() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    for _ in 0..6 {
        let _ = store.pair(&token, "nope", address(), None);
    }
    let other = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));
    assert!(store.pair(&token, token.reveal(), other, None).is_ok());
}

#[test]
fn a_successful_pairing_clears_the_failure_counter() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    for _ in 0..4 {
        let _ = store.pair(&token, "nope", address(), None);
    }
    assert!(store.pair(&token, token.reveal(), address(), None).is_ok());
    for _ in 0..5 {
        assert_eq!(
            store.pair(&token, "nope", address(), None).err(),
            Some(PairFailure::BadToken)
        );
    }
}

#[test]
fn an_unknown_cookie_does_not_authenticate() {
    let store = SessionStore::default();
    assert!(store.touch(SessionId::generate().reveal()).is_none());
    assert!(store.touch("not-a-session").is_none());
}

#[test]
fn revoking_a_session_invalidates_its_cookie() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    let paired = store
        .pair(&token, token.reveal(), address(), None)
        .expect("pairing succeeds");
    assert!(store.revoke_session(paired.session_id.reveal()));
    assert!(store.touch(paired.session_id.reveal()).is_none());
}

#[test]
fn revoking_a_device_invalidates_its_sessions() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    let paired = store
        .pair(&token, token.reveal(), address(), None)
        .expect("pairing succeeds");
    assert!(store.revoke_device(paired.device_id.as_str()));
    assert!(store.touch(paired.session_id.reveal()).is_none());
    assert!(!store.revoke_device(paired.device_id.as_str()));
}

#[test]
fn devices_are_listed_newest_first_and_mark_the_caller() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    let first = store.pair(&token, token.reveal(), address(), None).unwrap();
    let devices = store.devices(Some(&first.device_id));
    assert_eq!(devices.len(), 1);
    assert!(devices[0].current);
    assert_eq!(devices[0].id, first.device_id.as_str());
}

#[test]
fn stored_devices_are_restored_and_stale_ones_dropped() {
    let session = SessionId::generate();
    let fresh = PairedDevice {
        id: DeviceId::generate(),
        session_hash: hash_session_id(&session),
        label: "Mac · Chrome".to_owned(),
        created_ts: now_ms(),
        last_seen_ts: now_ms(),
    };
    let stale = PairedDevice {
        id: DeviceId::generate(),
        session_hash: hash_session_id(&SessionId::generate()),
        label: "Old".to_owned(),
        created_ts: 0,
        last_seen_ts: 0,
    };
    let store = SessionStore::new(vec![fresh, stale]);
    assert!(store.touch(session.reveal()).is_some());
    assert_eq!(store.devices(None).len(), 1);
}

#[test]
fn persistable_devices_are_only_returned_after_a_change() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    assert!(store.take_persistable().is_none());
    store.pair(&token, token.reveal(), address(), None).unwrap();
    let devices = store
        .take_persistable()
        .expect("pairing marks the store dirty");
    assert_eq!(devices.len(), 1);
    assert!(store.take_persistable().is_none());
}

#[test]
fn renaming_a_device_updates_its_label() {
    let token = AccessToken::generate();
    let store = SessionStore::default();
    let paired = store.pair(&token, token.reveal(), address(), None).unwrap();
    assert!(store.rename_device(paired.device_id.as_str(), "Kitchen iPad"));
    assert_eq!(store.devices(None)[0].label, "Kitchen iPad");
    assert!(!store.rename_device("nope", "x"));
}
