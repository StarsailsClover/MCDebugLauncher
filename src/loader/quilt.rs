// Quilt mod loader installer

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize)]
struct QuiltLoaderVersion {
    version: String,
}

pub struct QuiltInstaller {
    version: Option<String>,
}

impl QuiltInstaller {
    pub fn new(version: Option<String>) -> Self {
        Self { version }
    }

    pub async fn fetch_versions() -> Result<Vec<String>> {
        let url = "https://meta.quiltmc.org/v3/versions/loader";
        let response = reqwest::get(url)
            .await
            .context("Failed to fetch Quilt versions")?;
        let versions: Vec<QuiltLoaderVersion> = response.json().await?;

        Ok(versions.into_iter().map(|v| v.version).collect())
    }

    pub async fn install_loader(&self, mc_version: &str, quilt_version: &str, target_dir: &Path) -> Result<String> {
        tracing::info!("Installing Quilt {} for Minecraft {}", quilt_version, mc_version);

        // Download Quilt loader profile
        let profile_url = format!(
            "https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json",
            mc_version, quilt_version
        );

        let version_json_path = target_dir.join("version.json");

        tracing::info!("Downloading Quilt profile from {}", profile_url);
        let response = reqwest::get(&profile_url)
            .await
            .with_context(|| format!("Failed to download Quilt profile from {}", profile_url))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download Quilt profile: HTTP {}", response.status());
        }

        let bytes = response.bytes().await?;
        fs::write(&version_json_path, &bytes).await?;

        tracing::info!("Quilt {} installed successfully", quilt_version);
        Ok(format!("quilt-loader-{}-{}", quilt_version, mc_version))
    }
}

