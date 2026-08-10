// Content search & download from Modrinth (Alpha 7).
//
// Supports three content kinds: mods, resource packs and shaders. Search
// uses the Modrinth v2 search endpoint; install resolves the best matching
// version and downloads it into the cache, then installs a copy into the
// instance (resource packs -> resourcepacks/, shaders -> shaderpacks/,
// mods -> mods/).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
}

impl ContentKind {
    pub fn project_type(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::ResourcePack => "resourcepack",
            ContentKind::Shader => "shader",
        }
    }

    pub fn instance_dir(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    #[serde(default)]
    pub icon_url: String,
    #[serde(default)]
    pub latest_version: String,
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    version_number: String,
    files: Vec<VersionFile>,
    #[serde(default)]
    loaders: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    primary: bool,
}

fn client() -> Result<reqwest::Client> {
    crate::util::http::create_http_client()
}

/// Search Modrinth for a content kind. Returns hits ordered by downloads.
pub async fn search(kind: ContentKind, query: &str, mc_version: Option<&str>, loader: Option<&str>, limit: usize) -> Result<Vec<SearchHit>> {
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", kind.project_type())]];
    if let Some(v) = mc_version {
        facets.push(vec![format!("versions:{}", v)]);
    }
    if let Some(l) = loader {
        facets.push(vec![format!("categories:{}", l)]);
    }
    let facets_json = serde_json::to_string(&facets)?;

    let url = format!(
        "{}/search?query={}&facets={}&index=downloads&limit={}",
        MODRINTH_API,
        urlencoded(query),
        urlencoded(&facets_json),
        limit
    );

    let response = client()?
        .get(&url)
        .header("User-Agent", format!("MDL/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("Failed to query Modrinth search")?;
    if !response.status().is_success() {
        anyhow::bail!("Modrinth search returned HTTP {}", response.status());
    }
    let parsed: SearchResponse = response.json().await.context("Failed to parse Modrinth search")?;
    Ok(parsed.hits)
}

/// Resolve the best downloadable file for a project on a given MC version + loader.
async fn resolve_file(project_id: &str, mc_version: Option<&str>, loader: Option<&str>) -> Result<VersionFile> {
    let mut url = format!("{}/project/{}/version?", MODRINTH_API, project_id);
    if let Some(v) = mc_version {
        url.push_str(&format!("game_versions=[\"{}\"]&", v));
    }
    if let Some(l) = loader {
        url.push_str(&format!("loaders=[\"{}\"]&", l));
    }
    let response = client()?
        .get(&url)
        .header("User-Agent", format!("MDL/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("Failed to query Modrinth versions")?;
    if !response.status().is_success() {
        anyhow::bail!("Modrinth versions returned HTTP {}", response.status());
    }
    let versions: Vec<VersionEntry> = response.json().await.context("Failed to parse versions")?;
    let latest = versions.first().context("No matching version found")?;
    let file = latest
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| latest.files.first())
        .context("Version has no files")?;
    Ok(file.clone())
}

/// Download a content item and install a copy into the instance directory.
/// Returns the installed file path.
pub async fn install_content(
    kind: ContentKind,
    hit: &SearchHit,
    instance_dir: &Path,
    mc_version: Option<&str>,
    loader: Option<&str>,
) -> Result<PathBuf> {
    let file = resolve_file(&hit.project_id, mc_version, loader).await?;

    // Download into a temp location in the instance dir, then move.
    let target_dir = instance_dir.join(kind.instance_dir());
    std::fs::create_dir_all(&target_dir)?;
    let dest = target_dir.join(&file.filename);
    if dest.exists() {
        return Ok(dest); // already installed
    }

    crate::version::downloader::download_file(&file.url, &dest, None).await?;
    Ok(dest)
}

fn urlencoded(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencoded() {
        assert_eq!(urlencoded("hello world"), "hello%20world");
        assert_eq!(urlencoded("[\"a\"]"), "%5B%22a%22%5D");
    }

    #[test]
    fn test_kinds() {
        assert_eq!(ContentKind::Shader.project_type(), "shader");
        assert_eq!(ContentKind::ResourcePack.instance_dir(), "resourcepacks");
    }
}
