// Minecraft Bedrock Edition support (Alpha 7).
//
// MDL supports the Bedrock Dedicated Server (BDS) for Windows: download the
// official zip, extract into an instance directory and launch
// `bedrock_server.exe`. The BE *client* on Windows is a UWP app that cannot
// be freely downloaded or launched by a third-party launcher, so client
// support is limited to injection-based loaders (see util::injector, which
// is the groundwork for Aprism BE Native).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Candidate Windows BDS versions to probe, newest first. The official page
/// (https://www.minecraft.net/en-us/download/server/bedrock) is SPA-heavy, so
/// MDL probes the stable direct-link pattern for a known-good version.
pub const BDS_CANDIDATE_VERSIONS: &[&str] = &[
    "1.26.43.1",
    "1.21.95.01",
    "1.21.90.03",
    "1.21.70.03",
];

pub fn bds_url_for(version: &str) -> String {
    format!(
        "https://www.minecraft.net/bedrockdedicatedserver/bin-win/bedrock-server-{}.zip",
        version
    )
}

/// Probe candidate versions and return the newest one that responds 200.
/// Falls back to the first candidate when probing is not possible.
pub async fn latest_bds_url() -> (String, String) {
    if let Ok(client) = crate::util::http::create_http_client() {
        for v in BDS_CANDIDATE_VERSIONS {
            let url = bds_url_for(v);
            if let Ok(resp) = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.head(&url).send(),
            )
            .await
            {
                if let Ok(r) = resp {
                    if r.status().is_success() {
                        return (v.to_string(), url);
                    }
                }
            }
        }
    }
    let v = BDS_CANDIDATE_VERSIONS[0];
    (v.to_string(), bds_url_for(v))
}

/// Download and extract the Bedrock Dedicated Server into `dir`.
pub async fn install_bds(dir: &Path) -> Result<PathBuf> {
    let (version, url) = latest_bds_url().await;
    std::fs::create_dir_all(dir)?;

    let zip_path = dir.join(format!("bedrock-server-{}.zip", version));
    if !zip_path.exists() {
        crate::version::downloader::download_file(&url, &zip_path, None).await?;
    }

    let extract_dir = dir.join("server");
    crate::util::archive::extract_zip(&zip_path, &extract_dir).await?;
    Ok(extract_dir)
}

/// Launch the extracted BDS. Returns the spawned child PID.
pub fn launch_bds(server_dir: &Path) -> Result<u32> {
    let exe = server_dir.join("bedrock_server.exe");
    if !exe.exists() {
        anyhow::bail!("bedrock_server.exe not found in {}", server_dir.display());
    }
    let child = std::process::Command::new(&exe)
        .current_dir(server_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn bedrock_server.exe")?;
    Ok(child.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bds_url_for() {
        let url = bds_url_for("1.26.43.1");
        assert!(url.ends_with("bedrock-server-1.26.43.1.zip"));
    }
}
