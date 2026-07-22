// Diagnostic collector - collects logs, crash reports, and system information

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub instance_name: String,
    pub timestamp: String,
    pub system_info: SystemInfo,
    pub logs: Vec<LogEntry>,
    pub crash_reports: Vec<CrashReport>,
    pub errors: Vec<ErrorEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub java_version: Option<String>,
    pub memory_total: Option<u64>,
    pub memory_available: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrashReport {
    pub file_name: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorEntry {
    pub timestamp: String,
    pub error_type: String,
    pub message: String,
    pub stack_trace: Option<String>,
}

pub struct DiagnosticCollector {
    instance_dir: PathBuf,
}

impl DiagnosticCollector {
    pub fn new(instance_dir: PathBuf) -> Self {
        Self { instance_dir }
    }

    pub async fn collect(&self, instance_name: &str) -> Result<DiagnosticReport> {
        tracing::info!("Collecting diagnostics for instance '{}'", instance_name);

        let timestamp = chrono::Utc::now().to_rfc3339();

        let system_info = self.collect_system_info().await?;
        let logs = self.collect_logs().await?;
        let crash_reports = self.collect_crash_reports().await?;
        let errors = self.extract_errors(&logs).await?;

        Ok(DiagnosticReport {
            instance_name: instance_name.to_string(),
            timestamp,
            system_info,
            logs,
            crash_reports,
            errors,
        })
    }

    async fn collect_system_info(&self) -> Result<SystemInfo> {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();

        // Try to get Java version
        let java_version = self.get_java_version().await.ok();

        // Try to get memory info
        let (memory_total, memory_available) = self.get_memory_info().await.unwrap_or((None, None));

        Ok(SystemInfo {
            os,
            arch,
            java_version,
            memory_total,
            memory_available,
        })
    }

    async fn get_java_version(&self) -> Result<String> {
        let output = tokio::process::Command::new("java")
            .arg("-version")
            .output()
            .await?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let first_line = stderr.lines().next().unwrap_or("Unknown");
        Ok(first_line.to_string())
    }

    async fn get_memory_info(&self) -> Result<(Option<u64>, Option<u64>)> {
        #[cfg(target_os = "windows")]
        {
            use sysinfo::System;
            let mut sys = System::new_all();
            sys.refresh_memory();
            Ok((Some(sys.total_memory()), Some(sys.available_memory())))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok((None, None))
        }
    }

    async fn collect_logs(&self) -> Result<Vec<LogEntry>> {
        let logs_dir = self.instance_dir.join("logs");
        let latest_log = logs_dir.join("latest.log");

        if !latest_log.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&latest_log).await?;
        let entries = self.parse_log_content(&content)?;

        Ok(entries)
    }

    fn parse_log_content(&self, content: &str) -> Result<Vec<LogEntry>> {
        let mut entries = Vec::new();

        for line in content.lines().take(1000) {  // Limit to last 1000 lines
            if let Some(entry) = self.parse_log_line(line) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    fn parse_log_line(&self, line: &str) -> Option<LogEntry> {
        // Parse Minecraft log format: [HH:MM:SS] [Thread/LEVEL]: Message
        let re = regex::Regex::new(r"^\[([^\]]+)\] \[([^/]+)/([^\]]+)\]: (.+)$").ok()?;

        let caps = re.captures(line)?;

        Some(LogEntry {
            timestamp: caps.get(1)?.as_str().to_string(),
            source: caps.get(2)?.as_str().to_string(),
            level: caps.get(3)?.as_str().to_string(),
            message: caps.get(4)?.as_str().to_string(),
        })
    }

    async fn collect_crash_reports(&self) -> Result<Vec<CrashReport>> {
        let crash_reports_dir = self.instance_dir.join("crash-reports");

        if !crash_reports_dir.exists() {
            return Ok(Vec::new());
        }

        let mut reports = Vec::new();
        let mut entries = fs::read_dir(&crash_reports_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("txt") {
                let content = fs::read_to_string(&path).await?;
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let timestamp = self.extract_timestamp_from_crash(&file_name);

                reports.push(CrashReport {
                    file_name,
                    content,
                    timestamp,
                });
            }
        }

        Ok(reports)
    }

    fn extract_timestamp_from_crash(&self, file_name: &str) -> String {
        // Extract timestamp from crash report filename: crash-2024-01-15_12-30-45-client.txt
        file_name
            .strip_prefix("crash-")
            .and_then(|s| s.split('-').take(3).collect::<Vec<_>>().join("-").into())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    async fn extract_errors(&self, logs: &[LogEntry]) -> Result<Vec<ErrorEntry>> {
        let mut errors = Vec::new();

        for log in logs {
            if log.level == "ERROR" || log.level == "FATAL" {
                errors.push(ErrorEntry {
                    timestamp: log.timestamp.clone(),
                    error_type: self.classify_error(&log.message),
                    message: log.message.clone(),
                    stack_trace: None,  // TODO: Extract stack traces
                });
            }
        }

        Ok(errors)
    }

    fn classify_error(&self, message: &str) -> String {
        if message.contains("OutOfMemoryError") {
            "OutOfMemory".to_string()
        } else if message.contains("ClassNotFoundException") || message.contains("NoClassDefFoundError") {
            "ClassNotFound".to_string()
        } else if message.contains("Connection") || message.contains("IOException") {
            "Network".to_string()
        } else if message.contains("OpenGL") || message.contains("LWJGL") {
            "Graphics".to_string()
        } else if message.contains("crash") {
            "Crash".to_string()
        } else {
            "General".to_string()
        }
    }

    pub async fn save_report(&self, report: &DiagnosticReport, output_path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(report)?;
        fs::write(output_path, json).await?;
        Ok(())
    }
}
