// Minecraft asset downloader.
//
// Minecraft ships its textures, sounds, language files and other resources
// separately from the client JAR. Each version references an "asset index"
// (identified by an id such as "17") which maps virtual paths to content
// hashes. The launcher must download:
//   1. the index JSON -> <assets>/indexes/<id>.json
//   2. every referenced object -> <assets>/objects/<first-2-hash-chars>/<hash>
// from the Mojang resources CDN. Without these files Minecraft cannot find
// "assets/indexes/<id>.json" and starts with no textures or sounds.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use crate::version::manifest::AssetIndex;

const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

/// Return the asset objects CDN base URL. If the environment variable
/// `MDL_ASSETS_MIRROR` is set, its value overrides the default Mojang CDN.
/// This is useful in regions where `resources.download.minecraft.net` is
/// unreliable; point to a mirror such as `https://bmclapi2.bangbang93.com`
/// or any other compatible asset proxy.
fn resources_base_url() -> String {
    std::env::var("MDL_ASSETS_MIRROR")
        .unwrap_or_else(|_| RESOURCES_BASE_URL.to_string())
}

/// Maximum number of asset objects downloaded concurrently. The object CDN is
/// happy to serve many small files in parallel; this bounds our open sockets.
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

#[derive(Debug, Deserialize)]
pub struct AssetIndexFile {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AssetObject {
    pub hash: String,
    #[allow(dead_code)]
    pub size: u64,
}

/// Download the asset index and every referenced object into `assets_dir`.
///
/// This is idempotent: the index and each object are content-addressed by
/// SHA1, so existing files are skipped and re-running only fetches what is
/// missing. Safe to call on every launch.
pub async fn download_assets(asset_index: &AssetIndex, assets_dir: &Path) -> Result<()> {
    let indexes_dir = assets_dir.join("indexes");
    let objects_dir = assets_dir.join("objects");
    fs::create_dir_all(&indexes_dir).await?;
    fs::create_dir_all(&objects_dir).await?;

    // 1. Fetch (or reuse) the asset index JSON.
    let index_path = indexes_dir.join(format!("{}.json", asset_index.id));
    let index_content = if index_path.exists() {
        fs::read_to_string(&index_path).await?
    } else {
        tracing::info!("Downloading asset index {}", asset_index.id);
        let response = reqwest::get(&asset_index.url)
            .await
            .with_context(|| format!("Failed to download asset index from {}", asset_index.url))?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to download asset index: HTTP {}", response.status());
        }
        let text = response.text().await?;
        fs::write(&index_path, &text).await?;
        text
    };

    let index: AssetIndexFile =
        serde_json::from_str(&index_content).context("Failed to parse asset index JSON")?;

    // 2. Determine which objects are missing.
    let mut missing: Vec<AssetObject> = Vec::new();
    for object in index.objects.values() {
        let sub_dir = &object.hash[0..2];
        let object_path = objects_dir.join(sub_dir).join(&object.hash);
        if !object_path.exists() {
            missing.push(object.clone());
        }
    }

    if missing.is_empty() {
        tracing::info!("All {} assets already present", index.objects.len());
        return Ok(());
    }

    tracing::info!(
        "Downloading {} missing assets ({} total)",
        missing.len(),
        index.objects.len()
    );

    // 3. Download missing objects with bounded concurrency. A collection-level
    // progress bar (one bar for the whole batch, gated to interactive TTYs)
    // shows aggregate progress instead of hundreds of per-file bars.
    let show_pb = crate::util::progress::should_show_download_progress(0);
    let batch_bar = if show_pb {
        let bar = indicatif::ProgressBar::new(missing.len() as u64);
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            bar.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{msg} [{bar:40.green/blue}] {pos}/{len} ({eta})")
                    .unwrap()
                    .progress_chars("=>-"),
            );
            bar.set_message("Assets".to_string());
            bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(10));
        }
        Some(std::sync::Arc::new(bar))
    } else {
        None
    };

    let objects_dir = Arc::new(objects_dir.to_path_buf());
    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut tasks = Vec::new();

    for object in missing {
        let objects_dir = Arc::clone(&objects_dir);
        let semaphore = Arc::clone(&semaphore);
        let bar = batch_bar.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let res = download_object(&object, &objects_dir).await;
            if let Some(b) = &bar {
                b.inc(1);
            }
            res
        }));
    }

    let mut failures = 0;
    for task in tasks {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                failures += 1;
                tracing::warn!("Asset download failed: {}", e);
            }
            Err(e) => {
                failures += 1;
                tracing::warn!("Asset download task panicked: {}", e);
            }
        }
    }
    if let Some(bar) = batch_bar {
        bar.finish_and_clear();
    }

    if failures > 0 {
        anyhow::bail!("{} asset object(s) failed to download", failures);
    }

    tracing::info!("Assets downloaded successfully");
    Ok(())
}

async fn download_object(object: &AssetObject, objects_dir: &Path) -> Result<()> {
    let sub_dir = &object.hash[0..2];
    let target_dir = objects_dir.join(sub_dir);
    let target_path = target_dir.join(&object.hash);

    if target_path.exists() {
        return Ok(());
    }

    fs::create_dir_all(&target_dir).await?;

    let url = format!("{}/{}/{}", resources_base_url(), sub_dir, object.hash);

    // Retry up to 3 times with exponential backoff. CDN hiccups under concurrent
    // load are common and a single retry usually recovers them.
    let mut last_err = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1));
            tokio::time::sleep(delay).await;
            tracing::debug!(
                "Retrying asset {} (attempt {})",
                &object.hash[..8],
                attempt + 1
            );
        }

        match try_download_asset(&url, &target_path, &object.hash).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&target_path).await;
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap())
}

async fn try_download_asset(url: &str, target_path: &Path, hash: &str) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to fetch asset from {}", url))?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for asset {}", response.status(), &hash[..8]);
    }

    // Stream the body directly to disk so we never hold the full asset in
    // memory. With 16 concurrent downloads this bounds peak memory.
    let mut file = tokio::fs::File::create(target_path).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    drop(file);

    // Verify SHA1 from disk (reads in 64KB chunks).
    let ok = crate::util::checksum::verify_sha1_file(target_path, hash)
        .await
        .unwrap_or(false);
    if !ok {
        let _ = tokio::fs::remove_file(target_path).await;
        anyhow::bail!("SHA1 mismatch for asset {}", &hash[..8]);
    }

    Ok(())
}
