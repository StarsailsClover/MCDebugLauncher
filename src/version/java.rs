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
