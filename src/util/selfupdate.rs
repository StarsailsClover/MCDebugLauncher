// Self-update functionality for MDL
// Checks GitHub for new releases and automatically updates the binary

use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

/// Check if a newer version is available on GitHub
pub async fn check_for_update() -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .user_agent("MCDebugLauncher")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client
        .get("https://api.github.com/repos/StarsailsClover/MCDebugLauncher/releases/latest")
        .send()
        .await?;

    let release: serde_json::Value = if response.status().is_success() {
        response.json().await?
    } else if response.status().as_u16() == 404 {
        // No stable "latest" release exists — fall back to the newest release
        // (including Pre-Releases), matching the project's release policy.
        let list = client
            .get("https://api.github.com/repos/StarsailsClover/MCDebugLauncher/releases?per_page=10")
            .send()
            .await?;
        if !list.status().is_success() {
            anyhow::bail!("Failed to fetch latest release: {}", list.status());
        }
        let releases: Vec<serde_json::Value> = list.json().await?;
        releases
            .into_iter()
            .next()
            .context("No releases found")?
    } else {
        anyhow::bail!("Failed to fetch latest release: {}", response.status());
    };

    let latest_version = release["tag_name"]
        .as_str()
        .context("Missing tag_name in release")?
        .trim_start_matches('v');

    let current_version = env!("CARGO_PKG_VERSION");

    if version_compare(latest_version, current_version)? {
        Ok(Some(latest_version.to_string()))
    } else {
        Ok(None)
    }
}

/// Compare two semantic versions. Returns true if `new` is greater than `current`.
fn version_compare(new: &str, current: &str) -> Result<bool> {
    let parse = |v: &str| -> Result<Vec<u32>> {
        v.split('-')
            .next()
            .context("Invalid version format")?
            .split('.')
            .map(|s| s.parse::<u32>().context("Failed to parse version number"))
            .collect()
    };

    let new_parts = parse(new)?;
    let current_parts = parse(current)?;

    for (n, c) in new_parts.iter().zip(current_parts.iter()) {
        if n > c {
            return Ok(true);
        } else if n < c {
            return Ok(false);
        }
    }

    Ok(new_parts.len() > current_parts.len())
}

/// Download and install the latest version
pub async fn perform_update(new_version: &str) -> Result<()> {
    tracing::info!("Downloading MDL v{}...", new_version);

    let download_url = format!(
        "https://github.com/StarsailsClover/MCDebugLauncher/releases/download/v{}/mdl.exe",
        new_version
    );

    let client = reqwest::Client::builder()
        .user_agent("MCDebugLauncher")
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let response = client.get(&download_url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download update: {}", response.status());
    }

    let bytes = response.bytes().await?;

    // Get current executable path
    let current_exe = std::env::current_exe()
        .context("Failed to get current executable path")?;

    // Create backup
    let backup_path = current_exe.with_extension("exe.bak");
    if current_exe.exists() {
        fs::copy(&current_exe, &backup_path).await
            .context("Failed to create backup")?;
    }

    // Write new executable to temporary location
    let temp_path = current_exe.with_extension("exe.new");
    fs::write(&temp_path, &bytes).await
        .context("Failed to write new executable")?;

    tracing::info!("Update downloaded successfully");
    tracing::info!("Restart MDL to complete the update");
    tracing::info!("Old version backed up to: {}", backup_path.display());

    // On Windows, we can't replace the running executable directly
    // Instead, we create a batch script that will replace it after exit
    #[cfg(target_os = "windows")]
    {
        let script_path = current_exe.with_extension("bat");
        let script_content = format!(
            r#"@echo off
echo Updating MDL...
timeout /t 2 /nobreak >nul
move /y "{}" "{}"
del "%~f0"
"#,
            temp_path.display(),
            current_exe.display()
        );
        fs::write(&script_path, script_content).await?;

        tracing::info!("Update script created at: {}", script_path.display());
        tracing::info!("Run the following to complete the update:");
        tracing::info!("  {}", script_path.display());
    }

    Ok(())
}

/// Add MDL to system PATH environment variable
#[cfg(target_os = "windows")]
pub fn add_to_path() -> Result<()> {
    use std::process::Command;

    let exe_path = std::env::current_exe()
        .context("Failed to get current executable path")?;
    let exe_dir = exe_path.parent()
        .context("Failed to get executable directory")?;

    // Check if already in PATH
    if is_in_path(exe_dir)? {
        tracing::info!("MDL is already in PATH");
        return Ok(());
    }

    tracing::info!("Adding MDL to system PATH...");

    // Use PowerShell to modify user PATH
    let ps_script = format!(
        r#"$path = [Environment]::GetEnvironmentVariable('Path', 'User'); if ($path -notlike '*{}*') {{ [Environment]::SetEnvironmentVariable('Path', $path + ';{}', 'User'); Write-Output 'Added' }} else {{ Write-Output 'Exists' }}"#,
        exe_dir.display(),
        exe_dir.display()
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
        .context("Failed to execute PowerShell command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to add to PATH: {}", stderr);
    }

    let result = String::from_utf8_lossy(&output.stdout);
    if result.trim() == "Added" {
        tracing::info!("Successfully added MDL to PATH");
        tracing::info!("Please restart your terminal for changes to take effect");
    } else {
        tracing::info!("MDL directory already in PATH");
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn add_to_path() -> Result<()> {
    tracing::warn!("Automatic PATH registration is only supported on Windows");
    tracing::info!("Please manually add MDL to your PATH");
    Ok(())
}

/// Check if a directory is in the system PATH
fn is_in_path(dir: &Path) -> Result<bool> {
    let path_var = std::env::var("PATH").context("Failed to read PATH variable")?;
    let dir_str = dir.to_string_lossy();

    Ok(path_var.split(';')
        .any(|p| p.trim().eq_ignore_ascii_case(&dir_str)))
}
