use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use axum::Router;
use remote_control::auth::AccessToken;
use remote_control::hosts::AllowedHosts;
use settings::Setting as _;
use tokio::sync::broadcast;
use warp_core::features::FeatureFlag;
use warpui::{App, SingletonEntity as _};

use super::bridge::RemoteControlBridge;
use super::http::{self, AppState, SESSION_COOKIE};
use super::sessions::SessionStore;
use super::{RemoteControlServer, ServerState};
use crate::projects::registry::ProjectRegistryModel;
use crate::settings::{RemoteControlSecrets, RemoteControlSettings};
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::WorkspaceRegistry;

fn initialize_server_models(app: &mut App) {
    initialize_settings_for_tests(app);
    RemoteControlSettings::register(app);
    RemoteControlSecrets::register(app);
    app.add_singleton_model(|_| WorkspaceRegistry::new());
    app.add_singleton_model(|_| ProjectRegistryModel::new(None));
    app.add_singleton_model(|_| CLIAgentSessionsModel::new());
    app.add_singleton_model(RemoteControlBridge::new);

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("reserve a test port");
    let port = listener.local_addr().expect("local address").port();
    RemoteControlSettings::handle(app).update(app, |settings, ctx| {
        settings.remote_control_port.set_value(port, ctx).unwrap();
    });
}

fn set_server_enabled(enabled: bool, app: &mut App) {
    RemoteControlSettings::handle(app).update(app, |settings, ctx| {
        settings
            .remote_control_enabled
            .set_value(enabled, ctx)
            .unwrap();
    });
}

fn assert_server_running(app: &App) {
    app.read(|ctx| {
        let server = RemoteControlServer::as_ref(ctx);
        assert!(server.state().is_running(), "{:?}", server.state());
        assert_eq!(
            RemoteControlBridge::as_ref(ctx).instance_id(),
            server.instance_id
        );
        assert!(server.pairing_url(ctx).is_some());
    });
}

#[test]
fn server_starts_with_remote_control_already_enabled() {
    let _flag = FeatureFlag::RemoteControl.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_server_models(&mut app);
        set_server_enabled(true, &mut app);

        app.add_singleton_model(RemoteControlServer::new);

        assert_server_running(&app);
        set_server_enabled(false, &mut app);
    });
}

#[test]
fn server_can_be_enabled_again_after_stopping() {
    let _flag = FeatureFlag::RemoteControl.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_server_models(&mut app);
        app.add_singleton_model(RemoteControlServer::new);

        set_server_enabled(true, &mut app);
        assert_server_running(&app);
        set_server_enabled(false, &mut app);
        app.read(|ctx| {
            assert_eq!(
                RemoteControlServer::as_ref(ctx).state(),
                &ServerState::Stopped
            );
        });

        set_server_enabled(true, &mut app);
        assert_server_running(&app);
        set_server_enabled(false, &mut app);
    });
}

struct Harness {
    runtime: tokio::runtime::Runtime,
    address: SocketAddr,
    token: AccessToken,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Harness {
    fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a test runtime builds");
        let listener =
            std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
        listener.set_nonblocking(true).expect("non-blocking");
        let address = listener.local_addr().expect("local address");
        let token = AccessToken::generate();
        let (broadcast_tx, _) = broadcast::channel(8);

        let state = Arc::new(AppState {
            bridge_spawner: warpui::ModelSpawner::disconnected(),
            server_spawner: warpui::ModelSpawner::disconnected(),
            sessions: SessionStore::default(),
            allowed_hosts: AllowedHosts::loopback(address.port()),
            access_token: token.clone(),
            instance_id: "inst_test".to_owned(),
            build_id: "dev".to_owned(),
            app_version: "0.0.0-test".to_owned(),
            client_count: Arc::new(AtomicUsize::new(0)),
            lan_access: false,
            broadcast: broadcast_tx,
            endpoint: remote_control::hosts::RemoteEndpoint::new("127.0.0.1", address.port()),
            lan_endpoints: Vec::new(),
        });
        let router: Router = http::router(state);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let listener = {
            let guard = runtime.enter();
            let converted = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
            drop(guard);
            converted
        };
        runtime.spawn(async move {
            let _ = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        });
        Self {
            runtime,
            address,
            token,
            shutdown: Some(shutdown_tx),
        }
    }

