// Mirror source discovery and selection (Alpha 7).
//
// MDL ships a small list of Mojang download sources (official + Chinese
// mirrors). Before heavy downloads we probe each source with a tiny request
// and rank them by latency, then all downloads prefer the best source and
// fall back down the list (flexible source switching).
//
// URL mapping follows the OpenBMCLAPI convention:
//   launchermeta / piston-meta / piston-data / launcher  -> <mirror root>
//   libraries.minecraft.net                              -> <mirror>/maven
//   resources.download.minecraft.net                     -> <mirror>/assets

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A downloadable source: base URLs for the three Mojang namespaces.
#[derive(Debug, Clone)]
pub struct MirrorSource {
    pub name: &'static str,
    /// Replacement for https://piston-meta.mojang.com etc. (root)
    pub root: &'static str,
    /// Replacement prefix for libraries.minecraft.net
    pub maven: &'static str,
    /// Replacement prefix for resources.download.minecraft.net
    pub assets: &'static str,
    pub official: bool,
}

pub const MIRRORS: &[MirrorSource] = &[
    MirrorSource {
        name: "bmclapi",
        root: "https://bmclapi2.bangbang93.com",
        maven: "https://bmclapi2.bangbang93.com/maven",
        assets: "https://bmclapi2.bangbang93.com/assets",
        official: false,
    },
    MirrorSource {
        name: "official",
        root: "https://piston-meta.mojang.com",
        maven: "https://libraries.minecraft.net",
        assets: "https://resources.download.minecraft.net",
        official: true,
    },
];

/// Rewrite an official Mojang URL onto a mirror. Returns None when the URL
/// does not belong to a known Mojang namespace (caller keeps original).
pub fn map_url(mirror: &MirrorSource, url: &str) -> Option<String> {
    const ROOT_PREFIXES: &[&str] = &[
        "https://piston-meta.mojang.com",
        "https://launchermeta.mojang.com",
        "https://piston-data.mojang.com",
        "https://launcher.mojang.com",
        "https://piston-data.minecraft.net",
    ];
    for p in ROOT_PREFIXES {
        if let Some(rest) = url.strip_prefix(p) {
            return Some(format!("{}{}", mirror.root, rest));
        }
    }
    if let Some(rest) = url.strip_prefix("https://libraries.minecraft.net") {
        return Some(format!("{}{}", mirror.maven, rest));
    }
    if let Some(rest) = url.strip_prefix("https://resources.download.minecraft.net") {
        return Some(format!("{}{}", mirror.assets, rest));
    }
    None
}

/// Rewrite a URL onto the given mirror, keeping the original when the URL is
/// not a known Mojang host.
pub fn url_for(mirror: &MirrorSource, url: &str) -> String {
    map_url(mirror, url).unwrap_or_else(|| url.to_string())
}

/// Result of a live probe for one mirror.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorProbe {
    pub name: String,
    pub ok: bool,
    pub latency_ms: u64,
}

/// On-disk cache of the last probe so we do not re-probe on every command.
#[derive(Debug, Serialize, Deserialize)]
struct ProbeCache {
    probed_at: u64,
    order: Vec<String>,
}

const PROBE_TTL_SECS: u64 = 600; // 10 minutes
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

