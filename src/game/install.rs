// Companion mod installation.
//
// MDL ships the companion mod JAR alongside the launcher binary. Installing
// it copies the JAR into the instance's `mods/` directory (removing any
// older companion build first). Launching with the companion present also
// injects the control-server port as a JVM property.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::COMPANION_JAR_PREFIX;

/// Directories searched (in order) for the companion JAR.
fn companion_search_dirs() -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.to_path_buf());
        }
    }
    dirs.push(std::env::current_dir()?);
    dirs.push(crate::util::paths::get_data_dir()?.join("companions"));
    Ok(dirs)
}

/// Find the first file named `<prefix>-*.jar` in `dir`.
fn find_companion_jar_in(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("jar")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with(COMPANION_JAR_PREFIX))
                    .unwrap_or(false)
        })
        .collect();
    // Prefer the newest build when several exist.
    candidates.sort();
    candidates.pop()
}

/// Locate the bundled companion JAR. Returns the path or an error
/// explaining where to put it.
pub fn find_companion_jar() -> Result<PathBuf> {
    for dir in companion_search_dirs()? {
        if let Some(jar) = find_companion_jar_in(&dir) {
            return Ok(jar);
        }
    }
    anyhow::bail!(
        "Companion mod JAR not found. Expected a file named '{}-<version>.jar' \
         next to the mdl executable, in the current directory, or in the MDL \
         data directory's companions/ folder.",
        COMPANION_JAR_PREFIX
    )
}

/// List companion JARs currently installed in the instance's mods dir.
pub fn installed_companions(instance_dir: &Path) -> Vec<PathBuf> {
    let mods_dir = instance_dir.join("mods");
    let entries = match std::fs::read_dir(&mods_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("jar")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with(COMPANION_JAR_PREFIX))
                    .unwrap_or(false)
        })
        .collect()
}

/// Whether the companion mod is installed in the instance.
pub fn is_installed(instance_dir: &Path) -> bool {
    !installed_companions(instance_dir).is_empty()
}

/// Install the companion JAR into the instance's mods directory. Replaces
/// any previously installed companion build. Returns the installed filename.
pub async fn install(instance_dir: &Path) -> Result<String> {
    let source = find_companion_jar()?;
    let mods_dir = instance_dir.join("mods");
    tokio::fs::create_dir_all(&mods_dir).await?;

    // Remove older companion builds so only one version is loaded.
    for old in installed_companions(instance_dir) {
        if old.file_name() != source.file_name() {
            let _ = tokio::fs::remove_file(&old).await;
        }
    }

    let dest = mods_dir.join(source.file_name().context("Invalid companion jar path")?);
    tokio::fs::copy(&source, &dest)
        .await
        .with_context(|| {
            format!(
                "Failed to copy companion mod into {}",
                mods_dir.display()
            )
        })?;

    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    tracing::info!("Installed companion mod: {}", name);
    Ok(name)
}

/// Remove the companion mod from the instance. Returns how many files were
/// removed.
pub async fn uninstall(instance_dir: &Path) -> Result<usize> {
    let mut removed = 0;
    for jar in installed_companions(instance_dir) {
        tokio::fs::remove_file(&jar).await?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jar(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"fake").unwrap();
        path
    }

    #[test]
    fn test_find_companion_jar_in() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_companion_jar_in(dir.path()).is_none());

        fake_jar(dir.path(), "other-mod.jar");
        assert!(find_companion_jar_in(dir.path()).is_none());

        fake_jar(dir.path(), "mdl-agent-companion-1.0.0.jar");
        let found = find_companion_jar_in(dir.path()).unwrap();
        assert_eq!(
            found.file_name().unwrap().to_str().unwrap(),
            "mdl-agent-companion-1.0.0.jar"
        );

        // newest build preferred
        fake_jar(dir.path(), "mdl-agent-companion-2.0.0.jar");
        let found = find_companion_jar_in(dir.path()).unwrap();
        assert_eq!(
            found.file_name().unwrap().to_str().unwrap(),
            "mdl-agent-companion-2.0.0.jar"
        );
    }

    #[tokio::test]
    async fn test_install_uninstall_lifecycle() {
        let source_dir = tempfile::tempdir().unwrap();
        fake_jar(source_dir.path(), "mdl-agent-companion-1.0.0.jar");

        let instance = tempfile::tempdir().unwrap();

        assert!(!is_installed(instance.path()));
        assert_eq!(uninstall(instance.path()).await.unwrap(), 0);

        // Simulate install without relying on current_exe location:
        let mods_dir = instance.path().join("mods");
        tokio::fs::create_dir_all(&mods_dir).await.unwrap();
        let dest = mods_dir.join("mdl-agent-companion-1.0.0.jar");
        tokio::fs::copy(source_dir.path().join("mdl-agent-companion-1.0.0.jar"), &dest)
            .await
            .unwrap();

        assert!(is_installed(instance.path()));
        assert_eq!(installed_companions(instance.path()).len(), 1);
        assert_eq!(uninstall(instance.path()).await.unwrap(), 1);
        assert!(!is_installed(instance.path()));
    }
}
