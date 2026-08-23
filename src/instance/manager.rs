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

    /// Construct a manager rooted at a custom instances directory. Used by
    /// tests to avoid touching the real user data directory.
    pub fn with_dir(instances_dir: PathBuf) -> Self {
        Self { instances_dir }
    }

    pub async fn create(&self, config: InstanceConfig, install: bool) -> Result<Instance> {
        crate::util::validate::validate_name(&config.name)?;
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
        
        // Create progress bar for client jar download
        let pb = crate::util::progress::create_download_bar(0);
        pb.set_message(format!("Downloading {}.jar", version_info.id));
        
        crate::version::downloader::download_file_with_progress(
            client_url,
            &client_jar_path,
            Some(&version_metadata.downloads.client.sha1),
            |downloaded, total| {
                if total > 0 && pb.length() != Some(total) {
                    pb.set_length(total);
                }
                pb.set_position(downloaded);
            },
        ).await?;
        
        pb.finish_with_message(format!("✓ Downloaded {}.jar", version_info.id));

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

                    // Also install Mod Menu (modmenu) so users get a
                    // convenient in-game mod list UI. Best-effort like
                    // the Fabric API install above.
                    if let Err(e) = crate::loader::fabric::FabricInstaller::install_mod_menu(
                        &version_info.id,
                        &mods_dir,
                    )
                    .await
                    {
                        tracing::warn!("Could not install Mod Menu (install it manually): {}", e);
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

            let config: InstanceConfig =
                crate::util::jsonio::parse_async(&config_path, "instance config").await?;

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
        let config: InstanceConfig =
            crate::util::jsonio::parse_async(&config_path, "instance config").await?;

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

    /// Clone an instance into a new instance directory. The entire directory
    /// tree is copied (mods, configs, saves, worlds) and the new instance's
    /// `instance.json` is rewritten with the new name. v26.1-alpha.4: this
    /// matches the "duplicate instance" feature every mainstream launcher
    /// offers.
    pub async fn clone_instance(&self, src_name: &str, dst_name: &str) -> Result<Instance> {
        crate::util::validate::validate_name(dst_name)?;
        let src_path = self.instances_dir.join(src_name);
        let dst_path = self.instances_dir.join(dst_name);

        if !src_path.exists() {
            anyhow::bail!("Instance '{}' not found", src_name);
        }
        if dst_path.exists() {
            anyhow::bail!("Instance '{}' already exists", dst_name);
        }

        copy_dir_recursive(&src_path, &dst_path).await
            .with_context(|| format!("Failed to clone '{}' to '{}'", src_name, dst_name))?;

        // Rewrite the cloned config with the new name.
        let mut config = self.get(dst_name).await?.config;
        config.name = dst_name.to_string();
        let config_json = serde_json::to_string_pretty(&config)?;
        fs::write(dst_path.join("instance.json"), config_json).await
            .context("Failed to rewrite cloned instance configuration")?;

        Ok(Instance {
            name: dst_name.to_string(),
            config,
            path: dst_path,
        })
    }

    /// Rename an instance: move its directory and rewrite `instance.json`.
    /// v26.1-alpha.4: matches mainstream launcher rename support.
    pub async fn rename(&self, old_name: &str, new_name: &str) -> Result<Instance> {
        crate::util::validate::validate_name(new_name)?;
        let old_path = self.instances_dir.join(old_name);
        let new_path = self.instances_dir.join(new_name);

        if !old_path.exists() {
            anyhow::bail!("Instance '{}' not found", old_name);
        }
        if new_path.exists() {
            anyhow::bail!("Instance '{}' already exists", new_name);
        }

        fs::rename(&old_path, &new_path).await
            .with_context(|| format!("Failed to rename '{}' to '{}'", old_name, new_name))?;

        let mut config = self.get(new_name).await?.config;
        config.name = new_name.to_string();
        let config_json = serde_json::to_string_pretty(&config)?;
        fs::write(new_path.join("instance.json"), config_json).await
            .context("Failed to rewrite renamed instance configuration")?;

        Ok(Instance {
            name: new_name.to_string(),
            config,
            path: new_path,
        })
    }
}

/// Recursively copy a directory tree (files + subdirectories). Uses tokio fs
/// so it stays async-friendly. Skips symlinks (rare in instance dirs).
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    fs::create_dir_all(dst).await?;
    let mut entries = fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_child = entry.path();
        let dst_child = dst.join(entry.file_name());
        let ft = entry.file_type().await?;
        if ft.is_dir() {
            Box::pin(copy_dir_recursive(&src_child, &dst_child)).await?;
        } else if ft.is_file() {
            fs::copy(&src_child, &dst_child).await.with_context(|| {
                format!("Failed to copy {}", src_child.display())
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::config::InstanceConfig;
    use tempfile::tempdir;

    fn test_config(name: &str) -> InstanceConfig {
        InstanceConfig {
            name: name.to_string(),
            version: "1.21.4".to_string(),
            loader: None,
            javaagents: Vec::new(),
        }
    }

    /// Create a bare instance directory (config only, no download) for tests.
    async fn make_instance(manager: &InstanceManager, name: &str) {
        let path = manager.instances_dir.join(name);
        tokio::fs::create_dir_all(&path).await.unwrap();
        let config = test_config(name);
        let json = serde_json::to_string_pretty(&config).unwrap();
        tokio::fs::write(path.join("instance.json"), json).await.unwrap();
    }

    #[tokio::test]
    async fn test_clone_copies_tree_and_renames_config() {
        let dir = tempdir().unwrap();
        let manager = InstanceManager::with_dir(dir.path().to_path_buf());
        make_instance(&manager, "src").await;
        // Add a nested file to prove recursive copy.
        let nested = manager.instances_dir.join("src").join("mods");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        tokio::fs::write(nested.join("example.jar"), b"fake").await.unwrap();

        let cloned = manager.clone_instance("src", "dst").await.unwrap();
        assert_eq!(cloned.name, "dst");
        assert!(cloned.path.join("mods").join("example.jar").exists());
        // Config rewritten with new name.
        let cfg: InstanceConfig = serde_json::from_str(
            &tokio::fs::read_to_string(cloned.path.join("instance.json")).await.unwrap(),
        ).unwrap();
        assert_eq!(cfg.name, "dst");
    }

    #[tokio::test]
    async fn test_clone_rejects_existing_destination() {
        let dir = tempdir().unwrap();
        let manager = InstanceManager::with_dir(dir.path().to_path_buf());
        make_instance(&manager, "src").await;
        make_instance(&manager, "dst").await;
        assert!(manager.clone_instance("src", "dst").await.is_err());
    }

    #[tokio::test]
    async fn test_rename_moves_and_rewrites_config() {
        let dir = tempdir().unwrap();
        let manager = InstanceManager::with_dir(dir.path().to_path_buf());
        make_instance(&manager, "old").await;
        let renamed = manager.rename("old", "new").await.unwrap();
        assert_eq!(renamed.name, "new");
        assert!(!manager.instances_dir.join("old").exists());
        let cfg: InstanceConfig = serde_json::from_str(
            &tokio::fs::read_to_string(renamed.path.join("instance.json")).await.unwrap(),
        ).unwrap();
        assert_eq!(cfg.name, "new");
    }

    #[tokio::test]
    async fn test_rename_rejects_existing_destination() {
        let dir = tempdir().unwrap();
        let manager = InstanceManager::with_dir(dir.path().to_path_buf());
        make_instance(&manager, "old").await;
        make_instance(&manager, "taken").await;
        assert!(manager.rename("old", "taken").await.is_err());
    }
}
