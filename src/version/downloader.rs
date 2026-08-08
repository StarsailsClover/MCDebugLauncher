// File downloader with mirror fallback, chunked parallel transfer and
// checksum verification (Alpha 7).
//
// Every download now:
// 1. Builds an ordered list of candidate URLs (best mirror first, official
//    last) via `util::mirrors`.
// 2. For large files, splits the transfer into parallel Range chunks when
//    the server advertises `Accept-Ranges: bytes`.
// 3. Falls back to the next source on any failure (flexible source switch).

use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::util::checksum::verify_sha1;

/// Minimum size (bytes) that triggers chunked parallel download.
const CHUNK_THRESHOLD: u64 = 4 * 1024 * 1024;
/// Number of parallel chunks.
const CHUNK_COUNT: usize = 4;

/// Download a file with checksum verification, mirror fallback and chunked
/// transfer for large files.
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
) -> Result<()> {
    debug!("Downloading {} to {:?}", url, dest);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directory")?;
    }

    // Cache-first: reuse a previously downloaded source file (copy install).
    let cache_key = cache_key(url, expected_sha1);
    if let Ok(mut cache) = crate::util::cache::DownloadCache::new() {
        if cache.lookup(&cache_key).is_some() {
            if cache.install_copy(&cache_key, dest).unwrap_or(false) {
                info!("Installed from cache (copy): {}", dest.display());
                return Ok(());
            }
        }
    }

    let candidates = crate::util::mirrors::candidate_urls(url).await;
    let mut last_err: Option<anyhow::Error> = None;

    for candidate in &candidates {
        match try_single_source(candidate, dest, expected_sha1).await {
            Ok(bytes) => {
                write_and_sync(dest, &bytes).await?;
                // Register the downloaded bytes as the cache source file.
                if let Ok(mut cache) = crate::util::cache::DownloadCache::new() {
                    let rel = std::path::Path::new("dl")
                        .join(sanitize_for_cache(&cache_key))
                        .with_extension("bin");
                    let src = cache.root().join(&rel);
                    if let Ok(()) = write_and_sync(&src, &bytes).await {
                        cache.register(&cache_key, &rel, bytes.len() as u64);
                    }
                }
                info!("Downloaded {} ({} bytes) from {}", dest.display(), bytes.len(), candidate);
                return Ok(());
            }
            Err(e) => {
                debug!("Source {} failed: {}", candidate, e);
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No download source succeeded for {}", url)))
        .with_context(|| format!("All sources failed for {}", url))
}

/// Try one source URL: HEAD to learn size/range support, then either
/// chunked-parallel or single-shot GET. Returns the full byte buffer after
/// optional sha1 verification.
async fn try_single_source(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
) -> Result<Vec<u8>> {
    let client = crate::util::http::create_download_client()?;

    // Probe for size + range support. Some hosts reject HEAD, so fall back
    // to a 1-byte Range GET to learn the total via Content-Range.
    let (mut total, mut ranged) = match client.head(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let total = resp.content_length().unwrap_or(0);
            let ranged = resp
                .headers()
                .get("accept-ranges")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("bytes"))
                .unwrap_or(false);
            (total, ranged)
        }
        _ => (0, false),
    };
    if total == 0 {
        if let Ok(resp) = client
            .get(url)
            .header("Range", "bytes=0-0")
            .send()
            .await
        {
            if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                if let Some(cr) = resp.headers().get("content-range").and_then(|v| v.to_str().ok()) {
                    if let Some(sz) = cr.rsplit('/').next() {
                        total = sz.parse().unwrap_or(0);
                        ranged = true;
                    }
                }
            }
        }
    }

    let bytes = if ranged && total >= CHUNK_THRESHOLD {
        debug!("Chunked download ({} bytes, {} chunks)", total, CHUNK_COUNT);
        chunked_download(&client, url, total).await?
    } else {
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to download from {}", url))?;
        if !response.status().is_success() {
            anyhow::bail!("HTTP error {}: {}", response.status(), url);
        }
        response
            .bytes()
            .await
            .with_context(|| format!("Failed to read response body from {}", url))?
            .to_vec()
    };

    if total > 0 && bytes.len() as u64 != total {
        anyhow::bail!(
            "Size mismatch for {}: expected {} bytes, got {}",
            url,
            total,
            bytes.len()
        );
    }

    if let Some(expected) = expected_sha1 {
        if !verify_sha1(&bytes, expected) {
            anyhow::bail!("Checksum mismatch for {}", url);
        }
    }
    Ok(bytes)
}

