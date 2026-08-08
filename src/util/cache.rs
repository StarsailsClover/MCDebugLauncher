// Download cache with copy-install semantics (Alpha 7).
//
// Every downloaded artifact (game version jars, libraries, assets, mods,
// resource packs, shaders) is stored ONCE under the MDL cache. When an
// instance needs the artifact, MDL installs a *copy* from the cache instead
// of re-downloading. Cache entries carry a `fetched_at` timestamp and are
// eligible for eviction after a configurable TTL (default 7 days) counted
// from last use.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default cache TTL in days.
pub const DEFAULT_CACHE_DAYS: u64 = 7;

#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheMeta {
    /// artifact key -> { fetched_at, last_used, size }
    entries: HashMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub fetched_at: u64,
    pub last_used: u64,
    pub size: u64,
    /// Stable relative location of the source file inside the cache root.
    pub rel_path: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct DownloadCache {
    root: PathBuf,
    meta_path: PathBuf,
    meta: CacheMeta,
}

impl DownloadCache {
    pub fn new() -> Result<Self> {
        let root = crate::util::paths::get_cache_dir()?;
        std::fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create cache dir {}", root.display()))?;
        let meta_path = root.join("cache-meta.json");
        let meta = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|c| serde_json::from_str::<CacheMeta>(&c).ok())
            .unwrap_or_default();
        Ok(Self { root, meta_path, meta })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.meta) {
            let _ = std::fs::write(&self.meta_path, json);
        }
    }

    /// Register a freshly downloaded source file.
    pub fn register(&mut self, key: &str, rel_path: &Path, size: u64) {
        let now = now_secs();
        self.meta.entries.insert(
            key.to_string(),
            CacheEntry {
                fetched_at: now,
                last_used: now,
                size,
                rel_path: rel_path.to_string_lossy().to_string(),
            },
        );
        self.save();
    }

    /// Look up a cached source file; updates last_used. Returns the path if
    /// the file still exists on disk.
    pub fn lookup(&mut self, key: &str) -> Option<PathBuf> {
        let entry = self.meta.entries.get(key)?.clone();
        let path = self.root.join(&entry.rel_path);
        if path.exists() {
            if let Some(e) = self.meta.entries.get_mut(key) {
                e.last_used = now_secs();
            }
            self.save();
            Some(path)
        } else {
            self.meta.entries.remove(key);
            self.save();
            None
        }
    }

    /// Install a copy of a cached artifact into `dest`. Returns true on
    /// success.
    pub fn install_copy(&self, key: &str, dest: &Path) -> Result<bool> {
        let Some(src) = self.meta.entries.get(key).map(|e| self.root.join(&e.rel_path)) else {
            return Ok(false);
        };
        if !src.exists() {
            return Ok(false);
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, dest)
            .with_context(|| format!("Failed to copy cached artifact to {}", dest.display()))?;
        Ok(true)
    }

    /// Evict entries whose last_used is older than `days`. Returns number of
    /// files removed.
    pub fn evict_expired(&mut self, days: u64) -> usize {
        let cutoff = now_secs().saturating_sub(days * 86_400);
        let stale: Vec<String> = self
            .meta
            .entries
            .iter()
            .filter(|(_, e)| e.last_used < cutoff)
            .map(|(k, _)| k.clone())
            .collect();
        let mut removed = 0;
        for key in stale {
            if let Some(entry) = self.meta.entries.remove(&key) {
                let path = self.root.join(&entry.rel_path);
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.save();
        }
        removed
    }

    /// Total size of all cached entries (bytes).
    pub fn total_size(&self) -> u64 {
        self.meta.entries.values().map(|e| e.size).sum()
    }

    pub fn entry_count(&self) -> usize {
        self.meta.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_lookup_install_copy() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a cache root
        let src = dir.path().join("mods/fake-1.0.0.jar");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"cached-bytes").unwrap();

        let mut cache = DownloadCache {
            root: dir.path().to_path_buf(),
            meta_path: dir.path().join("cache-meta.json"),
            meta: CacheMeta::default(),
        };
        cache.register("fake-1.0.0", Path::new("mods/fake-1.0.0.jar"), 12);

        let found = cache.lookup("fake-1.0.0").unwrap();
        assert_eq!(found, src);

        let dest = dir.path().join("instance/mods/fake-1.0.0.jar");
        assert!(cache.install_copy("fake-1.0.0", &dest).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"cached-bytes");
    }

    #[test]
    fn test_evict_expired() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("old.jar");
        std::fs::write(&src, b"x").unwrap();

        let mut cache = DownloadCache {
            root: dir.path().to_path_buf(),
            meta_path: dir.path().join("cache-meta.json"),
            meta: CacheMeta::default(),
        };
        // Manually insert an ancient entry.
        cache.meta.entries.insert(
            "old".into(),
            CacheEntry {
                fetched_at: 1,
                last_used: 1,
                size: 1,
                rel_path: "old.jar".into(),
            },
        );
        assert_eq!(cache.evict_expired(DEFAULT_CACHE_DAYS), 1);
        assert!(!src.exists());
    }
}
