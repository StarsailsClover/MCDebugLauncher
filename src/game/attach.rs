// Hot-attach JavaAgent injection (v26.2-alpha.6).
//
// Loads a Java agent JAR into a RUNNING Minecraft JVM via the JVM Attach
// API (agentmain), complementing the launch-time `-javaagent` (premain)
// injection added in v26.2-alpha.5.
//
// Mechanism: MDL embeds a tiny precompiled helper class
// (AttachHelper.class, compiled with `javac -source 11 -target 11` from the
// adjacent AttachHelper.java). At runtime the class is extracted into the
// MDL data directory and executed with the instance's own Java runtime:
//
//   <java> -cp <dir> AttachHelper <pid> <agentJar> [params]
//
// The helper calls VirtualMachine.attach(pid).loadAgent(jar, params).
// This requires the target runtime to expose the `jdk.attach` module —
// present in all standard Temurin/Adoptium images (the same image family
// MDL auto-provisions), absent only in heavily stripped custom runtimes.

use anyhow::{Context, Result};
use std::path::Path;

/// Precompiled helper class bytes (see attach/AttachHelper.java for source).
const HELPER_CLASS: &[u8] = include_bytes!("attach/AttachHelper.class");

/// Extract the embedded helper class into `<data>/runtime-attach/` and
/// return the classpath directory. Idempotent: rewrites the file on every
/// call so upgrades of mdl always ship their current helper.
fn ensure_helper(java_dir: &Path) -> Result<std::path::PathBuf> {
    let dir = java_dir.join("attach-helper");
    std::fs::create_dir_all(&dir)?;
    let class_path = dir.join("AttachHelper.class");
    std::fs::write(&class_path, HELPER_CLASS)
        .with_context(|| format!("Failed to write {}", class_path.display()))?;
    Ok(dir)
}

/// Load `agent_jar` into the JVM process `pid` using java executable at
/// `java_path`. `params` is passed verbatim as the loadAgent options string
/// (JVM `-javaagent:<jar>=<options>` syntax after the `=`).
pub async fn inject_agent(
    java_path: &Path,
    pid: u32,
    agent_jar: &Path,
    params: Option<&str>,
) -> Result<()> {
    if !agent_jar.exists() {
        anyhow::bail!("Agent JAR not found: {}", agent_jar.display());
    }

    // Resolve the MDL data dir for helper extraction. Falls back to the
    // system temp dir if the data dir is unavailable (e.g. exotic setups).
    let base_dir = crate::util::paths::get_data_dir()
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|_| std::env::temp_dir());
    let cp_dir = ensure_helper(&base_dir)?;

    let mut cmd = tokio::process::Command::new(java_path);
    cmd.arg("-cp")
        .arg(&cp_dir)
        .arg("AttachHelper")
        .arg(pid.to_string())
        .arg(agent_jar.as_os_str());
    if let Some(p) = params {
        cmd.arg(p);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    tracing::info!(
        "Hot-attaching agent {} to PID {}",
        agent_jar.display(),
        pid
    );

    let output = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
        .await
        .context("Attach timed out after 30s")?
        .with_context(|| format!("Failed to spawn {}", java_path.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() && stdout.trim() == "OK" {
        tracing::info!("Agent attached successfully");
        return Ok(());
    }

    let mut detail = String::new();
    if !stdout.trim().is_empty() {
        detail.push_str(&stdout);
    }
    if !stderr.trim().is_empty() {
        if !detail.is_empty() {
            detail.push_str("\n");
        }
        detail.push_str(&stderr);
    }
    // Common actionable failure: jdk.attach module missing from a stripped
    // custom runtime. Standard Temurin images always have it.
    if stderr.contains("ModuleNotFoundException") || stderr.contains("com.sun.tools.attach") {
        anyhow::bail!(
            "Agent attach failed - the target JVM does not expose the jdk.attach module \
             (custom/stripped runtime?). Detail:\n{}",
            detail
        );
    }
    if stderr.contains("NoSuchProcessException") {
        anyhow::bail!("Agent attach failed - process {} is not running", pid);
    }
    anyhow::bail!("Agent attach failed:\n{}", detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helper_class_embedded() {
        // Magic bytes of a valid class file: CAFEBABE.
        assert!(HELPER_CLASS.len() > 100);
        assert_eq!(&HELPER_CLASS[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn test_ensure_helper_writes_class() {
        let dir = std::env::temp_dir().join(format!("mdl_attach_test_{}", std::process::id()));
        let cp = ensure_helper(&dir).unwrap();
        assert!(cp.join("AttachHelper.class").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
