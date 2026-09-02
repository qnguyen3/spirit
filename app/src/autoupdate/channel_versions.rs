use std::env;
use std::fs::read_to_string;
use std::time::Duration;

use anyhow::{Context as _, Result};
use channel_versions::ChannelVersions;

use crate::channel::ChannelState;

pub const FETCH_CHANNEL_VERSIONS_TIMEOUT: Duration = Duration::from_secs(60);

// Fetches channel versions asynchronously from JSON storage under the release assets host.
pub async fn fetch_channel_versions(
    nonce: &str,
    client: &http_client::Client,
) -> Result<ChannelVersions> {
    if let Ok(path) = env::var("WARP_CHANNEL_VERSIONS_PATH") {
        // Load channel versions from local filesystem. Used for testing both
        // autoupdate and changelog behavior.
        let path = shellexpand::tilde(&path);
        let channel_versions_string = read_to_string::<&str>(&path)?;
        return serde_json::from_str(channel_versions_string.as_str())
            .context("Failed to parse channel versions JSON");
    }

    fetch_channel_versions_from_json_storage(client, nonce).await
}

// Note, in order to run against a test file you can use the "channel_versions_test.json" file
// and update the file using gsutil cp channel_versions_test.json gs://warp-releases/channel_versions_test.json
async fn fetch_channel_versions_from_json_storage(
    client: &http_client::Client,
    nonce: &str,
) -> Result<ChannelVersions> {
    log::info!("Fetching channel versions from JSON storage");
    let res = client
        .get(
            format!(
                "{}/channel_versions.json?r={}",
                ChannelState::releases_base_url(),
                nonce
            )
            .as_str(),
        )
        .timeout(FETCH_CHANNEL_VERSIONS_TIMEOUT)
        .send()
        .await?;
    let versions: ChannelVersions = res.json().await?;
    log::info!("Received channel versions from JSON storage: {versions}");
    Ok(versions)
}