    fn authority(&self) -> String {
        format!("127.0.0.1:{}", self.address.port())
    }

    fn origin(&self) -> String {
        format!("http://{}", self.authority())
    }

    fn request(&self, request: Request) -> Reply {
        self.runtime.block_on(send(self.address, request))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

struct Request {
    method: &'static str,
    path: String,
    host: String,
    headers: Vec<(&'static str, String)>,
    body: Option<String>,
}

impl Request {
    fn get(path: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            method: "GET",
            path: path.into(),
            host: host.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    fn post(path: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            method: "POST",
            path: path.into(),
            host: host.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn session_cookie(&self) -> Option<String> {
        let raw = self.header("set-cookie")?;
        let value = raw.strip_prefix(&format!("{SESSION_COOKIE}="))?;
        Some(value.split(';').next().unwrap_or_default().to_owned())
    }
}

async fn send(address: SocketAddr, request: Request) -> Reply {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect to the test server");
    let mut raw = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        request.method, request.path, request.host
    );
    for (name, value) in &request.headers {
        raw.push_str(&format!("{name}: {value}\r\n"));
    }
    match &request.body {
        Some(body) => {
            raw.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
            raw.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
            raw.push_str(body);
        }
        None => raw.push_str("Content-Length: 0\r\n\r\n"),
    }
    stream
        .write_all(raw.as_bytes())
        .await
        .expect("write the request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read the response");
    parse(&String::from_utf8_lossy(&response))
}

fn parse(raw: &str) -> Reply {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: body.to_owned(),
    }
}

#[test]
fn health_answers_without_authentication() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/health", harness.authority()));
    assert_eq!(reply.status, 200);
    assert!(reply.body.contains("\"app\":\"spirit\""));
    assert!(reply.body.contains("\"protocol\":1"));
}

#[test]
fn an_unexpected_host_is_misdirected() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/health", "evil.example"));
    assert_eq!(reply.status, 421);
    assert!(reply.body.is_empty());
}

#[test]
fn every_response_carries_the_security_headers() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/health", harness.authority()));
    let policy = reply
        .header("content-security-policy")
        .expect("a policy is always present");
    assert!(policy.contains("default-src 'self'"));
    assert!(policy.contains("frame-ancestors 'none'"));
    assert_eq!(reply.header("x-frame-options"), Some("DENY"));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
}

#[test]
fn the_api_refuses_an_unauthenticated_caller() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/api/v1/status", harness.authority()));
    assert_eq!(reply.status, 401);
    assert!(reply.body.contains("unauthorized"));
}

#[test]
fn the_api_accepts_the_bearer_token() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/api/v1/status", harness.authority()).header(
        "Authorization",
        format!("Bearer {}", harness.token.reveal()),
    ));
    assert_eq!(reply.status, 200);
    assert!(reply.body.contains("inst_test"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
}

#[test]
fn pairing_with_a_wrong_token_sets_no_cookie() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/pair?t=wrong", harness.authority()));
    assert_eq!(reply.status, 403);
    assert!(reply.session_cookie().is_none());
}

#[test]
fn pairing_with_the_right_token_redirects_and_sets_a_cookie() {
    let harness = Harness::start();
    let reply = harness.request(Request::get(
        format!("/pair?t={}", harness.token.reveal()),
        harness.authority(),
    ));
    assert_eq!(reply.status, 303);
    assert_eq!(reply.header("location"), Some("/"));
    let cookie = reply.header("set-cookie").expect("a cookie is set");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Path=/"));
    assert!(!cookie.contains("Secure"));
}

