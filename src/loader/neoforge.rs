// NeoForge mod loader installer

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize)]
struct NeoForgeVersionManifest {
    versions: Vec<NeoForgeVersion>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeVersion {
    version: String,
}

pub struct NeoForgeInstaller {
    version: String,
}

impl NeoForgeInstaller {
    pub fn new(version: String) -> Self {
        Self { version }
    }

    pub async fn install_async(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let version_dir = Path::new(target_dir);
        fs::create_dir_all(&version_dir).await?;

        tracing::info!("Installing NeoForge {} for Minecraft {}", self.version, mc_version);

        // NeoForge uses similar structure to Forge
        let loader_version = if self.version == "latest" {
            self.fetch_latest_neoforge(mc_version).await?
        } else {
            self.version.clone()
        };

        // Download NeoForge installer/version JSON
        let version_json_url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}.json",
            loader_version, loader_version
        );

        let version_json_path = version_dir.join("version.json");

        tracing::info!("Downloading NeoForge version JSON from {}", version_json_url);
        match self.download_file(&version_json_url, &version_json_path).await {
            Ok(_) => {},
            Err(e) => {
                tracing::warn!("Failed to download from primary URL: {}", e);
                // Try alternative format
                let alt_url = format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{}-{}/neoforge-{}-{}.json",
                    loader_version, mc_version, loader_version, mc_version
                );
                tracing::info!("Trying alternative URL: {}", alt_url);
                self.download_file(&alt_url, &version_json_path).await?;
            }
        }

        tracing::info!("NeoForge {} installed successfully", loader_version);
        Ok(format!("{}-neoforge-{}", mc_version, loader_version))
    }

    async fn fetch_latest_neoforge(&self, mc_version: &str) -> Result<String> {
        let url = format!(
            "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge"
        );

        let response = reqwest::get(&url).await?;
        let manifest: NeoForgeVersionManifest = response.json().await?;

        // Find latest version for this MC version
        for version in &manifest.versions {
            if version.version.starts_with(&format!("{}-", mc_version)) {
                return Ok(version.version.clone());
            }
        }

        anyhow::bail!("No NeoForge version found for Minecraft {}", mc_version);
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

impl crate::loader::LoaderInstaller for NeoForgeInstaller {
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.install_async(mc_version, target_dir))
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn loader_type(&self) -> &str {
        "neoforge"
    }
}
