pub(crate) mod assets;
pub(crate) mod bridge;
pub(crate) mod commands;
pub(crate) mod history_ops;
pub(crate) mod http;
pub(crate) mod lan;
pub(crate) mod project_ops;
pub(crate) mod projection;
pub(crate) mod qr;
pub(crate) mod resolve;
pub(crate) mod sessions;
pub(crate) mod terminal_mirror;
pub(crate) mod terminal_snapshot;
pub(crate) mod terminal_streams;
pub(crate) mod watch;
pub(crate) mod worktree_ops;
pub(crate) mod ws;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use remote_control::hosts::{AllowedHosts, RemoteEndpoint};
use remote_control::protocol::{ServerEvent, ServerMessage};
use tokio::sync::broadcast;
use warp_core::channel::ChannelState;
use warp_core::features::FeatureFlag;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use self::bridge::RemoteControlBridge;
use self::http::AppState;
use self::sessions::SessionStore;
use crate::settings::{RemoteControlSecrets, RemoteControlSettings};

const BROADCAST_CAPACITY: usize = 64;

pub enum RemoteControlServerEvent {
    StateChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerState {
    Stopped,
    Running {
        endpoint: RemoteEndpoint,
        lan_endpoints: Vec<RemoteEndpoint>,
        started_ts: i64,
        connected_clients: usize,
    },
    Failed {
        message: String,
    },
}

impl ServerState {
    pub fn is_running(&self) -> bool {
        matches!(self, ServerState::Running { .. })
    }

    pub fn endpoint(&self) -> Option<&RemoteEndpoint> {
        match self {
            ServerState::Running { endpoint, .. } => Some(endpoint),
            ServerState::Stopped | ServerState::Failed { .. } => None,
        }
    }

    pub fn connected_clients(&self) -> usize {
        match self {
            ServerState::Running {
                connected_clients, ..
            } => *connected_clients,
            ServerState::Stopped | ServerState::Failed { .. } => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListenerConfig {
    enabled: bool,
    port: u16,
    allow_lan_access: bool,
}

impl ListenerConfig {
    fn read(ctx: &AppContext) -> Self {
        if !FeatureFlag::RemoteControl.is_enabled() {
            return Self {
                enabled: false,
                port: 0,
                allow_lan_access: false,
            };
        }
        let settings = RemoteControlSettings::as_ref(ctx);
        Self {
            enabled: settings.is_enabled(),
            port: settings.port(),
            allow_lan_access: settings.allows_lan_access(),
        }
    }
}

pub struct RemoteControlServer {
    state: ServerState,
    runtime: Option<tokio::runtime::Runtime>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    broadcast: Option<broadcast::Sender<Arc<ServerMessage>>>,
    client_count: Arc<AtomicUsize>,
    active_config: Option<ListenerConfig>,
    instance_id: String,
}

impl Entity for RemoteControlServer {
    type Event = RemoteControlServerEvent;
}

impl SingletonEntity for RemoteControlServer {}

impl RemoteControlServer {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut server = Self {
            state: ServerState::Stopped,
            runtime: None,
            shutdown: None,
            broadcast: None,
            client_count: Arc::new(AtomicUsize::new(0)),
            active_config: None,
            instance_id: new_instance_id(),
        };
        server.refresh_for_settings(ctx);
        ctx.subscribe_to_model(&RemoteControlSettings::handle(ctx), |server, _, _, ctx| {
            server.refresh_for_settings(ctx)
        });
        server
    }

    pub fn state(&self) -> &ServerState {
        &self.state
    }

    pub fn pairing_url(&self, ctx: &AppContext) -> Option<String> {
        let endpoint = self.state.endpoint()?;
        let token = RemoteControlSecrets::as_ref(ctx).access_token()?;
        Some(endpoint.pairing_url(token.reveal()))
    }

    fn refresh_for_settings(&mut self, ctx: &mut ModelContext<Self>) {
        let config = ListenerConfig::read(ctx);
        if !config.enabled {
            if self.runtime.is_some() || self.state != ServerState::Stopped {
                self.stop(ctx);
            }
            return;
        }
        match self.active_config {
            Some(active) if active == config && self.runtime.is_some() => {}
            Some(_) => {
                self.stop(ctx);
                self.start(config, ctx);
            }
            None => self.start(config, ctx),
        }
    }

    fn start(&mut self, config: ListenerConfig, ctx: &mut ModelContext<Self>) {
        let Some(token) = RemoteControlSecrets::ensure_access_token(ctx) else {
            self.fail(
                "could not store the Remote Control access token".to_owned(),
                ctx,
            );
            return;
        };
        let bind_address = if config.allow_lan_access {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        let listener = match std::net::TcpListener::bind(SocketAddr::new(bind_address, config.port))
        {
            Ok(listener) => listener,
            Err(error) => {
                self.fail(
                    format!("could not listen on port {}: {error}", config.port),
                    ctx,
                );
                return;
            }
        };
        if let Err(error) = listener.set_nonblocking(true) {
            self.fail(format!("could not configure the listener: {error}"), ctx);
            return;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("remote-control")
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                self.fail(
                    format!("could not start the Remote Control runtime: {error}"),
                    ctx,
                );
                return;
            }
        };
        let listener = {
            let guard = runtime.enter();
            let converted = tokio::net::TcpListener::from_std(listener);
            drop(guard);
            match converted {
                Ok(listener) => listener,
                Err(error) => {
                    self.fail(format!("could not register the listener: {error}"), ctx);
                    return;
                }
            }
        };

        let lan_endpoints = if config.allow_lan_access {
            lan::lan_endpoints(config.port)
        } else {
            Vec::new()
        };
        let allowed_hosts = AllowedHosts::with_extra(
            config.port,
            lan::allowed_host_names(&lan_endpoints, config.allow_lan_access),
        );
        let endpoint = RemoteEndpoint::new("127.0.0.1", config.port);
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let bridge_handle = RemoteControlBridge::handle(ctx);
        let broadcast_for_bridge = broadcast_tx.clone();
        let sessions = SessionStore::new(RemoteControlSecrets::as_ref(ctx).paired_devices());
        let sessions_for_bridge = sessions.clone();
        let bridge_spawner = bridge_handle.update(ctx, |bridge, ctx| {
            bridge.attach_server(
                broadcast_for_bridge,
                sessions_for_bridge,
                self.instance_id.clone(),
                ctx,
            );
            ctx.spawner()
        });
        let server_spawner = ctx.spawner();
        self.client_count.store(0, Ordering::Relaxed);

        let state = Arc::new(AppState {
            bridge_spawner,
            server_spawner,
            sessions,
            allowed_hosts,
            access_token: token.clone(),
            instance_id: self.instance_id.clone(),
            build_id: build_id(),
            app_version: app_version(),
            client_count: self.client_count.clone(),
            lan_access: config.allow_lan_access,
            broadcast: broadcast_tx.clone(),
            endpoint: endpoint.clone(),
            lan_endpoints: lan_endpoints.clone(),
        });
        let router = http::router(state);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let served = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(error) = served.await {
                log::warn!("Remote Control listener stopped: {error:#}");
            }
        });

        let url = endpoint.url();
        self.runtime = Some(runtime);
        self.shutdown = Some(shutdown_tx);
        self.broadcast = Some(broadcast_tx);
        self.active_config = Some(config);
        self.state = ServerState::Running {
            endpoint,
            lan_endpoints,
            started_ts: now_ms(),
            connected_clients: 0,
        };
        warp_core::safe_info!(
            safe: ("Remote Control is running"),
            full: ("Remote Control is running at {url}")
        );
        ctx.emit(RemoteControlServerEvent::StateChanged);
        ctx.notify();
    }

