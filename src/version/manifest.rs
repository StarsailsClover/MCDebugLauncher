// Minecraft version manifest structures and fetching

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Version manifest containing all available Minecraft versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionInfo>,
}

/// Latest release and snapshot versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

/// Basic version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default, rename = "complianceLevel")]
    pub compliance_level: Option<i32>,
}

/// Detailed version metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMetadata {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub arguments: Option<GameArguments>,
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>, // Legacy format
    pub downloads: Downloads,
    pub libraries: Vec<Library>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, rename = "minimumLauncherVersion")]
    pub minimum_launcher_version: Option<i32>,
}

/// Game launch arguments (modern format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameArguments {
    #[serde(default)]
    pub game: Vec<ArgumentValue>,
    #[serde(default)]
    pub jvm: Vec<ArgumentValue>,
}

/// Argument value (can be string or conditional)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    String(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValueInner,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValueInner {
    String(String),
    Array(Vec<String>),
}

/// Rule for conditional arguments/libraries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

/// Download information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Downloads {
    pub client: DownloadInfo,
    #[serde(default)]
    pub server: Option<DownloadInfo>,
    #[serde(default, rename = "client_mappings")]
    pub client_mappings: Option<DownloadInfo>,
    #[serde(default, rename = "server_mappings")]
    pub server_mappings: Option<DownloadInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// Library information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<ExtractRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<DownloadInfo>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

/// Asset index information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    #[serde(rename = "totalSize")]
    pub total_size: u64,
}

/// Java version requirement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u8,
}

impl VersionManifest {
    /// Fetch the version manifest from Mojang
    pub async fn fetch() -> Result<Self> {
        let response = reqwest::get(VERSION_MANIFEST_URL)
            .await
            .context("Failed to fetch version manifest")?;

        let manifest = response
            .json::<VersionManifest>()
            .await
            .context("Failed to parse version manifest")?;

        Ok(manifest)
    }

    /// Find a version by ID, type, or alias
    pub fn find_version(&self, version_id: &str) -> Option<&VersionInfo> {
        match version_id {
            "release" | "latest" => self.versions.iter().find(|v| v.id == self.latest.release),
            "snapshot" => self.versions.iter().find(|v| v.id == self.latest.snapshot),
            _ => self.versions.iter().find(|v| v.id == version_id),
        }
    }

    /// Filter versions by type
    pub fn filter_by_type(&self, version_type: &str) -> Vec<&VersionInfo> {
        self.versions
            .iter()
            .filter(|v| v.version_type == version_type)
            .collect()
    }

    /// Search versions by pattern
    pub fn search(&self, pattern: &str) -> Vec<&VersionInfo> {
        self.versions
            .iter()
            .filter(|v| v.id.contains(pattern))
            .collect()
    }
}

impl VersionMetadata {
    /// Fetch detailed metadata for a version
    pub async fn fetch(url: &str) -> Result<Self> {
        let response = reqwest::get(url)
            .await
            .context("Failed to fetch version metadata")?;

        let metadata = response
            .json::<VersionMetadata>()
            .await
            .context("Failed to parse version metadata")?;

        Ok(metadata)
    }

    /// Get required Java major version
    pub fn required_java_version(&self) -> u8 {
        self.java_version
            .as_ref()
            .map(|jv| jv.major_version)
            .unwrap_or(8) // Default to Java 8 for legacy versions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_manifest() {
        let manifest = VersionManifest::fetch().await;
        assert!(manifest.is_ok());

        let manifest = manifest.unwrap();
        assert!(!manifest.versions.is_empty());
        assert!(!manifest.latest.release.is_empty());
    }

    #[test]
    fn test_find_version() {
        let manifest = VersionManifest {
            latest: LatestVersions {
                release: "1.21.4".to_string(),
                snapshot: "24w51a".to_string(),
            },
            versions: vec![
                VersionInfo {
                    id: "1.21.4".to_string(),
                    version_type: "release".to_string(),
                    url: "".to_string(),
                    time: "".to_string(),
                    release_time: "".to_string(),
                    sha1: None,
                    compliance_level: None,
                },
                VersionInfo {
                    id: "1.21.3".to_string(),
                    version_type: "release".to_string(),
                    url: "".to_string(),
                    time: "".to_string(),
                    release_time: "".to_string(),
                    sha1: None,
                    compliance_level: None,
                },
            ],
        };

        assert!(manifest.find_version("release").is_some());
        assert_eq!(manifest.find_version("release").unwrap().id, "1.21.4");
        assert!(manifest.find_version("1.21.3").is_some());
        assert!(manifest.find_version("nonexistent").is_none());
    }
}
