use std::path::Path;

use remote_server::transport::{Error, InstallOutcome};

/// Reports that the remote host has no usable remote-server binary.
///
/// Spirit has no artifact CDN to install from: deploy the binary yourself with
/// `script/deploy_remote_server --host <user@hostname>`. Failing here leaves the session on a
/// plain SSH connection rather than blocking it.
pub(super) async fn install_binary(_socket_path: &Path) -> InstallOutcome {
    let binary_path = remote_server::setup::remote_server_binary();
    InstallOutcome {
        source: None,
        result: Err(Error::Other(anyhow::anyhow!(
            "No remote server binary at {binary_path}. Deploy one with \
             `script/deploy_remote_server --host <user@hostname>`."
        ))),
    }
}
