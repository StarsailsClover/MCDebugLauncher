// Mod management for Minecraft instances
// Handles installation, removal, and listing of mods

use anyhow::{Result, Context, bail};
use serde::{Serialize, Deserialize};
use std::path::Path;
use tokio::fs;

use super::InstanceManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub enabled: bool,
    /// File kind: "jar" (Fabric/Forge/NeoForge/Quilt mod), "aje" (Aprism
    /// native mod, v26.2-alpha.8), or "other" (unknown file in mods/).
    pub kind: String,
}

/// Classify a mods/ entry by extension.
fn classify_mod(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".jar") {
        "jar"
    } else if lower.ends_with(".aje") {
        "aje"
    } else {
        "other"
    }
}

pub struct ModManager {
    manager: InstanceManager,
}

impl ModManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: InstanceManager::new()?,
        })
    }

    /// List all mods in an instance
    pub async fn list_mods(&self, instance_name: &str) -> Result<Vec<ModInfo>> {
        let instance = self.manager.get(instance_name).await?;
        let mods_dir = instance.path.join("mods");

        if !mods_dir.exists() {
            return Ok(Vec::new());
        }

        let mut mods = Vec::new();
        let mut entries = fs::read_dir(&mods_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() {
                let filename = entry.file_name().to_string_lossy().to_string();

                // Check if mod is disabled (*.disabled extension)
                let enabled = !filename.ends_with(".disabled");
                let display_name = if enabled {
                    filename
                } else {
                    filename.trim_end_matches(".disabled").to_string()
                };

                let metadata = entry.metadata().await?;
                let kind = classify_mod(&display_name).to_string();

                mods.push(ModInfo {
                    filename: display_name,
                    size_bytes: metadata.len(),
                    enabled,
                    kind,
                });
            }
        }

        mods.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(mods)
    }

    /// Install a mod from a local file
    pub async fn install_mod(&self, instance_name: &str, mod_path: &Path) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let mods_dir = instance.path.join("mods");

        // Create mods directory if it doesn't exist
        fs::create_dir_all(&mods_dir).await?;

        // Verify the file is a JAR
        let filename = mod_path.file_name()
            .context("Invalid mod file path")?
            .to_string_lossy();

        // v26.2-alpha.8: accept Aprism native mods (.aje) alongside JARs.
        let filename_lower = filename.to_ascii_lowercase();
        if !filename_lower.ends_with(".jar") && !filename_lower.ends_with(".aje") {
            bail!("Mod file must be a .jar (loader mod) or .aje (Aprism native mod) file");
        }

        // Copy mod to instance
        let dest_path = mods_dir.join(&*filename);
        fs::copy(mod_path, &dest_path).await
            .context("Failed to copy mod file")?;

        tracing::info!("Installed mod: {}", filename);
        Ok(())
    }

    /// Remove a mod from an instance
    pub async fn remove_mod(&self, instance_name: &str, mod_filename: &str) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let mods_dir = instance.path.join("mods");

        // Try both enabled and disabled versions
        let enabled_path = mods_dir.join(mod_filename);
        let disabled_path = mods_dir.join(format!("{}.disabled", mod_filename));

        if enabled_path.exists() {
            fs::remove_file(&enabled_path).await?;
            tracing::info!("Removed mod: {}", mod_filename);
        } else if disabled_path.exists() {
            fs::remove_file(&disabled_path).await?;
            tracing::info!("Removed disabled mod: {}", mod_filename);
        } else {
            bail!("Mod not found: {}", mod_filename);
        }

        Ok(())
    }

    /// Enable a mod by removing .disabled extension
    pub async fn enable_mod(&self, instance_name: &str, mod_filename: &str) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let mods_dir = instance.path.join("mods");

        let disabled_path = mods_dir.join(format!("{}.disabled", mod_filename));
        let enabled_path = mods_dir.join(mod_filename);

        if !disabled_path.exists() {
            bail!("Mod is not disabled or does not exist: {}", mod_filename);
        }

        fs::rename(&disabled_path, &enabled_path).await?;
        tracing::info!("Enabled mod: {}", mod_filename);
        Ok(())
    }

    /// Disable a mod by adding .disabled extension
    pub async fn disable_mod(&self, instance_name: &str, mod_filename: &str) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let mods_dir = instance.path.join("mods");

        let enabled_path = mods_dir.join(mod_filename);
        let disabled_path = mods_dir.join(format!("{}.disabled", mod_filename));

        if !enabled_path.exists() {
            bail!("Mod not found or already disabled: {}", mod_filename);
        }

        fs::rename(&enabled_path, &disabled_path).await?;
        tracing::info!("Disabled mod: {}", mod_filename);
        Ok(())
    }
}
