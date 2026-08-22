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

    // Display name for the progress bar: prefer the destination filename.
    let display_name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string();

    for candidate in &candidates {
        match try_single_source(candidate, dest, expected_sha1, Some(&display_name)).await {
            Ok(bytes_written) => {
                // Register the downloaded file in the cache by copying it
                // from the destination into the cache dir. This avoids
                // buffering the full file in memory — the previous
                // implementation returned Vec<u8> and held the entire
                // download in RAM, causing ~1.9GB peak usage on first launch.
                if let Ok(mut cache) = crate::util::cache::DownloadCache::new() {
                    let rel = std::path::Path::new("dl")
                        .join(sanitize_for_cache(&cache_key))
                        .with_extension("bin");
                    let src = cache.root().join(&rel);
                    if let Ok(()) = copy_file_for_cache(dest, &src).await {
                        cache.register(&cache_key, &rel, bytes_written);
                    }
                }
                info!("Downloaded {} ({} bytes) from {}", dest.display(), bytes_written, candidate);
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

/// Copy a file into the cache directory. Uses a bounded buffer to avoid
/// loading the entire file into memory.
async fn copy_file_for_cache(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut reader = fs::File::open(src).await?;
    let mut writer = fs::File::create(dest).await?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await?;
        if n == 0 {
            break;
        }
        tokio::io::AsyncWriteExt::write_all(&mut writer, &buf[..n]).await?;
    }
    writer.sync_all().await?;
    Ok(())
}

/// Try one source URL: HEAD to learn size/range support, then either
/// chunked-parallel or single-shot GET. Streams the response body directly
/// to `dest` (or a temp file when dest is empty), avoiding holding the
/// full download in memory.
///
/// Returns the number of bytes written.
async fn try_single_source(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    display_name: Option<&str>,
) -> Result<u64> {
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

    // Choose the output target: the real destination, or a temp file when
    // the caller only wants bytes (download_bytes path passes "").
    let use_temp = dest.as_os_str().is_empty();
    let out_path: std::path::PathBuf = if use_temp {
        std::env::temp_dir().join(format!("mdl_dl_{}", std::process::id()))
    } else {
        dest.to_path_buf()
    };
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).await.ok();
    }

    let bytes_written = if ranged && total >= CHUNK_THRESHOLD {
        debug!("Chunked download ({} bytes, {} chunks)", total, CHUNK_COUNT);
        chunked_download_to_file(&client, url, total, display_name, &out_path).await?
    } else {
        use futures_util::StreamExt;
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to download from {}", url))?;
        if !response.status().is_success() {
            anyhow::bail!("HTTP error {}: {}", response.status(), url);
        }
        // Stream the body directly to disk so we never hold the full file
        // in memory. This is the critical fix for the 1.9GB memory issue.
        let show_pb = display_name
            .map(|_| crate::util::progress::should_show_download_progress(total))
            .unwrap_or(false);
        let pb = if show_pb {
            let bar = crate::util::progress::create_download_bar(total);
            bar.set_message(display_name.unwrap_or("download").to_string());
            Some(bar)
        } else {
            None
        };
        let mut file = fs::File::create(&out_path).await
            .with_context(|| format!("Failed to create output file {}", out_path.display()))?;
        let mut written: u64 = 0;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .with_context(|| format!("Failed to read response body from {}", url))?;
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
            if let Some(bar) = &pb {
                bar.set_position(written);
            }
        }
        file.sync_all().await?;
        if let Some(bar) = pb {
            bar.finish_and_clear();
        }
        written
    };

    if total > 0 && bytes_written != total {
        anyhow::bail!(
            "Size mismatch for {}: expected {} bytes, got {}",
            url,
            total,
            bytes_written
        );
    }

    // Verify checksum from disk (reads in 64KB chunks, not the full file).
    if let Some(expected) = expected_sha1 {
        let ok = crate::util::checksum::verify_sha1_file(&out_path, expected)
            .await
            .unwrap_or(false);
        if !ok {
            if use_temp {
                let _ = fs::remove_file(&out_path).await;
            }
            anyhow::bail!("Checksum mismatch for {}", url);
        }
    }

    Ok(bytes_written)
}

