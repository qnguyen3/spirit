use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use remote_control::auth::DeviceId;
use remote_control::limits::STATE_COALESCE_MS;
use remote_control::protocol::{
    AppSnapshot, ClonePhase, CommandError, CommandName, ErrorCode, ServerEvent, ServerMessage,
};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use warpui::r#async::Timer;
use warpui::{Entity, EntityId, ModelContext, ModelSpawner, SingletonEntity};

use super::mirror_encoder::MirrorPayload;
use super::sessions::SessionStore;
use super::terminal_mirror::{self, MirrorHub};
use super::terminal_snapshot::build_attach_snapshot;
use super::terminal_streams::{StreamFrame, TerminalStreams};
use super::{commands, projection, remote_control_available, watch};
use crate::projects::interactive_path_env;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(1);
const PATH_ENV_REFRESH: Duration = Duration::from_secs(300);
const CLIENT_CHANNEL_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ClientId(u64);

impl ClientId {
    pub fn as_string(self) -> String {
        format!("c{}", self.0)
    }
}

pub(crate) enum CommandOutcome {
    Immediate(Result<Value, CommandError>),
    Deferred,
}

struct ClientHandle {
    sender: mpsc::Sender<Arc<ServerMessage>>,
    streams: Option<mpsc::Sender<(u32, StreamFrame)>>,
    mirror_payloads: Option<mpsc::Sender<MirrorPayload>>,
    device_id: Option<DeviceId>,
}

pub(crate) struct EncoderHost {
    pub runtime: tokio::runtime::Handle,
    pub spawner: ModelSpawner<RemoteControlBridge>,
}

pub(crate) struct ClientRegistration {
    pub client_id: ClientId,
    pub receiver: mpsc::Receiver<Arc<ServerMessage>>,
    pub instance_id: String,
    pub latest: Option<Arc<ServerMessage>>,
}

pub struct RemoteControlBridge {
    clients: HashMap<ClientId, ClientHandle>,
    next_client_id: u64,
    latest: Option<(u64, AppSnapshot)>,
    dirty: bool,
    rebuild_scheduled: bool,
    reconcile_running: bool,
    state_tx: Option<broadcast::Sender<Arc<ServerMessage>>>,
    sessions: Option<SessionStore>,
    watched_workspaces: HashSet<EntityId>,
    watched_terminals: HashSet<EntityId>,
    watched_focus_states: HashSet<EntityId>,
    watchers_installed: bool,
    path_env: Option<String>,
    streams: TerminalStreams,
    pub(super) mirrors: MirrorHub,
    encoder_host: Option<EncoderHost>,
    instance_id: String,
}

impl Entity for RemoteControlBridge {
    type Event = ();
}

impl SingletonEntity for RemoteControlBridge {}

