// Java runtime management

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info};

/// Java runtime information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub version: String,
    pub major_version: u8,
}

impl JavaRuntime {
    /// Detect Java runtime from PATH or common locations
    pub fn detect() -> Result<Self> {
        // Try java command first
        if let Ok(runtime) = Self::from_command("java") {
            return Ok(runtime);
        }

        // Try common installation paths
        #[cfg(target_os = "windows")]
        let common_paths = vec![
            r"C:\Program Files\Java",
            r"C:\Program Files (x86)\Java",
            r"C:\Program Files\Eclipse Adoptium",
        ];

        #[cfg(target_os = "linux")]
        let common_paths = vec![
            "/usr/lib/jvm",
            "/usr/java",
            "/opt/java",
        ];

        #[cfg(target_os = "macos")]
        let common_paths = vec![
            "/Library/Java/JavaVirtualMachines",
            "/System/Library/Java/JavaVirtualMachines",
        ];

        for base_path in common_paths {
            if let Ok(runtime) = Self::scan_directory(base_path) {
                return Ok(runtime);
            }
        }

        anyhow::bail!("No Java runtime found. Please install Java or specify path manually.")
    }

    /// Create JavaRuntime from a java command
    fn from_command(command: &str) -> Result<Self> {
        let output = Command::new(command)
            .arg("-version")
            .output()
            .context("Failed to execute java command")?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let version = Self::parse_version(&stderr)?;
        let major_version = Self::extract_major_version(&version)?;

        Ok(Self {
            path: PathBuf::from(command),
            version,
            major_version,
        })
    }

    /// Scan directory for Java installations
    fn scan_directory(path: &str) -> Result<Self> {
        let path = PathBuf::from(path);
        if !path.exists() {
            anyhow::bail!("Directory does not exist: {:?}", path);
        }

        // Look for bin/java or bin/java.exe
        #[cfg(target_os = "windows")]
        let java_bin = "bin\\java.exe";
        #[cfg(not(target_os = "windows"))]
        let java_bin = "bin/java";

        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let java_path = entry.path().join(java_bin);

            if java_path.exists() {
                if let Ok(runtime) = Self::from_path(&java_path) {
                    return Ok(runtime);
                }
            }
        }