    fn stop(&mut self, ctx: &mut ModelContext<Self>) {
        if let Some(broadcast) = self.broadcast.take() {
            let _ = broadcast.send(Arc::new(ServerMessage::Event {
                event: ServerEvent::ServerShuttingDown,
            }));
        }
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        RemoteControlBridge::handle(ctx).update(ctx, |bridge, _| bridge.detach_server());
        self.runtime = None;
        self.active_config = None;
        self.client_count.store(0, Ordering::Relaxed);
        if self.state != ServerState::Stopped {
            self.state = ServerState::Stopped;
            ctx.emit(RemoteControlServerEvent::StateChanged);
            ctx.notify();
        }
    }

    fn fail(&mut self, message: String, ctx: &mut ModelContext<Self>) {
        log::warn!("Remote Control could not start: {message}");
        self.runtime = None;
        self.shutdown = None;
        self.broadcast = None;
        self.active_config = None;
        self.state = ServerState::Failed { message };
        ctx.emit(RemoteControlServerEvent::StateChanged);
        ctx.notify();
    }

    pub(crate) fn set_connected_clients(&mut self, count: usize, ctx: &mut ModelContext<Self>) {
        let ServerState::Running {
            connected_clients, ..
        } = &mut self.state
        else {
            return;
        };
        if *connected_clients == count {
            return;
        }
        *connected_clients = count;
        ctx.emit(RemoteControlServerEvent::StateChanged);
        ctx.notify();
    }

    pub(crate) fn persist_paired_devices(
        &mut self,
        devices: Vec<remote_control::auth::PairedDevice>,
        ctx: &mut ModelContext<Self>,
    ) {
        RemoteControlSecrets::set_paired_devices(&devices, ctx);
    }

    pub fn rotate_access_token(ctx: &mut AppContext) {
        RemoteControlSecrets::rotate_access_token(ctx);
        RemoteControlSecrets::set_paired_devices(&[], ctx);
        Self::handle(ctx).update(ctx, |server, ctx| {
            let config = ListenerConfig::read(ctx);
            server.stop(ctx);
            if config.enabled {
                server.start(config, ctx);
            }
        });
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn new_instance_id() -> String {
    format!("inst_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn app_version() -> String {
    ChannelState::app_version().unwrap_or("dev").to_owned()
}

pub(crate) fn build_id() -> String {
    ChannelState::app_version()
        .map(|version| version.replace(|c: char| !c.is_ascii_alphanumeric(), "-"))
        .unwrap_or_else(|| "dev".to_owned())
}

pub(crate) fn remote_control_available(ctx: &AppContext) -> bool {
    FeatureFlag::RemoteControl.is_enabled() && RemoteControlSettings::as_ref(ctx).is_enabled()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