impl RemoteControlBridge {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut bridge = Self {
            clients: HashMap::new(),
            next_client_id: 1,
            latest: None,
            dirty: true,
            rebuild_scheduled: false,
            reconcile_running: false,
            state_tx: None,
            sessions: None,
            watched_workspaces: HashSet::new(),
            watched_terminals: HashSet::new(),
            watched_focus_states: HashSet::new(),
            watchers_installed: false,
            path_env: None,
            streams: TerminalStreams::default(),
            mirrors: MirrorHub::default(),
            encoder_host: None,
            instance_id: String::new(),
        };
        bridge.refresh_path_env(ctx);
        bridge
    }

    pub(crate) fn attach_server(
        &mut self,
        state_tx: broadcast::Sender<Arc<ServerMessage>>,
        sessions: SessionStore,
        instance_id: String,
        runtime: tokio::runtime::Handle,
        ctx: &mut ModelContext<Self>,
    ) -> ModelSpawner<Self> {
        self.state_tx = Some(state_tx);
        self.sessions = Some(sessions);
        self.instance_id = instance_id;
        self.latest = None;
        self.dirty = true;
        let spawner = self.install_encoder_host(runtime, ctx);
        if !self.watchers_installed {
            watch::install_watchers(ctx);
            self.watchers_installed = true;
        }
        watch::ensure_entity_watchers(self, ctx);
        spawner
    }

    pub(crate) fn install_encoder_host(
        &mut self,
        runtime: tokio::runtime::Handle,
        ctx: &mut ModelContext<Self>,
    ) -> ModelSpawner<Self> {
        let spawner = ctx.spawner();
        self.encoder_host = Some(EncoderHost {
            runtime,
            spawner: spawner.clone(),
        });
        spawner
    }

    pub(crate) fn encoder_host(&self) -> Option<&EncoderHost> {
        self.encoder_host.as_ref()
    }

    pub(crate) fn detach_server(&mut self) {
        terminal_mirror::stop_all(self);
        self.state_tx = None;
        self.sessions = None;
        self.encoder_host = None;
        self.clients.clear();
        self.streams.close_all();
        self.latest = None;
    }

    pub(crate) fn streams(&mut self) -> &mut TerminalStreams {
        &mut self.streams
    }

    pub(crate) fn path_env(&self) -> Option<&str> {
        self.path_env.as_deref()
    }

    pub(crate) fn sessions(&self) -> Option<&SessionStore> {
        self.sessions.as_ref()
    }

    pub(crate) fn client_device(&self, client_id: ClientId) -> Option<DeviceId> {
        self.clients.get(&client_id)?.device_id.clone()
    }

    pub(crate) fn set_client_device(&mut self, client_id: ClientId, device_id: Option<DeviceId>) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.device_id = device_id;
        }
    }

    pub(crate) fn disconnect_device(&mut self, device_id: &DeviceId) {
        let owned: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| client.device_id.as_ref() == Some(device_id))
            .map(|(client_id, _)| *client_id)
            .collect();
        for client_id in owned {
            self.clients.remove(&client_id);
            terminal_mirror::stop_client(self, client_id);
        }
    }

    pub(crate) fn watched_workspaces(&mut self) -> &mut HashSet<EntityId> {
        &mut self.watched_workspaces
    }

    pub(crate) fn watched_terminals(&mut self) -> &mut HashSet<EntityId> {
        &mut self.watched_terminals
    }

    pub(crate) fn watched_focus_states(&mut self) -> &mut HashSet<EntityId> {
        &mut self.watched_focus_states
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn mark_dirty(&mut self, ctx: &mut ModelContext<Self>) {
        self.dirty = true;
        if self.rebuild_scheduled || self.clients.is_empty() {
            return;
        }
        self.rebuild_scheduled = true;
        ctx.spawn(
            async move { Timer::after(Duration::from_millis(STATE_COALESCE_MS)).await },
            |bridge, _, ctx| {
                bridge.rebuild_scheduled = false;
                bridge.rebuild(ctx);
            },
        );
    }

    fn rebuild(&mut self, ctx: &mut ModelContext<Self>) {
        if !self.dirty || self.clients.is_empty() {
            return;
        }
        self.dirty = false;
        watch::ensure_entity_watchers(self, ctx);
        let next = self.build_snapshot(ctx);
        let changed = match &self.latest {
            Some((_, previous)) => !snapshots_match(previous, &next),
            None => true,
        };
        if !changed {
            return;
        }
        let version = self
            .latest
            .as_ref()
            .map(|(version, _)| *version)
            .unwrap_or(0)
            + 1;
        let mut stored = next;
        stored.version = version;
        if let Some(state_tx) = &self.state_tx {
            let _ = state_tx.send(Arc::new(ServerMessage::State {
                version,
                snapshot: Box::new(stored.clone()),
            }));
        }
        self.latest = Some((version, stored));
    }

    fn build_snapshot(&mut self, ctx: &mut ModelContext<Self>) -> AppSnapshot {
        let clients = self.clients.len();
        let instance_id = self.instance_id.clone();
        let path_env = self.path_env.clone();
        projection::build_snapshot(&instance_id, clients, path_env.as_deref(), ctx)
    }

    pub(crate) fn snapshot_now(
        &mut self,
        ctx: &mut ModelContext<Self>,
    ) -> Result<AppSnapshot, CommandError> {
        if !remote_control_available(ctx) {
            return Err(CommandError::new(
                ErrorCode::FeatureDisabled,
                "Remote Control is turned off",
            ));
        }
        let version = self
            .latest
            .as_ref()
            .map(|(version, _)| *version)
            .unwrap_or(0);
        let mut snapshot = self.build_snapshot(ctx);
        snapshot.version = version;
        Ok(snapshot)
    }

    pub(crate) fn connect(&mut self, ctx: &mut ModelContext<Self>) -> ClientRegistration {
        let client_id = ClientId(self.next_client_id);
        self.next_client_id += 1;
        let (sender, receiver) = mpsc::channel(CLIENT_CHANNEL_CAPACITY);
        self.clients.insert(
            client_id,
            ClientHandle {
                sender,
                streams: None,
                mirror_payloads: None,
                device_id: None,
            },
        );
        self.dirty = true;
        if let Some(sessions) = &self.sessions {
            sessions.sweep_idle();
        }
        watch::ensure_entity_watchers(self, ctx);
        self.rebuild(ctx);
        self.start_reconcile_loop(ctx);
        let latest = self.latest.as_ref().map(|(version, snapshot)| {
            Arc::new(ServerMessage::State {
                version: *version,
                snapshot: Box::new(snapshot.clone()),
            })
        });
        ClientRegistration {
            client_id,
            receiver,
            instance_id: self.instance_id.clone(),
            latest,
        }
    }

    pub(crate) fn disconnect(&mut self, client_id: ClientId) {
        self.clients.remove(&client_id);
        self.streams.detach_client(client_id);
        terminal_mirror::stop_client(self, client_id);
    }

    pub(crate) fn set_stream_sender(
        &mut self,
        client_id: ClientId,
        sender: mpsc::Sender<(u32, StreamFrame)>,
    ) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.streams = Some(sender);
        }
    }

    pub(crate) fn stream_sender(
        &self,
        client_id: ClientId,
    ) -> Option<mpsc::Sender<(u32, StreamFrame)>> {
        self.clients.get(&client_id)?.streams.clone()
    }

    pub(crate) fn set_mirror_sender(
        &mut self,
        client_id: ClientId,
        sender: mpsc::Sender<MirrorPayload>,
    ) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.mirror_payloads = Some(sender);
        }
    }

    pub(crate) fn mirror_sender(&self, client_id: ClientId) -> Option<mpsc::Sender<MirrorPayload>> {
        self.clients.get(&client_id)?.mirror_payloads.clone()
    }

    pub(crate) fn mirror_frame_done(
        &mut self,
        client_id: ClientId,
        mirror_id: u32,
        ctx: &mut ModelContext<Self>,
    ) {
        terminal_mirror::frame_done(self, client_id, mirror_id, ctx);
    }

    pub(crate) fn build_resync(
        &mut self,
        client_id: ClientId,
        attach_id: u32,
        ctx: &mut ModelContext<Self>,
    ) -> Option<ServerEvent> {
        let terminal_id = self.streams.terminal_for(attach_id, client_id)?;
        let target = super::resolve::terminal(&terminal_id.to_string(), ctx).ok()?;
        let snapshot = build_attach_snapshot(&target.terminal, ctx);
        Some(ServerEvent::TerminalResync {
            attach_id,
            cols: snapshot.cols,
            rows: snapshot.rows,
            mode: snapshot.mode,
            snapshot: snapshot.encoded(),
        })
    }

    pub(crate) fn write_attachment_input(
        &mut self,
        client_id: ClientId,
        attach_id: u32,
        bytes: Vec<u8>,
        ctx: &mut ModelContext<Self>,
    ) {
        if !remote_control_available(ctx) {
            return;
        }
        let Some(terminal_id) = self.streams.terminal_for(attach_id, client_id) else {
            return;
        };
        let written = commands::write_bytes_to_terminal(&terminal_id.to_string(), bytes, ctx);
        if written.is_ok() {
            self.mark_dirty(ctx);
        }
    }

    pub(crate) fn send_to_client(&self, client_id: ClientId, message: ServerMessage) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        let _ = client.sender.try_send(Arc::new(message));
    }

    pub(crate) fn send_clone_progress(
        &self,
        client_id: ClientId,
        job_id: String,
        phase: ClonePhase,
        percent: Option<u8>,
        message: Option<String>,
    ) {
        self.send_to_client(
            client_id,
            ServerMessage::Event {
                event: ServerEvent::ProjectCloneProgress {
                    job_id,
                    phase,
                    percent,
                    message,
                },
            },
        );
    }

    pub(crate) fn resolve_deferred(
        &self,
        client_id: ClientId,
        command_id: String,
        result: Result<Value, CommandError>,
    ) {
        let message = match result {
            Ok(data) => ServerMessage::ok_result(command_id, data),
            Err(error) => ServerMessage::error_result(command_id, error),
        };
        self.send_to_client(client_id, message);
    }

    pub(crate) fn execute(
        &mut self,
        client_id: ClientId,
        command_id: String,
        name: CommandName,
        params: Value,
        ctx: &mut ModelContext<Self>,
    ) -> CommandOutcome {
        if !remote_control_available(ctx) {
            return CommandOutcome::Immediate(Err(CommandError::new(
                ErrorCode::FeatureDisabled,
                "Remote Control is turned off",
            )));
        }
        let outcome = commands::execute(self, client_id, command_id, name, params, ctx);
        if matches!(outcome, CommandOutcome::Immediate(Ok(_))) {
            self.mark_dirty(ctx);
        }
        outcome
    }

    fn start_reconcile_loop(&mut self, ctx: &mut ModelContext<Self>) {
        if self.reconcile_running {
            return;
        }
        self.reconcile_running = true;
        self.schedule_reconcile(ctx);
    }

    fn schedule_reconcile(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.spawn(
            async move { Timer::after(RECONCILE_INTERVAL).await },
            |bridge, _, ctx| {
                if bridge.clients.is_empty() {
                    bridge.reconcile_running = false;
                    return;
                }
                bridge.mark_dirty(ctx);
                bridge.schedule_reconcile(ctx);
            },
        );
    }

    fn refresh_path_env(&mut self, ctx: &mut ModelContext<Self>) {
        let resolve = interactive_path_env(ctx);
        ctx.spawn(resolve, |bridge, path_env, ctx| {
            bridge.path_env = path_env;
            ctx.spawn(
                async move { Timer::after(PATH_ENV_REFRESH).await },
                |bridge, _, ctx| bridge.refresh_path_env(ctx),
            );
        });
    }
}

fn snapshots_match(left: &AppSnapshot, right: &AppSnapshot) -> bool {
    left.instance_id == right.instance_id
        && left.active_window_id == right.active_window_id
        && left.windows == right.windows
        && left.projects == right.projects
        && left.sessions == right.sessions
        && left.agents == right.agents
        && left.server == right.server
        && left.features == right.features
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
