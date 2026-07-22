// Quilt mod loader installer

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize)]
struct QuiltVersionManifest {
    loader: Vec<QuiltLoaderVersion>,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderVersion {
    version: String,
}

pub struct QuiltInstaller {
    version: String,
}

impl QuiltInstaller {
    pub fn new(version: String) -> Self {
        Self { version }
    }

    pub async fn install_async(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let version_dir = Path::new(target_dir);
        fs::create_dir_all(&version_dir).await?;

        tracing::info!("Installing Quilt {} for Minecraft {}", self.version, mc_version);

        // Fetch available Quilt versions if needed
        let loader_version = if self.version == "latest" {
            self.fetch_latest_quilt().await?
        } else {
            self.version.clone()
        };

        // Download Quilt loader profile
        let profile_url = format!(
            "https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json",
            mc_version, loader_version
        );

        let version_json_path = version_dir.join("version.json");

        tracing::info!("Downloading Quilt profile from {}", profile_url);
        self.download_file(&profile_url, &version_json_path).await?;

        tracing::info!("Quilt {} installed successfully", loader_version);
        Ok(format!("quilt-loader-{}-{}", loader_version, mc_version))
    }

    async fn fetch_latest_quilt(&self) -> Result<String> {
        let url = "https://meta.quiltmc.org/v3/versions/loader";
        let response = reqwest::get(url).await?;
        let versions: Vec<QuiltLoaderVersion> = response.json().await?;

        versions
            .first()
            .map(|v| v.version.clone())
            .context("No Quilt loader versions available")
    }

    async fn download_file(&self, url: &str, dest: &Path) -> Result<()> {
        let response = reqwest::get(url)
            .await
            .context(format!("Failed to download from {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!("HTTP error {}: {}", response.status(), url);
        }

        let bytes = response.bytes().await?;
        fs::write(dest, &bytes).await?;
        Ok(())
    }
}

impl crate::loader::LoaderInstaller for QuiltInstaller {
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.install_async(mc_version, target_dir))
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn loader_type(&self) -> &str {
        "quilt"
    }
}
