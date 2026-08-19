// JavaAgent management (v26.2-alpha.5)
//
// Provides instance-level JavaAgent lifecycle management: install, list,
// remove, enable/disable. Agents are stored in the instance's `javaagents/`
// directory and registered in `instance.json` under the `javaagents` field.
//
// At launch, all enabled agents are appended as `-javaagent:<jar>[=<params>]`
// JVM arguments, after any Aprism/Despotes native agents.

use anyhow::{Context, Result};
use std::path::Path;

use crate::instance::config::{InstanceConfig, JavaAgentEntry};

/// Subdirectory inside an instance where agent JARs are stored.
pub const AGENTS_DIR: &str = "javaagents";

/// Install a JavaAgent JAR into an instance.
///
/// Copies the JAR into `<instance>/javaagents/`, registers it in
/// `instance.json`, and returns the display name.
pub async fn install(instance_dir: &Path, jar_path: &Path, params: Option<&str>) -> Result<String> {
    let agents_dir = instance_dir.join(AGENTS_DIR);
    tokio::fs::create_dir_all(&agents_dir)
        .await
        .context("Failed to create javaagents directory")?;

    let jar_name = jar_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid JAR filename")?;

    let dest = agents_dir.join(jar_name);
    tokio::fs::copy(jar_path, &dest)
        .await
        .with_context(|| format!("Failed to copy JAR to {}", dest.display()))?;

    let display_name = jar_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("agent")
        .to_string();

    let entry = JavaAgentEntry {
        name: display_name.clone(),
        path: format!("{}/{}", AGENTS_DIR, jar_name),
        params: params.map(|s| s.to_string()),
        enabled: true,
    };

    // Update instance.json
    let config_path = instance_dir.join("instance.json");
    let config_data = tokio::fs::read_to_string(&config_path).await?;
    let mut config: InstanceConfig = serde_json::from_str(&config_data)?;

    // Check for duplicate name.
    if config.javaagents.iter().any(|a| a.name == entry.name) {
        anyhow::bail!("JavaAgent '{}' is already registered in this instance", entry.name);
    }

    config.javaagents.push(entry);
    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, json).await?;

    Ok(display_name)
}

/// List all registered JavaAgents in an instance.
pub async fn list(instance_dir: &Path) -> Result<Vec<JavaAgentEntry>> {
    let config = read_config(instance_dir).await?;
    Ok(config.javaagents)
}

/// Remove a registered JavaAgent (deletes the JAR and unregisters).
pub async fn remove(instance_dir: &Path, name: &str) -> Result<()> {
    let config_path = instance_dir.join("instance.json");
    let config_data = tokio::fs::read_to_string(&config_path).await?;
    let mut config: InstanceConfig = serde_json::from_str(&config_data)?;

    let idx = config
        .javaagents
        .iter()
        .position(|a| a.name == name)
        .context(format!("JavaAgent '{}' not found", name))?;

    let entry = config.javaagents.remove(idx);

    // Delete the JAR file (best-effort).
    let jar_path = instance_dir.join(&entry.path);
    let _ = tokio::fs::remove_file(&jar_path).await;

    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, json).await?;

    Ok(())
}

/// Enable a disabled JavaAgent.
pub async fn enable(instance_dir: &Path, name: &str) -> Result<()> {
    set_enabled(instance_dir, name, true).await
}

/// Disable a JavaAgent (keeps the file, skips at launch).
pub async fn disable(instance_dir: &Path, name: &str) -> Result<()> {
    set_enabled(instance_dir, name, false).await
}

async fn set_enabled(instance_dir: &Path, name: &str, enabled: bool) -> Result<()> {
    let config_path = instance_dir.join("instance.json");
    let config_data = tokio::fs::read_to_string(&config_path).await?;
    let mut config: InstanceConfig = serde_json::from_str(&config_data)?;

    let entry = config
        .javaagents
        .iter_mut()
        .find(|a| a.name == name)
        .context(format!("JavaAgent '{}' not found", name))?;

    entry.enabled = enabled;

    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, json).await?;

    Ok(())
}

/// Read the instance config.
async fn read_config(instance_dir: &Path) -> Result<InstanceConfig> {
    let config_path = instance_dir.join("instance.json");
    let config_data = tokio::fs::read_to_string(&config_path).await?;
    let config: InstanceConfig = serde_json::from_str(&config_data)?;
    Ok(config)
}

/// Build the `-javaagent` JVM arguments for all enabled agents in an
/// instance. Returns absolute paths so the JVM can find them.
pub async fn build_agent_args(instance_dir: &Path) -> Result<Vec<String>> {
    let config = read_config(instance_dir).await?;
    let mut args = Vec::new();
    for entry in &config.javaagents {
        if !entry.enabled {
            continue;
        }
        let jar_path = instance_dir.join(&entry.path);
        if !jar_path.exists() {
            tracing::warn!("JavaAgent '{}' JAR not found at {}, skipping", entry.name, jar_path.display());
            continue;
        }
        let arg = match &entry.params {
            Some(p) => format!("-javaagent:{}={}", jar_path.display(), p),
            None => format!("-javaagent:{}", jar_path.display()),
        };
        args.push(arg);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java_agent_entry_serializes() {
        let entry = JavaAgentEntry {
            name: "my-agent".to_string(),
            path: "javaagents/my-agent.jar".to_string(),
            params: Some("port=25585".to_string()),
            enabled: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"name\":\"my-agent\""));
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"params\":\"port=25585\""));
    }

    #[test]
    fn test_java_agent_entry_disabled_params_omitted() {
        let entry = JavaAgentEntry {
            name: "agent".to_string(),
            path: "javaagents/agent.jar".to_string(),
            params: None,
            enabled: false,
        };
        let json = serde_json::to_string(&entry).unwrap();
        // params should be skipped when None
        assert!(!json.contains("params"));
    }
}