/// Download `total` bytes in `CHUNK_COUNT` parallel Range requests and
/// reassemble in order.
async fn chunked_download(
    client: &reqwest::Client,
    url: &str,
    total: u64,
) -> Result<Vec<u8>> {
    let chunk_size = (total + CHUNK_COUNT as u64 - 1) / CHUNK_COUNT as u64;
    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < total {
        let end = (start + chunk_size - 1).min(total - 1);
        ranges.push((start, end));
        start = end + 1;
    }

    let url_owned = url.to_string();
    let client = client.clone();
    let handles: Vec<_> = ranges
        .into_iter()
        .map(|(s, e)| {
            let client = client.clone();
            let url = url_owned.clone();
            tokio::spawn(async move {
                let resp = client
                    .get(&url)
                    .header("Range", format!("bytes={}-{}", s, e))
                    .send()
                    .await
                    .with_context(|| format!("Chunk request failed for {}", url))?;
                if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    anyhow::bail!("Chunk returned HTTP {} for {}", resp.status(), url);
                }
                Ok::<Vec<u8>, anyhow::Error>(resp.bytes().await?.to_vec())
            })
        })
        .collect();

    let mut out = Vec::with_capacity(total as usize);
    for h in handles {
        let chunk = h
            .await
            .context("Chunk task panicked")?
            .context("Chunk download failed")?;
        out.extend_from_slice(&chunk);
    }
    if out.len() as u64 != total {
        anyhow::bail!("Chunked reassembly size mismatch: {} != {}", out.len(), total);
    }
    Ok(out)
}

async fn write_and_sync(dest: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::create(dest)
        .await
        .context("Failed to create file")?;
    file.write_all(bytes).await.context("Failed to write file")?;
    file.sync_all().await.context("Failed to sync file to disk")?;
    Ok(())
}

fn cache_key(url: &str, sha1: Option<&str>) -> String {
    match sha1 {
        Some(h) => format!("sha1:{}", h),
        None => format!("url:{}", url),
    }
}

fn sanitize_for_cache(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect()
}

/// Download a file with progress callback (single-shot, mirror fallback).
pub async fn download_file_with_progress<F>(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    mut progress_callback: F,
) -> Result<()>
where
    F: FnMut(u64, u64),
{
    let bytes = download_bytes(url, expected_sha1).await?;
    progress_callback(bytes.len() as u64, bytes.len() as u64);
    write_and_sync(dest, &bytes).await?;
    info!("Downloaded {} ({} bytes)", dest.display(), bytes.len());
    Ok(())
}

/// Fetch bytes only (no file write), with mirror fallback.
pub async fn download_bytes(url: &str, expected_sha1: Option<&str>) -> Result<Vec<u8>> {
    let candidates = crate::util::mirrors::candidate_urls(url).await;
    let mut last_err: Option<anyhow::Error> = None;
    for candidate in &candidates {
        match try_single_source(candidate, Path::new(""), expected_sha1).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No download source succeeded for {}", url)))
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

        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        let result = download_file(url, &dest, None).await;

        assert!(result.is_ok());
        assert!(dest.exists());
    }

    #[test]
    fn test_chunk_math() {
        let total: u64 = 10;
        let chunk_size = (total + CHUNK_COUNT as u64 - 1) / CHUNK_COUNT as u64;
        assert_eq!(chunk_size, 3);
        let mut ranges = Vec::new();
        let mut start = 0u64;
        while start < total {
            let end = (start + chunk_size - 1).min(total - 1);
            ranges.push((start, end));
            start = end + 1;
        }
        assert_eq!(ranges, vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
    }
}
