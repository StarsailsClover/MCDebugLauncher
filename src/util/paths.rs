// Path utilities

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Get the data directory for MDL
pub fn get_data_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .context("Failed to determine data directory")?
        .join("mdl");

    Ok(data_dir)
}

/// Get the config directory for MDL
pub fn get_config_dir() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Failed to determine config directory")?
        .join("mdl");

    Ok(config_dir)
}

/// Get the cache directory for MDL
pub fn get_cache_dir() -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .context("Failed to determine cache directory")?
        .join("mdl");

    Ok(cache_dir)
}

/// Get the instances directory
pub fn get_instances_dir() -> Result<PathBuf> {
    Ok(get_data_dir()?.join("instances"))
}

/// Get the versions cache directory
pub fn get_versions_cache_dir() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("versions"))
}

/// Get the libraries cache directory
pub fn get_libraries_cache_dir() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("libraries"))
}

/// Get the assets cache directory
pub fn get_assets_cache_dir() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("assets"))
}

/// Get the Java runtimes cache directory. Auto-downloaded JDK/JRE builds are
/// installed here, one subdirectory per major version (e.g. java/21).
pub fn get_java_cache_dir() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("java"))
}

/// Ensure a directory exists
pub async fn ensure_dir(path: &PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .context(format!("Failed to create directory: {:?}", path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_data_dir() {
        let dir = get_data_dir();
        assert!(dir.is_ok());
        println!("Data dir: {:?}", dir.unwrap());
    }

    #[test]
    fn test_get_config_dir() {
        let dir = get_config_dir();
        assert!(dir.is_ok());
        println!("Config dir: {:?}", dir.unwrap());
    }
}