#[test]
fn a_paired_cookie_authenticates_the_api() {
    let harness = Harness::start();
    let paired = harness.request(Request::get(
        format!("/pair?t={}", harness.token.reveal()),
        harness.authority(),
    ));
    let session = paired.session_cookie().expect("a session id");
    let reply = harness.request(
        Request::get("/api/v1/status", harness.authority())
            .header("Cookie", format!("{SESSION_COOKIE}={session}")),
    );
    assert_eq!(reply.status, 200);
}

#[test]
fn logging_out_requires_the_origin_and_the_marker_header() {
    let harness = Harness::start();
    let paired = harness.request(Request::get(
        format!("/pair?t={}", harness.token.reveal()),
        harness.authority(),
    ));
    let session = paired.session_cookie().expect("a session id");
    let cookie = format!("{SESSION_COOKIE}={session}");

    let without_origin = harness.request(
        Request::post("/api/v1/logout", harness.authority()).header("Cookie", cookie.clone()),
    );
    assert_eq!(without_origin.status, 403);

    let without_marker = harness.request(
        Request::post("/api/v1/logout", harness.authority())
            .header("Cookie", cookie.clone())
            .header("Origin", harness.origin()),
    );
    assert_eq!(without_marker.status, 403);

    let complete = harness.request(
        Request::post("/api/v1/logout", harness.authority())
            .header("Cookie", cookie.clone())
            .header("Origin", harness.origin())
            .header("X-Spirit-Remote", "1"),
    );
    assert_eq!(complete.status, 204);

    let after = harness
        .request(Request::get("/api/v1/status", harness.authority()).header("Cookie", cookie));
    assert_eq!(after.status, 401);
}

#[test]
fn a_cross_site_origin_cannot_mutate() {
    let harness = Harness::start();
    let paired = harness.request(Request::get(
        format!("/pair?t={}", harness.token.reveal()),
        harness.authority(),
    ));
    let session = paired.session_cookie().expect("a session id");
    let reply = harness.request(
        Request::post("/api/v1/logout", harness.authority())
            .header("Cookie", format!("{SESSION_COOKIE}={session}"))
            .header("Origin", "http://evil.example")
            .header("X-Spirit-Remote", "1"),
    );
    assert_eq!(reply.status, 403);
}

#[test]
fn the_root_serves_the_pairing_page_when_unauthenticated() {
    let harness = Harness::start();
    let reply = harness.request(Request::get("/", harness.authority()));
    assert_eq!(reply.status, 200);
    assert!(reply.body.contains("<form"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
}

#[test]
fn asset_paths_cannot_escape_the_web_directory() {
    let harness = Harness::start();
    for path in ["/assets/dev/../secret", "/assets/dev/..%2Fsecret"] {
        let reply = harness.request(Request::get(path, harness.authority()));
        assert_ne!(reply.status, 200, "{path} must not be served");
    }
}

#[test]
fn the_pairing_qr_needs_authentication_and_is_an_svg() {
    let harness = Harness::start();
    let anonymous = harness.request(Request::get("/api/v1/pairing-qr.svg", harness.authority()));
    assert_eq!(anonymous.status, 401);

    let reply = harness.request(
        Request::get("/api/v1/pairing-qr.svg", harness.authority()).header(
            "Authorization",
            format!("Bearer {}", harness.token.reveal()),
        ),
    );
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-type"), Some("image/svg+xml"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert!(reply.body.contains("<svg"));
}

#[test]
fn a_websocket_upgrade_without_an_origin_is_refused() {
    let harness = Harness::start();
    let reply = harness.request(
        Request::get("/api/v1/ws", harness.authority())
            .header(
                "Authorization",
                format!("Bearer {}", harness.token.reveal()),
            )
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
    );
    assert_eq!(reply.status, 403);
}
