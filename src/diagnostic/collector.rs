// Diagnostic collector - collects logs, crash reports, and system information

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use super::log_parser::{self, LogParser, Severity};

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub instance_name: String,
    pub timestamp: String,
    pub system_info: SystemInfo,
    pub logs: Vec<LogEntry>,
    pub crash_reports: Vec<CrashReport>,
    pub errors: Vec<ErrorEntry>,
    /// v26.2-alpha.2: structured log analysis (categories, crash type,
    /// detected mods, top stack frames) produced by the integrated
    /// mclog-analyzer parser.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<LogAnalysisSummary>,
}

/// Structured analysis summary embedded in the diagnostic report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAnalysisSummary {
    pub total_lines: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub crash_count: u64,
    pub detected_mods: Vec<String>,
    pub crash_type: Option<String>,
    pub stack_trace_top: Vec<String>,
    pub categories: std::collections::HashMap<String, u64>,
    pub top_errors: Vec<String>,
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
        let errors = self.extract_errors(&logs, &crash_reports).await?;

        // v26.2-alpha.2: run the integrated log analyzer on the collected
        // log entries to produce a structured summary (crash type, mods,
        // categories, stack traces). This replaces the previous TODO at
        // line 213 (stack_trace: None).
        let parser = LogParser::new();
        let parsed_entries: Vec<log_parser::LogEntry> = logs.iter().map(|l| {
            log_parser::LogEntry {
                line_number: 0,
                timestamp: Some(l.timestamp.clone()),
                severity: Severity::from_str(&l.level).unwrap_or(Severity::Info),
                source: Some(l.source.clone()),
                message: l.message.clone(),
                is_exception: false,
                exception_type: None,
                is_crash_marker: l.level == "FATAL",
                categories: Vec::new(),
            }
        }).collect();
        let analysis_result = parser.analyze(&parsed_entries);
        let analysis = Some(LogAnalysisSummary {
            total_lines: analysis_result.total_lines,
            error_count: analysis_result.error_count,
            warning_count: analysis_result.warning_count,
            crash_count: analysis_result.crash_count,
            detected_mods: analysis_result.detected_mods,
            crash_type: analysis_result.crash_type,
            stack_trace_top: analysis_result.stack_trace_top,
            categories: analysis_result.categories,
            top_errors: analysis_result.top_errors,
        });

        Ok(DiagnosticReport {
            instance_name: instance_name.to_string(),
            timestamp,
            system_info,
            logs,
            crash_reports,
            errors,
            analysis,
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
        // v26.2-alpha.2: use the integrated mclog-analyzer parser instead
        // of the previous inline regex. This gives us proper severity
        // detection, categorization, and crash-marker recognition.
        let parser = LogParser::new();
        let parsed = parser.parse_content(content);

        let entries: Vec<LogEntry> = parsed.into_iter().map(|p| LogEntry {
            timestamp: p.timestamp.unwrap_or_default(),
            level: p.severity.as_str().to_string(),
            source: p.source.unwrap_or_default(),
            message: p.message,
        }).collect();

        Ok(entries)
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

    async fn extract_errors(&self, logs: &[LogEntry], crash_reports: &[CrashReport]) -> Result<Vec<ErrorEntry>> {
        let mut errors = Vec::new();

        // v26.2-alpha.2: extract stack traces from crash reports using
        // the integrated log_parser. Previously this was a TODO (line 213).
        let crash_text: String = crash_reports.iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        for log in logs {
            if log.level == "ERROR" || log.level == "FATAL" {
                let stack_trace = log_parser::extract_stack_trace(&crash_text, 10);
                let stack_trace_str = if stack_trace.is_empty() {
                    None
                } else {
                    Some(stack_trace.join("\n"))
                };

                errors.push(ErrorEntry {
                    timestamp: log.timestamp.clone(),
                    error_type: self.classify_error(&log.message),
                    message: log.message.clone(),
                    stack_trace: stack_trace_str,
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
