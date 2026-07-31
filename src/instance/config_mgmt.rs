// Configuration management for Minecraft instances
// Handles reading and modifying instance configuration files

use anyhow::{Result, Context, bail};
use serde_json::Value;
use std::path::Path;
use tokio::fs;

use super::InstanceManager;

pub struct ConfigManager {
    manager: InstanceManager,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: InstanceManager::new()?,
        })
    }

    /// Get a configuration value from options.txt
    pub async fn get_option(&self, instance_name: &str, key: &str) -> Result<Option<String>> {
        let instance = self.manager.get(instance_name).await?;
        let options_path = instance.path.join("options.txt");

        if !options_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&options_path).await?;

        for line in content.lines() {
            if let Some((k, v)) = line.split_once(':') {
                if k == key {
                    return Ok(Some(v.to_string()));
                }
            }
        }

        Ok(None)
    }

    /// Set a configuration value in options.txt
    pub async fn set_option(&self, instance_name: &str, key: &str, value: &str) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let options_path = instance.path.join("options.txt");

        let mut lines = if options_path.exists() {
            let content = fs::read_to_string(&options_path).await?;
            content.lines().map(String::from).collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Find and update existing key, or append new one
        let mut found = false;
        for line in &mut lines {
            if let Some((k, _)) = line.split_once(':') {
                if k == key {
                    *line = format!("{}:{}", key, value);
                    found = true;
                    break;
                }
            }
        }

        if !found {
            lines.push(format!("{}:{}", key, value));
        }

        // Write back
        let content = lines.join("\n") + "\n";
        fs::write(&options_path, content).await?;

        tracing::info!("Set option {}={} in instance {}", key, value, instance_name);
        Ok(())
    }

    /// Get server configuration
    pub async fn get_server_config(&self, instance_name: &str) -> Result<Option<Value>> {
        let instance = self.manager.get(instance_name).await?;
        let servers_path = instance.path.join("servers.dat");

        if !servers_path.exists() {
            return Ok(None);
        }

        // servers.dat is NBT format, which is complex to parse
        // For now, return None and document this limitation
        bail!("Server configuration reading from NBT format is not yet implemented");
    }

    /// Export instance configuration as JSON
    pub async fn export_config(&self, instance_name: &str) -> Result<Value> {
        let instance = self.manager.get(instance_name).await?;
        let options_path = instance.path.join("options.txt");

        let mut options = serde_json::Map::new();

        if options_path.exists() {
            let content = fs::read_to_string(&options_path).await?;
            for line in content.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    options.insert(k.to_string(), Value::String(v.to_string()));
                }
            }
        }

        Ok(Value::Object(options))
    }

    /// Import instance configuration from JSON
    pub async fn import_config(&self, instance_name: &str, config: &Value) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;
        let options_path = instance.path.join("options.txt");

        let obj = config.as_object()
            .context("Configuration must be a JSON object")?;

        let mut lines = Vec::new();
        for (key, value) in obj {
            let value_str = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };
            lines.push(format!("{}:{}", key, value_str));
        }

        let content = lines.join("\n") + "\n";
        fs::write(&options_path, content).await?;

        tracing::info!("Imported configuration for instance {}", instance_name);
        Ok(())
    }

    /// Backup instance configuration files
    pub async fn backup_config(&self, instance_name: &str, backup_path: &Path) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;

        fs::create_dir_all(backup_path).await?;

        // Backup options.txt
        let options_src = instance.path.join("options.txt");
        if options_src.exists() {
            let options_dst = backup_path.join("options.txt");
            fs::copy(&options_src, &options_dst).await?;
        }

        // Backup servers.dat
        let servers_src = instance.path.join("servers.dat");
        if servers_src.exists() {
            let servers_dst = backup_path.join("servers.dat");
            fs::copy(&servers_src, &servers_dst).await?;
        }

        tracing::info!("Backed up configuration for instance {} to {:?}", instance_name, backup_path);
        Ok(())
    }

    /// Restore instance configuration from backup
    pub async fn restore_config(&self, instance_name: &str, backup_path: &Path) -> Result<()> {
        let instance = self.manager.get(instance_name).await?;

        if !backup_path.exists() {
            bail!("Backup path does not exist: {:?}", backup_path);
        }

        // Restore options.txt
        let options_src = backup_path.join("options.txt");
        if options_src.exists() {
            let options_dst = instance.path.join("options.txt");
            fs::copy(&options_src, &options_dst).await?;
        }

        // Restore servers.dat
        let servers_src = backup_path.join("servers.dat");
        if servers_src.exists() {
            let servers_dst = instance.path.join("servers.dat");
            fs::copy(&servers_src, &servers_dst).await?;
        }

        tracing::info!("Restored configuration for instance {} from {:?}", instance_name, backup_path);
        Ok(())
    }
}
