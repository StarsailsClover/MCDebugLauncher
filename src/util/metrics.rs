// Launch metrics collection (v26.2-alpha.9).
//
// Captures lightweight, local-only observability data per launch:
//   - total launch duration (spawn time)
//   - time-to-ready when --wait-ready is used (Despotes poll latency)
//   - download bytes / file count / cache hit count from the downloader
//
// Storage: <instance>/runtime/metrics.json holds the latest launch;
// runtime/metrics.jsonl accumulates history (one JSON object per line).
// Nothing ever leaves the machine - there is deliberately no network
// telemetry path.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static DOWNLOAD_BYTES: AtomicU64 = AtomicU64::new(0);
static DOWNLOAD_COUNT: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);

/// Record a successful network download of `bytes` bytes.
pub fn record_download(bytes: u64) {
    DOWNLOAD_BYTES.fetch_add(bytes, Ordering::Relaxed);
    DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Record a cache hit that avoided a network download.
pub fn record_cache_hit() {
    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

/// One recorded launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchMetrics {
    /// RFC3339 timestamp of the launch.
    pub timestamp: String,
    pub instance: String,
    pub pid: u32,
    #[serde(default)]
    pub detached: bool,
    /// Seconds from launch start to process spawn.
    pub spawn_secs: f64,
    /// Seconds from spawn until the game broadcast ready (only when
    /// --wait-ready was used and succeeded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_secs: Option<f64>,
    /// Network bytes downloaded during this launch.
    pub download_bytes: u64,
    /// Number of files downloaded over the network.
    pub downloads: u64,
    /// Number of copy-installs served from the download cache.
    pub cache_hits: u64,
}

/// Snapshot the process-global download counters.
pub fn snapshot_counters() -> (u64, u64, u64) {
    (
        DOWNLOAD_BYTES.load(Ordering::Relaxed),
        DOWNLOAD_COUNT.load(Ordering::Relaxed),
        CACHE_HITS.load(Ordering::Relaxed),
    )
}

/// Persist a finished launch: latest snapshot + append-only history.
pub fn save_launch(instance_dir: &Path, metrics: &LaunchMetrics) -> Result<()> {
    let runtime = instance_dir.join("runtime");
    std::fs::create_dir_all(&runtime)?;

    let latest = runtime.join("metrics.json");
    std::fs::write(&latest, serde_json::to_string_pretty(metrics)?)?;

    use std::io::Write;
    let mut history = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime.join("metrics.jsonl"))?;
    writeln!(history, "{}", serde_json::to_string(metrics)?)?;
    Ok(())
}

/// Load the latest recorded launch for an instance.
pub fn load_latest(instance_dir: &Path) -> Option<LaunchMetrics> {
    let raw = std::fs::read_to_string(instance_dir.join("runtime").join("metrics.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Load the full recorded history (oldest first). Capped at the last 100
/// entries so pathological histories cannot balloon memory.
pub fn load_history(instance_dir: &Path) -> Vec<LaunchMetrics> {
    let raw = match std::fs::read_to_string(instance_dir.join("runtime").join("metrics.jsonl")) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<LaunchMetrics> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if out.len() > 100 {
        out = out.split_off(out.len() - 100);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_snapshot() {
        let (b0, d0, h0) = snapshot_counters();
        record_download(1024);
        record_download(512);
        record_cache_hit();
        let (b1, d1, h1) = snapshot_counters();
        assert_eq!(b1 - b0, 1536);
        assert_eq!(d1 - d0, 2);
        assert_eq!(h1 - h0, 1);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let m = LaunchMetrics {
            timestamp: "2026-08-22T00:00:00Z".into(),
            instance: "t".into(),
            pid: 42,
            detached: true,
            spawn_secs: 3.5,
            ready_secs: Some(30.0),
            download_bytes: 1000,
            downloads: 2,
            cache_hits: 1,
        };
        save_launch(dir.path(), &m).unwrap();
        // Latest readable.
        let got = load_latest(dir.path()).unwrap();
        assert_eq!(got.pid, 42);
        assert_eq!(got.ready_secs, Some(30.0));
        // History has exactly one entry.
        let hist = load_history(dir.path());
        assert_eq!(hist.len(), 1);
        // ready_secs omitted from JSON when None.
        let m2 = LaunchMetrics { ready_secs: None, ..m };
        save_launch(dir.path(), &m2).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("runtime").join("metrics.json")).unwrap();
        assert!(!raw.contains("ready_secs"));
    }

    #[test]
    fn test_load_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_latest(dir.path()).is_none());
        assert!(load_history(dir.path()).is_empty());
    }
}
