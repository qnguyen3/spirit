use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::{
    ConnectInfo, DefaultBodyLimit, Form, FromRequestParts, Path, Query, Request, State,
};
use axum::http::StatusCode;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, HeaderMap, HeaderName, HeaderValue, ORIGIN,
    SET_COOKIE, USER_AGENT,
};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use remote_control::auth::{AccessToken, DeviceId};
use remote_control::hosts::{AllowedHosts, RemoteEndpoint};
use remote_control::limits::{COOKIE_MAX_AGE_SECONDS, MAX_JSON_FRAME_BYTES};
use remote_control::protocol::{ApiError, ErrorCode, ServerMessage};
use remote_control::{PROTOCOL_VERSION, limits};
use serde::Deserialize;
use tokio::sync::broadcast;
use warpui::ModelSpawner;

use super::bridge::RemoteControlBridge;
use super::sessions::{PairFailure, SessionStore};
use super::{RemoteControlServer, assets, ws};

pub(crate) const SESSION_COOKIE: &str = "spirit_rc";
pub(crate) const REMOTE_HEADER: &str = "x-spirit-remote";

pub(crate) struct AppState {
    pub bridge_spawner: ModelSpawner<RemoteControlBridge>,
    pub server_spawner: ModelSpawner<RemoteControlServer>,
    pub sessions: SessionStore,
    pub allowed_hosts: AllowedHosts,
    pub access_token: AccessToken,
    pub instance_id: String,
    pub build_id: String,
    pub app_version: String,
    pub client_count: Arc<AtomicUsize>,
    pub lan_access: bool,
    pub broadcast: broadcast::Sender<Arc<ServerMessage>>,
    pub endpoint: RemoteEndpoint,
    pub lan_endpoints: Vec<RemoteEndpoint>,
}

impl AppState {
    pub(crate) fn pairing_endpoint(&self) -> &RemoteEndpoint {
        self.lan_endpoints
            .first()
            .filter(|_| self.lan_access)
            .unwrap_or(&self.endpoint)
    }
}

impl AppState {
    pub(crate) async fn flush_sessions(&self) {
        let Some(devices) = self.sessions.take_persistable() else {
            return;
        };
        let _ = self
            .server_spawner
            .spawn(move |server, ctx| server.persist_paired_devices(devices, ctx))
            .await;
    }

    pub(crate) async fn publish_client_count(&self) {
        let count = self.client_count.load(Ordering::Relaxed);
        let _ = self
            .server_spawner
            .spawn(move |server, ctx| server.set_connected_clients(count, ctx))
            .await;
    }
}

pub(crate) fn router(state: Arc<AppState>) -> Router {
    let host_state = state.clone();
    Router::new()
        .route("/health", get(health))
        .route("/", get(index))
        .route("/pair", get(pair_via_query).post(pair_via_form))
        .route("/assets/{build_id}/{*path}", get(asset))
        .route("/api/v1/status", get(status))
        .route("/api/v1/pairing-qr.svg", get(pairing_qr))
        .route("/api/v1/state", get(state_snapshot))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/devices", get(list_devices))
        .route(
            "/api/v1/devices/{device_id}",
            axum::routing::delete(revoke_device),
        )
        .route("/api/v1/ws", get(ws::upgrade))
        .layer(DefaultBodyLimit::max(MAX_JSON_FRAME_BYTES as usize))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn_with_state(host_state, host_guard))
        .with_state(state)
}

async fn host_guard(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !state.allowed_hosts.allows(host) {
        return (StatusCode::MISDIRECTED_REQUEST, "").into_response();
    }
    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
             connect-src 'self' ws: wss:; font-src 'self'; frame-ancestors 'none'; \
             base-uri 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    if path.starts_with("/api/") || path == "/" || path == "/pair" {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

pub(crate) struct Authed {
    pub device_id: Option<DeviceId>,
    pub session_cookie: Option<String>,
}

impl FromRequestParts<Arc<AppState>> for Authed {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        authenticate(&parts.headers, state).ok_or_else(|| {
            api_error(ErrorCode::Unauthorized, "a paired session is required").into_response()
        })
    }
}

pub(crate) struct Mutating {
    pub authed: Authed,
}

impl FromRequestParts<Arc<AppState>> for Mutating {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        SameSiteOrigin::from_request_parts(parts, state).await?;
        if !carries_remote_header(&parts.headers) {
            return Err(api_error(
                ErrorCode::ForbiddenOrigin,
                "this request must come from the Remote Control web app",
            ));
        }
        let authed = Authed::from_request_parts(parts, state).await?;
        Ok(Self { authed })
    }
}

pub(crate) struct SameSiteOrigin;

impl FromRequestParts<Arc<AppState>> for SameSiteOrigin {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        if origin_is_same_site(&parts.headers, state) {
            Ok(Self)
        } else {
            Err(api_error(
                ErrorCode::ForbiddenOrigin,
                "this request must come from the Remote Control web app",
            ))
        }
    }
}

pub(crate) fn authenticate(headers: &HeaderMap, state: &AppState) -> Option<Authed> {
    if let Some(session_id) = session_cookie(headers)
        && let Some(entry) = state.sessions.touch(&session_id)
    {
        return Some(Authed {
            device_id: Some(entry.device_id),
            session_cookie: Some(session_id),
        });
    }
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))?;
    state
        .access_token
        .constant_time_eq(bearer)
        .then_some(Authed {
            device_id: None,
            session_cookie: None,
        })
}

pub(crate) fn origin_is_same_site(headers: &HeaderMap, state: &AppState) -> bool {
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    state.allowed_hosts.origin_matches(origin, host)
}