        anyhow::bail!("No valid Java installation found in {:?}", path)
    }

    /// Create JavaRuntime from explicit path
    pub fn from_path(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("Java executable not found: {:?}", path);
        }

        let output = Command::new(path)
            .arg("-version")
            .output()
            .context("Failed to execute java")?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let version = Self::parse_version(&stderr)?;
        let major_version = Self::extract_major_version(&version)?;

        Ok(Self {
            path: path.clone(),
            version,
            major_version,
        })
    }

    /// Parse version string from java -version output
    fn parse_version(output: &str) -> Result<String> {
        for line in output.lines() {
            if line.contains("version") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        return Ok(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }
        anyhow::bail!("Could not parse Java version from output")
    }

    /// Extract major version number
    fn extract_major_version(version: &str) -> Result<u8> {
        // Handle both old (1.8.0_xxx) and new (17.0.1) formats
        let parts: Vec<&str> = version.split('.').collect();

        if parts.is_empty() {
            anyhow::bail!("Invalid version format: {}", version);
        }

        if parts[0] == "1" && parts.len() > 1 {
            // Old format: 1.8.0_xxx -> major version is 8
            parts[1].parse::<u8>()
                .context("Failed to parse major version")
        } else {
            // New format: 17.0.1 -> major version is 17
            parts[0].parse::<u8>()
                .context("Failed to parse major version")
        }
    }

    /// Check if this runtime meets minimum version requirement
    pub fn meets_requirement(&self, required_version: u8) -> bool {
        self.major_version >= required_version
    }

    /// Ensure a Java runtime satisfying `required_version` is available,
    /// auto-downloading one from Adoptium if the system has none suitable.
    ///
    /// Resolution order:
    ///   1. A previously auto-downloaded runtime in the java cache that meets
    ///      the requirement (fast path, no process spawn beyond a version check).
    ///   2. A system Java on PATH / common install locations that meets it.
    ///   3. Download the matching Temurin JRE from Adoptium into the java cache.
    ///
    /// This is what launch code should call instead of `detect()` so a missing
    /// or too-old Java no longer aborts the launch.
    pub async fn ensure_version(required_version: u8) -> Result<Self> {
        // 1. Cached auto-downloaded runtime for this major version.
        if let Ok(cached) = Self::from_cache(required_version) {
            if cached.meets_requirement(required_version) {
                debug!("Using cached Java {}", cached.major_version);
                return Ok(cached);
            }
        }

        // 2. System Java, if it satisfies the requirement.
        if let Ok(system) = Self::detect() {
            if system.meets_requirement(required_version) {
                debug!("Using system Java {}", system.major_version);
                return Ok(system);
            }
            info!(
                "System Java {} does not meet requirement (need {}), downloading...",
                system.major_version, required_version
            );
        } else {
            info!("No system Java found, downloading Java {}...", required_version);
        }

        // 3. Download from Adoptium.
        Self::download(required_version).await
    }

    /// Locate a previously auto-downloaded runtime for the given major version.
    fn from_cache(major_version: u8) -> Result<Self> {
        let base = crate::util::paths::get_java_cache_dir()?.join(major_version.to_string());
        if !base.exists() {
            anyhow::bail!("No cached Java {}", major_version);
        }
        let java_path = Self::find_java_binary(&base)?;
        Self::from_path(&java_path)
    }

    /// Recursively locate the `bin/java` (or `bin/java.exe`) executable within a
    /// freshly extracted JDK/JRE archive. Adoptium archives contain a single
    /// top-level version directory, so the binary sits one or two levels down.
    pub(crate) fn find_java_binary(root: &std::path::Path) -> Result<PathBuf> {
        #[cfg(target_os = "windows")]
        let rel = std::path::Path::new("bin").join("java.exe");
        #[cfg(not(target_os = "windows"))]
        let rel = std::path::Path::new("bin").join("java");

        // Direct hit: root/bin/java
        let direct = root.join(&rel);
        if direct.exists() {
            return Ok(direct);
        }

        // Otherwise search immediate subdirectories (root/<jdk-dir>/bin/java).
        // On macOS the layout is <jdk-dir>/Contents/Home/bin/java.
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let candidate = entry.path().join(&rel);
            if candidate.exists() {
                return Ok(candidate);
            }
            #[cfg(target_os = "macos")]
            {
                let mac = entry
                    .path()
                    .join("Contents")
                    .join("Home")
                    .join(&rel);
                if mac.exists() {
                    return Ok(mac);
                }
            }
        }

        anyhow::bail!("Java executable not found under {:?}", root)
    }

    /// Download and extract the Temurin JRE for `major_version` from Adoptium
    /// into the java cache, returning the resulting runtime.
    ///
    /// Streams the archive to a temp file on disk, then extracts from disk.
    /// The previous implementation used `response.bytes().await` which held
    /// the entire JRE archive (100-200MB) in memory.
    async fn download(major_version: u8) -> Result<Self> {
        let (os, arch, archive_ext) = Self::adoptium_platform()?;

        // Adoptium's "latest binary" redirect resolves to the current GA build
        // for the requested feature version, OS and architecture.
        let url = format!(
            "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jre/hotspot/normal/eclipse",
            major_version, os, arch
        );

        info!("Downloading Java {} from Adoptium ({}/{})", major_version, os, arch);

        let client = crate::util::http::create_download_client()?;
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to download Java from {}", url))?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to download Java {}: HTTP {} (no Temurin build for {}/{}?)",
                major_version,
                response.status(),
                os,
                arch
            );
        }

        // Stream the archive to a temp file on disk instead of buffering
        // the entire JRE in memory. A Temurin JRE zip is ~100-200MB; holding
        // it in RAM was a significant contributor to peak memory.
        let cache_dir = crate::util::paths::get_java_cache_dir()?;
        let target_dir = cache_dir.join(major_version.to_string());
        let archive_path = cache_dir.join(format!("java_{}_download.{}", major_version, archive_ext));

        // Clean any partial previous extraction.
        if target_dir.exists() {
            let _ = std::fs::remove_dir_all(&target_dir);
        }
        std::fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create Java cache dir {:?}", cache_dir))?;
        std::fs::create_dir_all(&target_dir)
            .with_context(|| format!("Failed to create Java dir {:?}", target_dir))?;

        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&archive_path).await
            .with_context(|| format!("Failed to create temp archive file {:?}", archive_path))?;
        let mut stream = response.bytes_stream();
        let mut total_written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read Java archive chunk")?;
            file.write_all(&chunk).await?;
            total_written += chunk.len() as u64;
        }
        file.sync_all().await?;
        drop(file);

        info!("Extracting Java runtime ({} bytes)...", total_written);
        let bytes = std::fs::read(&archive_path)
            .with_context(|| format!("Failed to re-read Java archive from disk {:?}", archive_path))?;

        match archive_ext {
            "zip" => Self::extract_zip(&bytes, &target_dir)?,
            "tar.gz" => Self::extract_tar_gz(&bytes, &target_dir)?,
            other => anyhow::bail!("Unsupported Java archive format: {}", other),
        }

        // Clean up the temp archive.
        let _ = std::fs::remove_file(&archive_path);

        let java_path = Self::find_java_binary(&target_dir)?;
        let runtime = Self::from_path(&java_path)?;
        info!("Java {} installed at {:?}", runtime.major_version, runtime.path);
        Ok(runtime)
    }

    /// Map the current platform to Adoptium's (os, arch, archive-extension)
    /// naming.
    fn adoptium_platform() -> Result<(&'static str, &'static str, &'static str)> {
        let os = match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "mac",
            other => anyhow::bail!("Unsupported OS for Java download: {}", other),
        };

        let arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "x86" => "x86",
            "aarch64" => "aarch64",
            other => anyhow::bail!("Unsupported architecture for Java download: {}", other),
        };

        let ext = if os == "windows" { "zip" } else { "tar.gz" };
        Ok((os, arch, ext))
    }

    pub(crate) fn extract_zip(bytes: &[u8], target_dir: &std::path::Path) -> Result<()> {
        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).context("Failed to read Java zip archive")?;
        archive
            .extract(target_dir)
            .context("Failed to extract Java zip archive")?;
        Ok(())
    }

    pub(crate) fn extract_tar_gz(bytes: &[u8], target_dir: &std::path::Path) -> Result<()> {
        let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
        let mut archive = tar::Archive::new(gz);
        archive
            .unpack(target_dir)
            .context("Failed to extract Java tar.gz archive")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_major_version() {
        assert_eq!(JavaRuntime::extract_major_version("1.8.0_391").unwrap(), 8);
        assert_eq!(JavaRuntime::extract_major_version("17.0.8").unwrap(), 17);
        assert_eq!(JavaRuntime::extract_major_version("21.0.1").unwrap(), 21);
    }

    #[test]
    fn test_detect_java() {
        // This test will only pass if Java is installed
        if let Ok(runtime) = JavaRuntime::detect() {
            println!("Detected Java: {:?}", runtime);
            assert!(runtime.major_version >= 8);
        }
    }
}
