// World backup and restore for Minecraft instances
// Handles creation and restoration of world backups

use anyhow::{Result, Context, bail};
use chrono::Utc;
use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use super::InstanceManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub name: String,
    pub instance: String,
    pub world: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub path: PathBuf,
}

pub struct BackupManager {
    manager: InstanceManager,
}

impl BackupManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: InstanceManager::new()?,
        })
    }

    /// Create a backup of a world
    pub async fn create_backup(&self, instance_name: &str, world_name: &str, backup_name: Option<String>) -> Result<BackupInfo> {
        let instance = self.manager.get(instance_name).await?;
        let world_path = instance.path.join("saves").join(world_name);

        if !world_path.exists() {
            bail!("World '{}' does not exist in instance '{}'", world_name, instance_name);
        }

        // Generate backup name with timestamp
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_name = backup_name.unwrap_or_else(|| format!("{}_{}", world_name, timestamp));

        // Create backups directory
        let backups_dir = instance.path.join("backups");
        fs::create_dir_all(&backups_dir).await?;

        let backup_path = backups_dir.join(format!("{}.zip", backup_name));

        // Create zip archive
        let world_path_clone = world_path.clone();
        let backup_path_clone = backup_path.clone();

        tokio::task::spawn_blocking(move || {
            Self::create_zip_archive(&world_path_clone, &backup_path_clone)
        }).await??;

        // Get backup size
        let metadata = fs::metadata(&backup_path).await?;
        let size_bytes = metadata.len();

        let info = BackupInfo {
            name: backup_name,
            instance: instance_name.to_string(),
            world: world_name.to_string(),
            created_at: Utc::now().to_rfc3339(),
            size_bytes,
            path: backup_path,
        };

        tracing::info!("Created backup: {} ({} bytes)", info.name, info.size_bytes);
        Ok(info)
    }

    /// List all backups for an instance
    pub async fn list_backups(&self, instance_name: &str) -> Result<Vec<BackupInfo>> {
        let instance = self.manager.get(instance_name).await?;
        let backups_dir = instance.path.join("backups");

        if !backups_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        let mut entries = fs::read_dir(&backups_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                let metadata = fs::metadata(&path).await?;
                let size_bytes = metadata.len();

                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Extract world name from backup name (before timestamp)
                let world = name.split('_')
                    .next()
                    .unwrap_or(&name)
                    .to_string();

                let created_at = metadata.created()
                    .ok()
                    .and_then(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339().into())
                    .unwrap_or_else(|| "unknown".to_string());

                backups.push(BackupInfo {
                    name,
                    instance: instance_name.to_string(),
                    world,
                    created_at,
                    size_bytes,
                    path,
                });
            }
        }

        backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(backups)
    }

    /// Restore a backup
    pub async fn restore_backup(&self, instance_name: &str, backup_name: &str, target_world: Option<String>) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let backups_dir = instance.path.join("backups");
        let backup_path = backups_dir.join(format!("{}.zip", backup_name));

        if !backup_path.exists() {
            bail!("Backup '{}' does not exist", backup_name);
        }

        // Determine target world name
        let world_name = target_world.unwrap_or_else(|| {
            backup_name.split('_')
                .next()
                .unwrap_or(backup_name)
                .to_string()
        });

        let world_path = instance.path.join("saves").join(&world_name);

        // If world exists, create automatic backup before restoring
        if world_path.exists() {
            tracing::info!("World '{}' exists, creating automatic backup before restore", world_name);
            let auto_backup_name = format!("{}_pre_restore_{}", world_name, Utc::now().format("%Y%m%d_%H%M%S"));
            self.create_backup(instance_name, &world_name, Some(auto_backup_name)).await?;

            // Remove existing world
            fs::remove_dir_all(&world_path).await?;
        }

        // Extract backup
        let backup_path_clone = backup_path.clone();
        let world_path_clone = world_path.clone();

        tokio::task::spawn_blocking(move || {
            Self::extract_zip_archive(&backup_path_clone, &world_path_clone)
        }).await??;

        tracing::info!("Restored backup '{}' to world '{}'", backup_name, world_name);
        Ok(())
    }

    /// Delete a backup
    pub async fn delete_backup(&self, instance_name: &str, backup_name: &str) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let backups_dir = instance.path.join("backups");
        let backup_path = backups_dir.join(format!("{}.zip", backup_name));

        if !backup_path.exists() {
            bail!("Backup '{}' does not exist", backup_name);
        }

        fs::remove_file(&backup_path).await?;
        tracing::info!("Deleted backup: {}", backup_name);
        Ok(())
    }

    // Helper function to create zip archive (blocking I/O)
    fn create_zip_archive(source: &Path, dest: &Path) -> Result<()> {
        use std::fs::File;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let file = File::create(dest)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        Self::zip_directory(&mut zip, source, source, options)?;
        zip.finish()?;
        Ok(())
    }

    fn zip_directory<W: std::io::Write + std::io::Seek>(
        zip: &mut zip::ZipWriter<W>,
        source: &Path,
        base: &Path,
        options: zip::write::SimpleFileOptions,
    ) -> Result<()> {
        use std::fs;
        use std::io::{Read, Write};

        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.strip_prefix(base)
                .context("Failed to strip prefix")?;

            if path.is_file() {
                zip.start_file(name.to_string_lossy().to_string(), options)?;
                let mut file = fs::File::open(&path)?;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)?;
                zip.write_all(&buffer)?;
            } else if path.is_dir() {
                zip.add_directory(name.to_string_lossy().to_string(), options)?;
                Self::zip_directory(zip, &path, base, options)?;
            }
        }

        Ok(())
    }

    // Helper function to extract zip archive (blocking I/O)
    fn extract_zip_archive(source: &Path, dest: &Path) -> Result<()> {
        use std::fs::File;
        use zip::ZipArchive;

        let file = File::open(source)?;
        let mut archive = ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = dest.join(file.name());

            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    std::fs::create_dir_all(p)?;
                }
                let mut outfile = File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Ok(())
    }
}