fn carries_remote_header(headers: &HeaderMap) -> bool {
    headers
        .get(HeaderName::from_static(REMOTE_HEADER))
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "1")
}

pub(crate) fn session_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == SESSION_COOKIE).then(|| value.trim().to_owned())
    })
}

pub(crate) fn api_error(code: ErrorCode, message: &str) -> Response {
    let status = StatusCode::from_u16(code.http_status()).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(ApiError::new(code, message))).into_response()
}

async fn health(State(state): State<Arc<AppState>>) -> Response {
    Json(serde_json::json!({
        "app": "spirit",
        "protocol": PROTOCOL_VERSION,
        "version": state.app_version,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct PairQuery {
    #[serde(default)]
    t: String,
}

#[derive(Deserialize)]
struct PairForm {
    #[serde(default)]
    t: String,
}

async fn pair_via_query(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<PairQuery>,
) -> Response {
    complete_pairing(&state, peer, &headers, &query.t).await
}

async fn pair_via_form(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PairForm>,
) -> Response {
    complete_pairing(&state, peer, &headers, &form.t).await
}

async fn complete_pairing(
    state: &Arc<AppState>,
    peer: SocketAddr,
    headers: &HeaderMap,
    attempt: &str,
) -> Response {
    let remote_address = Some(peer.ip());
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok());
    match state
        .sessions
        .pair(&state.access_token, attempt, remote_address, user_agent)
    {
        Ok(session) => {
            let label = session.label.clone();
            let device_id = session.device_id.as_str().to_owned();
            state.flush_sessions().await;
            log::info!("Remote Control paired a new device: {label} ({device_id})");
            let cookie = format!(
                "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE_SECONDS}",
                session.session_id.reveal()
            );
            let mut response = (StatusCode::SEE_OTHER, "").into_response();
            let response_headers = response.headers_mut();
            response_headers.insert(axum::http::header::LOCATION, HeaderValue::from_static("/"));
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response_headers.insert(SET_COOKIE, value);
            }
            response
        }
        Err(failure) => {
            let message = match failure {
                PairFailure::BadToken => "That access token is not valid.",
                PairFailure::RateLimited => "Too many attempts. Wait a minute and try again.",
            };
            log::warn!(
                "Remote Control pairing was refused for {}",
                remote_address
                    .map(|address| address.to_string())
                    .unwrap_or_else(|| "an unknown address".to_owned())
            );
            pair_page(state, Some(message), StatusCode::FORBIDDEN)
        }
    }
}

async fn index(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if authenticate(&headers, &state).is_some() {
        return assets::serve_document(&state.build_id, "index.html");
    }
    pair_page(&state, None, StatusCode::OK)
}

fn pair_page(state: &AppState, error: Option<&str>, status: StatusCode) -> Response {
    let mut response = assets::serve_pairing_page(&state.build_id, error);
    *response.status_mut() = status;
    response
}

async fn asset(Path((build_id, path)): Path<(String, String)>) -> Response {
    assets::serve(&build_id, &path)
}

async fn status(State(state): State<Arc<AppState>>, _: Authed) -> Response {
    let limits: Vec<serde_json::Value> = limits::all()
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect();
    Json(serde_json::json!({
        "instance_id": state.instance_id,
        "version": state.app_version,
        "protocol": PROTOCOL_VERSION,
        "connected_clients": state.client_count.load(Ordering::Relaxed),
        "lan_access": state.lan_access,
        "limits": limits,
    }))
    .into_response()
}

async fn pairing_qr(State(state): State<Arc<AppState>>, _: Authed) -> Response {
    let endpoint = state.pairing_endpoint();
    let Some(svg) = super::qr::pairing_svg(endpoint, state.access_token.reveal()) else {
        return api_error(
            ErrorCode::Unsupported,
            "that pairing URL is too long to encode",
        );
    };
    text_response(
        StatusCode::OK,
        HeaderValue::from_static("image/svg+xml"),
        "no-store",
        svg.into_bytes(),
    )
}

async fn state_snapshot(State(state): State<Arc<AppState>>, _: Authed) -> Response {
    match state
        .bridge_spawner
        .spawn(|bridge, ctx| bridge.snapshot_now(ctx))
        .await
    {
        Ok(Ok(snapshot)) => Json(snapshot).into_response(),
        Ok(Err(error)) => api_error(error.code, &error.message),
        Err(_) => api_error(ErrorCode::BridgeUnavailable, "Spirit is shutting down"),
    }
}

async fn logout(State(state): State<Arc<AppState>>, mutating: Mutating) -> Response {
    if let Some(cookie) = mutating.authed.session_cookie.as_deref() {
        state.sessions.revoke_session(cookie);
        state.flush_sessions().await;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
    )) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

async fn list_devices(State(state): State<Arc<AppState>>, authed: Authed) -> Response {
    Json(serde_json::json!({
        "devices": state.sessions.devices(authed.device_id.as_ref()),
    }))
    .into_response()
}

async fn revoke_device(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
    _: Mutating,
) -> Response {
    if state.sessions.revoke_device(&device_id) {
        state.flush_sessions().await;
        StatusCode::NO_CONTENT.into_response()
    } else {
        api_error(ErrorCode::NotFound, "that device is not paired")
    }
}

pub(crate) fn content_type_for(path: &str) -> HeaderValue {
    let guessed = mime_guess::from_path(path).first_raw();
    guessed
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"))
}

pub(crate) fn text_response(
    status: StatusCode,
    content_type: HeaderValue,
    cache_control: &str,
    body: Vec<u8>,
) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, content_type);
    if let Ok(value) = HeaderValue::from_str(cache_control) {
        headers.insert(CACHE_CONTROL, value);
    }
    response
}