fn cache_path() -> Option<std::path::PathBuf> {
    crate::util::paths::get_data_dir()
        .ok()
        .map(|d| d.join("mirror-status.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Probe all mirrors with a tiny GET and return them ordered by latency
/// (fastest first; official always last among OK mirrors so mirrors win
/// when available, but official is the guaranteed fallback).
pub async fn probe_all() -> Vec<MirrorProbe> {
    let client = crate::util::http::create_http_client().ok();
    let Some(client) = client else {
        return MIRRORS
            .iter()
            .map(|m| MirrorProbe { name: m.name.into(), ok: false, latency_ms: u64::MAX })
            .collect();
    };

    let mut probes = Vec::new();
    for m in MIRRORS {
        let url = format!("{}/mc/game/version_manifest_v2.json", m.root);
        let start = Instant::now();
        let ok = match tokio::time::timeout(PROBE_TIMEOUT, client.get(&url).send()).await {
            Ok(Ok(resp)) => resp.status().is_success(),
            _ => false,
        };
        let latency = start.elapsed().as_millis() as u64;
        tracing::debug!("Mirror {} probe: ok={} latency={}ms", m.name, ok, latency);
        probes.push(MirrorProbe { name: m.name.into(), ok, latency_ms: latency });
    }
    // Sort: ok first by latency; not-ok last.
    probes.sort_by(|a, b| match (a.ok, b.ok) {
        (true, true) => a.latency_ms.cmp(&b.latency_ms),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.latency_ms.cmp(&b.latency_ms),
    });
    probes
}

/// Return mirrors ordered by preference, using a 10-minute on-disk cache of
/// probe results. Never fails: on any error returns the static order.
pub async fn ordered_sources() -> Vec<&'static MirrorSource> {
    // Fast path: cached order.
    if let Some(path) = cache_path() {
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if let Ok(cache) = serde_json::from_str::<ProbeCache>(&content) {
                if now_secs().saturating_sub(cache.probed_at) < PROBE_TTL_SECS {
                    let mut out: Vec<&'static MirrorSource> = cache
                        .order
                        .iter()
                        .filter_map(|n| MIRRORS.iter().find(|m| m.name == n))
                        .collect();
                    // Always append any mirror missing from the cache.
                    for m in MIRRORS {
                        if !out.iter().any(|o| o.name == m.name) {
                            out.push(m);
                        }
                    }
                    return out;
                }
            }
        }
    }

    // Slow path: probe now.
    let probes = probe_all().await;
    let order: Vec<&'static MirrorSource> = probes
        .iter()
        .filter_map(|p| MIRRORS.iter().find(|m| m.name == p.name))
        .collect();

    if let Some(path) = cache_path() {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let cache = ProbeCache {
            probed_at: now_secs(),
            order: order.iter().map(|m| m.name.to_string()).collect(),
        };
        if let Ok(json) = serde_json::to_string(&cache) {
            let _ = tokio::fs::write(&path, json).await;
        }
    }
    order
}

/// Pick the best mirror for a URL: the first ordered source that can map it;
/// returns the ordered list of concrete URLs to try (best first). The
/// original URL is always included as the final fallback.
pub async fn candidate_urls(url: &str) -> Vec<String> {
    let mut out = Vec::new();
    for m in ordered_sources().await {
        if let Some(mapped) = map_url(m, url) {
            if mapped != url {
                out.push(mapped);
            }
        }
    }
    out.push(url.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_url_root() {
        let m = &MIRRORS[0];
        assert_eq!(
            map_url(m, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
    }

    #[test]
    fn test_map_url_maven() {
        let m = &MIRRORS[0];
        assert_eq!(
            map_url(m, "https://libraries.minecraft.net/net/java/jinput/jinput/2.0.5/jinput-2.0.5.jar")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/maven/net/java/jinput/jinput/2.0.5/jinput-2.0.5.jar"
        );
    }

    #[test]
    fn test_map_url_assets() {
        let m = &MIRRORS[0];
        assert_eq!(
            map_url(m, "https://resources.download.minecraft.net/ab/cd1234").unwrap(),
            "https://bmclapi2.bangbang93.com/assets/ab/cd1234"
        );
    }

    #[test]
    fn test_map_url_passthrough() {
        let m = &MIRRORS[0];
        assert!(map_url(m, "https://api.modrinth.com/v2/search").is_none());
        assert_eq!(
            url_for(m, "https://api.modrinth.com/v2/search"),
            "https://api.modrinth.com/v2/search"
        );
    }

    #[test]
    fn test_official_identity() {
        let m = &MIRRORS[1];
        let u = "https://piston-meta.mojang.com/x.json";
        assert_eq!(map_url(m, u).unwrap(), u);
    }
}
