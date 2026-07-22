// File downloader with progress tracking and checksum verification

use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::util::checksum::verify_sha1;

/// Download a file with checksum verification
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
) -> Result<()> {
    debug!("Downloading {} to {:?}", url, dest);

    // Create parent directory if it doesn't exist
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directory")?;
    }

    // Download file
    let response = reqwest::get(url)
        .await
        .context(format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error {}: {}", response.status(), url);
    }

    let content_length = response.content_length();
    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    // Verify size if content-length was provided
    if let Some(expected_size) = content_length {
        if bytes.len() as u64 != expected_size {
            anyhow::bail!(
                "Size mismatch for {}: expected {} bytes, got {} bytes",
                url,
                expected_size,
                bytes.len()
            );
        }
    }

    // Verify checksum if provided
    if let Some(expected) = expected_sha1 {
        debug!("Verifying SHA1 checksum");
        if !verify_sha1(&bytes, expected) {
            anyhow::bail!("Checksum mismatch for {}", url);
        }
    }

    // Write to file
    let mut file = fs::File::create(dest)
        .await
        .context("Failed to create file")?;

    file.write_all(&bytes)
        .await
        .context("Failed to write file")?;

    file.sync_all()
        .await
        .context("Failed to sync file to disk")?;

    info!("Downloaded {} ({} bytes)", dest.display(), bytes.len());
    Ok(())
}

/// Download a file with progress callback
pub async fn download_file_with_progress<F>(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    mut progress_callback: F,
) -> Result<()>
where
    F: FnMut(u64, u64),
{
    debug!("Downloading {} to {:?}", url, dest);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directory")?;
    }

    let response = reqwest::get(url)
        .await
        .context(format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error {}: {}", response.status(), url);
    }

    let total_size = response.content_length().unwrap_or(0);
    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    progress_callback(bytes.len() as u64, total_size);

    // Verify size if content-length was provided
    if total_size > 0 && bytes.len() as u64 != total_size {
        anyhow::bail!(
            "Size mismatch for {}: expected {} bytes, got {} bytes",
            url,
            total_size,
            bytes.len()
        );
    }

    if let Some(expected) = expected_sha1 {
        debug!("Verifying SHA1 checksum");
        if !verify_sha1(&bytes, expected) {
            anyhow::bail!("Checksum mismatch for {}", url);
        }
    }

    let mut file = fs::File::create(dest)
        .await
        .context("Failed to create file")?;

    file.write_all(&bytes)
        .await
        .context("Failed to write file")?;

    file.sync_all()
        .await
        .context("Failed to sync file to disk")?;

    info!("Downloaded {} ({} bytes)", dest.display(), bytes.len());
    Ok(())
}

/// Download multiple files concurrently
pub async fn download_files(downloads: Vec<DownloadTask>) -> Result<Vec<Result<()>>> {
    let futures = downloads.into_iter().map(|task| async move {
        download_file(&task.url, &task.dest, task.sha1.as_deref()).await
    });

    let results = futures_util::future::join_all(futures).await;
    Ok(results)
}

/// Download task specification
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: std::path::PathBuf,
    pub sha1: Option<String>,
}

impl DownloadTask {
    pub fn new(url: String, dest: std::path::PathBuf) -> Self {
        Self {
            url,
            dest,
            sha1: None,
        }
    }

    pub fn with_sha1(mut self, sha1: String) -> Self {
        self.sha1 = Some(sha1);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_download_file() {
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("test.json");

        // Test with a small public file
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        let result = download_file(url, &dest, None).await;

        assert!(result.is_ok());
        assert!(dest.exists());
    }
}
