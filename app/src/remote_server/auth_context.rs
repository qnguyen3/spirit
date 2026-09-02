use std::sync::OnceLock;

use remote_server::auth::RemoteServerAuthContext;
use warpui::r#async::BoxFuture;

const IDENTITY_KEY_FILE: &str = "remote_server_identity";

/// Builds the app-wide auth context used by remote-server connections.
///
/// The client is permanently logged out, so no bearer token is ever offered; the
/// daemon is reached over the user's own SSH connection instead.
pub fn server_api_auth_context() -> RemoteServerAuthContext {
    RemoteServerAuthContext::new(
        || -> BoxFuture<'static, Option<String>> { Box::pin(async { None }) },
        || remote_server_identity_key().to_owned(),
        String::new(),
        String::new(),
    )
}

/// Stable, non-secret key partitioning the remote daemon's socket/PID directory.
///
/// Persisted locally so reconnects and relaunches address the same daemon.
fn remote_server_identity_key() -> &'static str {
    static IDENTITY_KEY: OnceLock<String> = OnceLock::new();
    IDENTITY_KEY.get_or_init(|| {
        let path = warp_core::paths::state_dir().join(IDENTITY_KEY_FILE);
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let existing = existing.trim();
            if !existing.is_empty() {
                return existing.to_owned();
            }
        }

        let generated = uuid::Uuid::new_v4().to_string();
        if let Err(err) = std::fs::write(&path, &generated) {
            log::warn!("Unable to persist the remote-server identity key: {err:?}");
        }
        generated
    })
}
