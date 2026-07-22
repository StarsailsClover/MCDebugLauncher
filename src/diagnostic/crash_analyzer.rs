// Crash analyzer - analyzes Minecraft crash reports and provides diagnostic suggestions

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashAnalysis {
    pub summary: String,
    pub crash_type: CrashType,
    pub likely_cause: String,
    pub suggestions: Vec<String>,
    pub mod_conflicts: Vec<String>,
    pub stack_trace_top: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CrashType {
    OutOfMemory,
    ModConflict,
    MissingDependency,
    GraphicsDriver,
    JavaVersion,
    CorruptedFile,
    NetworkIssue,
    Unknown,
}

pub struct CrashAnalyzer;

impl CrashAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, crash_report: &str) -> Result<CrashAnalysis> {
        tracing::info!("Analyzing crash report...");

        let crash_type = self.detect_crash_type(crash_report);
        let stack_trace = self.extract_stack_trace(crash_report);
        let mods = self.extract_mod_info(crash_report);
        let (likely_cause, suggestions) = self.generate_diagnosis(&crash_type, crash_report, &mods);

        let summary = self.generate_summary(&crash_type, &likely_cause);

        Ok(CrashAnalysis {
            summary,
            crash_type,
            likely_cause,
            suggestions,
            mod_conflicts: mods,
            stack_trace_top: stack_trace.into_iter().take(10).collect(),
        })
    }

    fn detect_crash_type(&self, crash_report: &str) -> CrashType {
        if crash_report.contains("OutOfMemoryError") || crash_report.contains("Java heap space") {
            CrashType::OutOfMemory
        } else if crash_report.contains("ClassNotFoundException")
            || crash_report.contains("NoClassDefFoundError")
            || crash_report.contains("NoSuchMethodError") {
            CrashType::MissingDependency
        } else if crash_report.contains("OpenGL")
            || crash_report.contains("GLFW")
            || crash_report.contains("graphics driver")
            || crash_report.contains("GLException") {
            CrashType::GraphicsDriver
        } else if crash_report.contains("UnsupportedClassVersionError")
            || crash_report.contains("java.lang.VerifyError") {
            CrashType::JavaVersion
        } else if crash_report.contains("ZipException")
            || crash_report.contains("corrupt")
            || crash_report.contains("invalid") {
            CrashType::CorruptedFile
        } else if crash_report.contains("IOException")
            || crash_report.contains("Connection")
            || crash_report.contains("SocketException") {
            CrashType::NetworkIssue
        } else if crash_report.contains("conflict")
            || crash_report.contains("incompatible")
            || self.has_multiple_mods(crash_report) {
            CrashType::ModConflict
        } else {
            CrashType::Unknown
        }
    }

    fn has_multiple_mods(&self, crash_report: &str) -> bool {
        let mod_count = crash_report.lines()
            .filter(|line| line.contains(".jar") || line.contains("mod_id"))
            .count();
        mod_count > 3
    }

    fn extract_stack_trace(&self, crash_report: &str) -> Vec<String> {
        let mut stack_trace = Vec::new();
        let mut in_stack = false;

        for line in crash_report.lines() {
            if line.contains("at ") && (line.contains("(") || line.contains(".java:")) {
                in_stack = true;
                stack_trace.push(line.trim().to_string());
            } else if in_stack && !line.trim().is_empty() && !line.starts_with('\t') {
                break;
            }
        }

        stack_trace
    }

    fn extract_mod_info(&self, crash_report: &str) -> Vec<String> {
        let mut mods = Vec::new();

        for line in crash_report.lines() {
            if line.contains(".jar") || line.contains("mod_id") {
                // Extract mod names from various formats
                if let Some(mod_name) = self.parse_mod_line(line) {
                    if !mods.contains(&mod_name) {
                        mods.push(mod_name);
                    }
                }
            }
        }

        mods
    }

    fn parse_mod_line(&self, line: &str) -> Option<String> {
        // Try to extract mod name from .jar file path
        if line.contains(".jar") {
            let parts: Vec<&str> = line.split('/').collect();
            if let Some(jar_name) = parts.last() {
                return Some(jar_name.trim().to_string());
            }
        }

        // Try to extract from mod_id format
        if line.contains("mod_id") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() > 1 {
                return Some(parts[1].trim().trim_matches('"').to_string());
            }
        }

        None
    }

    fn generate_diagnosis(&self, crash_type: &CrashType, crash_report: &str, mods: &[String]) -> (String, Vec<String>) {
        match crash_type {
            CrashType::OutOfMemory => (
                "Insufficient memory allocated to Minecraft".to_string(),
                vec![
                    "Increase JVM heap size using -Xmx argument (e.g., -Xmx4G)".to_string(),
                    "Close other applications to free up system memory".to_string(),
                    "Remove unused mods or resource packs".to_string(),
                    "Use OptiFine or Sodium for better memory management".to_string(),
                ],
            ),
            CrashType::MissingDependency => (
                "Missing required classes or libraries".to_string(),
                vec![
                    "Install required mod loader version".to_string(),
                    "Check mod dependencies and install missing mods".to_string(),
                    "Update mods to compatible versions".to_string(),
                    "Verify Minecraft installation integrity".to_string(),
                ],
            ),
            CrashType::GraphicsDriver => (
                "Graphics driver or OpenGL issue".to_string(),
                vec![
                    "Update graphics drivers to the latest version".to_string(),
                    "Try running with -Dorg.lwjgl.opengl.Display.allowSoftwareOpenGL=true".to_string(),
                    "Check if your GPU supports the required OpenGL version".to_string(),
                    "Disable shaders or advanced graphics features".to_string(),
                ],
            ),
            CrashType::JavaVersion => (
                "Java version incompatibility".to_string(),
                vec![
                    format!("Install Java 17 or higher for Minecraft 1.18+"),
                    "Check Java version with 'java -version'".to_string(),
                    "Update JAVA_HOME environment variable".to_string(),
                ],
            ),
            CrashType::CorruptedFile => (
                "Corrupted game files or mods".to_string(),
                vec![
                    "Re-download corrupted mod files".to_string(),
                    "Verify Minecraft client integrity".to_string(),
                    "Clear cache and temporary files".to_string(),
                    "Reinstall problematic mods".to_string(),
                ],
            ),
            CrashType::NetworkIssue => (
                "Network connectivity problem".to_string(),
                vec![
                    "Check internet connection".to_string(),
                    "Verify server availability".to_string(),
                    "Check firewall settings".to_string(),
                    "Try offline mode if applicable".to_string(),
                ],
            ),
            CrashType::ModConflict => {
                let conflicting_mods = if mods.len() > 1 {
                    format!("Possible conflict between: {}", mods.join(", "))
                } else {
                    "Multiple mods may be conflicting".to_string()
                };

                (
                    conflicting_mods,
                    vec![
                        "Remove mods one by one to identify the conflict".to_string(),
                        "Check mod compatibility matrix".to_string(),
                        "Update all mods to latest compatible versions".to_string(),
                        "Check mod authors' documentation for known conflicts".to_string(),
                    ],
                )
            }
            CrashType::Unknown => {
                let suggestions = self.extract_generic_suggestions(crash_report);
                (
                    "Unable to determine specific crash cause".to_string(),
                    suggestions,
                )
            }
        }
    }

    fn extract_generic_suggestions(&self, crash_report: &str) -> Vec<String> {
        let mut suggestions = vec![
            "Check the full crash report for more details".to_string(),
            "Search for the error message online".to_string(),
            "Try running with minimal mods".to_string(),
        ];

        // Add specific suggestions based on error patterns
        if crash_report.contains("NullPointerException") {
            suggestions.push("A mod is trying to access null data - report to mod author".to_string());
        }

        if crash_report.contains("StackOverflowError") {
            suggestions.push("Possible infinite loop in mod code".to_string());
        }

        suggestions
    }

    fn generate_summary(&self, crash_type: &CrashType, likely_cause: &str) -> String {
        format!("{:?}: {}", crash_type, likely_cause)
    }
}
