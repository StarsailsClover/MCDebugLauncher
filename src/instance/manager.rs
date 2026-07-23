// Instance manager

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use super::config::InstanceConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct Instance {
    pub name: String,
    pub config: InstanceConfig,
    pub path: PathBuf,
}

pub struct InstanceManager {
    instances_dir: PathBuf,
}

impl InstanceManager {
    pub fn new() -> Result<Self> {
        let instances_dir = crate::util::paths::get_instances_dir()?;
        Ok(Self { instances_dir })
    }

    pub async fn create(&self, config: InstanceConfig, install: bool) -> Result<Instance> {
        let instance_path = self.instances_dir.join(&config.name);

        if instance_path.exists() {
            anyhow::bail!("Instance '{}' already exists", config.name);
        }

        fs::create_dir_all(&instance_path).await
            .with_context(|| format!("Failed to create instance directory: {}", instance_path.display()))?;

        let config_path = instance_path.join("instance.json");
        let config_json = serde_json::to_string_pretty(&config)?;
        fs::write(&config_path, config_json).await
            .context("Failed to write instance configuration")?;

        if install {
            self.install_version(&instance_path, &config).await?;
        }

        Ok(Instance {
            name: config.name.clone(),
            config,
            path: instance_path,
        })
    }

    async fn install_version(&self, instance_path: &Path, config: &InstanceConfig) -> Result<()> {
        let versions_cache = crate::util::paths::get_versions_cache_dir()?;
        fs::create_dir_all(&versions_cache).await?;

        tracing::info!("Fetching version manifest...");
        let manifest = crate::version::manifest::VersionManifest::fetch().await?;

        let version_info = manifest.find_version(&config.version)
            .with_context(|| format!("Minecraft version '{}' not found", config.version))?;

        tracing::info!("Downloading Minecraft {}...", version_info.id);
        let version_metadata = crate::version::manifest::VersionMetadata::fetch(&version_info.url).await?;

        let version_dir = instance_path.join("versions").join(&version_info.id);
        fs::create_dir_all(&version_dir).await?;

        let client_url = version_metadata.downloads.client.url.as_str();
        let client_jar_path = version_dir.join(format!("{}.jar", version_info.id));

        tracing::info!("Downloading client jar...");
        crate::version::downloader::download_file(
            client_url,
            &client_jar_path,
            Some(&version_metadata.downloads.client.sha1),
        ).await?;

        let version_json_path = version_dir.join(format!("{}.json", version_info.id));
        let version_json = serde_json::to_string_pretty(&version_metadata)?;
        fs::write(&version_json_path, version_json).await?;

        if let Some(loader_config) = &config.loader {
            tracing::info!("Installing {} loader...", loader_config.loader_type);

            match loader_config.loader_type.as_str() {
                "fabric" => {
                    let installer = crate::loader::fabric::FabricInstaller::new(
                        Some(loader_config.version.clone())
                    );

                    let loader_version = if loader_config.version == "latest" {
                        let versions = crate::loader::fabric::FabricInstaller::fetch_versions().await?;
                        versions
                            .iter()
                            .find(|v| v.stable)
                            .map(|v| v.version.clone())
                            .context("No stable Fabric loader version found")?
                    } else {
                        loader_config.version.clone()
                    };

                    installer.install_loader(&version_info.id, &loader_version, &version_dir).await?;

                    // Install Fabric API into the instance's mods directory.
                    // This is best-effort: a failure warns but does not abort
                    // the instance creation.
                    let mods_dir = instance_path.join("mods");
                    if let Err(e) = crate::loader::fabric::FabricInstaller::install_fabric_api(
                        &version_info.id,
                        &mods_dir,
                    )
                    .await
                    {
                        tracing::warn!("Could not install Fabric API (install it manually): {}", e);
                    }
                }
                "forge" => {
                    let installer = crate::loader::forge::ForgeInstaller::new(
                        Some(loader_config.version.clone())
                    );

                    let forge_version = if loader_config.version == "latest" {
                        let versions = crate::loader::forge::ForgeInstaller::fetch_versions(&version_info.id).await?;
                        versions
                            .first()
                            .cloned()
                            .with_context(|| format!("No Forge version found for Minecraft {}", version_info.id))?
                            .strip_prefix(&format!("{}-", version_info.id))
                            .unwrap_or(&versions[0])
                            .to_string()
                    } else {
                        loader_config.version.clone()
                    };

                    installer.install_loader(&version_info.id, &forge_version, &version_dir).await?;
                }
                "neoforge" => {
                    let installer = crate::loader::neoforge::NeoForgeInstaller::new(
                        Some(loader_config.version.clone())
                    );

                    let neoforge_version = if loader_config.version == "latest" {
                        let versions = crate::loader::neoforge::NeoForgeInstaller::fetch_versions(&version_info.id).await?;
                        versions
                            .first()
                            .cloned()
                            .with_context(|| format!("No NeoForge version found for Minecraft {}", version_info.id))?
                    } else {
                        loader_config.version.clone()
                    };

                    installer.install_loader(&version_info.id, &neoforge_version, &version_dir).await?;
                }
                "quilt" => {
                    let installer = crate::loader::quilt::QuiltInstaller::new(
                        Some(loader_config.version.clone())
                    );

                    let quilt_version = if loader_config.version == "latest" {
                        let versions = crate::loader::quilt::QuiltInstaller::fetch_versions().await?;
                        versions
                            .first()
                            .cloned()
                            .context("No Quilt loader versions available")?
                    } else {
                        loader_config.version.clone()
                    };

                    installer.install_loader(&version_info.id, &quilt_version, &version_dir).await?;
                }
                "optifine" => {
                    let installer = crate::loader::optifine::OptiFineInstaller::new(
                        Some(loader_config.version.clone())
                    );

                    let optifine_version = if loader_config.version == "latest" {
                        "latest".to_string()
                    } else {
                        loader_config.version.clone()
                    };

                    installer.install_loader(&version_info.id, &optifine_version, &version_dir).await?;
                }
                _ => anyhow::bail!("Unsupported loader type: {}", loader_config.loader_type),
            }
        }

        tracing::info!("Instance '{}' created successfully", config.name);

        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<Instance>> {
        if !self.instances_dir.exists() {
            return Ok(vec![]);
        }

        let mut instances = Vec::new();
        let mut entries = fs::read_dir(&self.instances_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join("instance.json");
            if !config_path.exists() {
                continue;
            }

            let config_json = fs::read_to_string(&config_path).await?;
            let config: InstanceConfig = serde_json::from_str(&config_json)?;

            instances.push(Instance {
                name: config.name.clone(),
                config,
                path,
            });
        }

        Ok(instances)
    }

    pub async fn get(&self, name: &str) -> Result<Instance> {
        let instance_path = self.instances_dir.join(name);

        if !instance_path.exists() {
            anyhow::bail!("Instance '{}' not found", name);
        }

        let config_path = instance_path.join("instance.json");
        let config_json = fs::read_to_string(&config_path).await?;
        let config: InstanceConfig = serde_json::from_str(&config_json)?;

        Ok(Instance {
            name: config.name.clone(),
            config,
            path: instance_path,
        })
    }

    pub async fn delete(&self, name: &str) -> Result<()> {
        let instance_path = self.instances_dir.join(name);

        if !instance_path.exists() {
            anyhow::bail!("Instance '{}' not found", name);
        }

        fs::remove_dir_all(&instance_path).await
            .with_context(|| format!("Failed to delete instance '{}'", name))?;

        Ok(())
    }
}