/// Download `total` bytes in `CHUNK_COUNT` parallel Range requests and
/// write them to `out_path` in order. Each chunk streams directly to a
/// temp file and is concatenated, avoiding holding the full download in
/// memory.
async fn chunked_download_to_file(
    client: &reqwest::Client,
    url: &str,
    total: u64,
    display_name: Option<&str>,
    out_path: &Path,
) -> Result<u64> {
    use futures_util::StreamExt;
    let chunk_size = (total + CHUNK_COUNT as u64 - 1) / CHUNK_COUNT as u64;
    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < total {
        let end = (start + chunk_size - 1).min(total - 1);
        ranges.push((start, end));
        start = end + 1;
    }

    // Shared progress bar across all parallel chunks (driven by received bytes).
    let show_pb = display_name
        .map(|_| crate::util::progress::should_show_download_progress(total))
        .unwrap_or(false);
    let pb: Option<std::sync::Arc<indicatif::ProgressBar>> = if show_pb {
        let bar = crate::util::progress::create_download_bar(total);
        bar.set_message(display_name.unwrap_or("download").to_string());
        Some(std::sync::Arc::new(bar))
    } else {
        None
    };

    // Each chunk writes to its own temp file, then we concatenate them
    // sequentially into out_path. This bounds memory to the chunk size.
    let temp_dir = std::env::temp_dir();
    let chunk_temp_paths: Vec<std::path::PathBuf> = (0..ranges.len())
        .map(|i| temp_dir.join(format!("mdl_chunk_{}_{}", std::process::id(), i)))
        .collect();

    let url_owned = url.to_string();
    let client = client.clone();
    let chunk_paths_clone = chunk_temp_paths.clone();
    let handles: Vec<_> = ranges
        .into_iter()
        .enumerate()
        .map(|(i, (s, e))| {
            let client = client.clone();
            let url = url_owned.clone();
            let pb = pb.clone();
            let chunk_path = chunk_paths_clone[i].clone();
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
                let mut file = fs::File::create(&chunk_path).await?;
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    file.write_all(&chunk).await?;
                    if let Some(bar) = &pb {
                        bar.inc(chunk.len() as u64);
                    }
                }
                file.sync_all().await?;
                Ok::<(), anyhow::Error>(())
            })
        })
        .collect();

    let mut chunk_errors = Vec::new();
    for h in handles {
        if let Err(e) = h.await {
            chunk_errors.push(e);
        }
    }
    if let Some(bar) = pb {
        bar.finish_and_clear();
    }
    if !chunk_errors.is_empty() {
        // Clean up temp files
        for p in &chunk_temp_paths {
            let _ = fs::remove_file(p).await;
        }
        anyhow::bail!("Chunked download failed: {} error(s)", chunk_errors.len());
    }

    // Concatenate chunk files into the output file.
    let mut out_file = fs::File::create(out_path).await?;
    let mut written: u64 = 0;
    for chunk_path in &chunk_temp_paths {
        let mut reader = fs::File::open(chunk_path).await?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await?;
            if n == 0 {
                break;
            }
            out_file.write_all(&buf[..n]).await?;
            written += n as u64;
        }
        let _ = fs::remove_file(chunk_path).await;
    }
    out_file.sync_all().await?;

    if written != total {
        anyhow::bail!("Chunked reassembly size mismatch: {} != {}", written, total);
    }
    Ok(written)
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

/// Download a file with progress callback (streaming with real-time updates).
pub async fn download_file_with_progress<F>(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    mut progress_callback: F,
) -> Result<()>
where
    F: FnMut(u64, u64),
{
    
    
    debug!("Downloading {} to {:?} with progress", url, dest);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create parent directory")?;
    }

    // Try cache first
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
        let result = try_download_with_progress(candidate, dest, expected_sha1, &mut progress_callback).await;
        match result {
            Ok(bytes_len) => {
                debug!("Downloaded {} ({} bytes) from {}", dest.display(), bytes_len, candidate);
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

/// Try to download from a single source with streaming progress updates.
/// Streams directly to disk — the full body is never buffered in memory.
async fn try_download_with_progress<F>(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    progress_callback: &mut F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    use futures_util::StreamExt;

    let client = crate::util::http::create_download_client()?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error {}: {}", response.status(), url);
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    // Report initial progress
    progress_callback(0, total_size);

    // Stream directly to the destination file — no in-memory buffer.
    let mut file = fs::File::create(dest).await
        .with_context(|| format!("Failed to create output file {}", dest.display()))?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Failed to read chunk from {}", url))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress_callback(downloaded, total_size);
    }
    file.sync_all().await?;

    // Verify checksum from disk (bounded memory).
    if let Some(expected) = expected_sha1 {
        let ok = crate::util::checksum::verify_sha1_file(dest, expected)
            .await
            .unwrap_or(false);
        if !ok {
            anyhow::bail!("Checksum mismatch for {}", url);
        }
    }

    Ok(downloaded)
}

/// Fetch bytes only (no file write), with mirror fallback.
/// Streams to a temp file then reads it back — the temp file is cleaned up.
pub async fn download_bytes(url: &str, expected_sha1: Option<&str>) -> Result<Vec<u8>> {
    let candidates = crate::util::mirrors::candidate_urls(url).await;
    let mut last_err: Option<anyhow::Error> = None;
    for candidate in &candidates {
        match try_single_source(candidate, Path::new(""), expected_sha1, None).await {
            Ok(_bytes_written) => {
                // The data was written to a temp file; read it back.
                let temp_path = std::env::temp_dir().join(format!("mdl_dl_{}", std::process::id()));
                let result = fs::read(&temp_path).await;
                let _ = fs::remove_file(&temp_path).await;
                return result.map_err(|e| anyhow::anyhow!("Failed to read downloaded temp file: {}", e));
            }
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
