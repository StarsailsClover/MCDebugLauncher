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
    /// v26.3-alpha.3: last recorded idle-watchdog termination for this
    /// instance, if any (from runtime/idle_timeout).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout_event: Option<IdleTimeoutEvent>,
    /// v26.3-alpha.3: most recent launch metrics (spawn/ready timings,
    /// download volume, cache hits) for crash correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_launch_metrics: Option<crate::util::metrics::LaunchMetrics>,
}

/// One recorded idle-watchdog termination (marker file written by
/// game::watchdog when it kills an unresponsive game).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleTimeoutEvent {
    pub instance: String,
    pub pid: u32,
    pub idle_seconds: u64,
    pub timestamp: String,
}

/// Read and parse the idle-timeout marker from an instance's runtime dir.
/// Returns None when absent or unparseable.
pub fn read_idle_timeout_marker(instance_dir: &Path) -> Option<IdleTimeoutEvent> {
    let path = instance_dir.join("runtime").join("idle_timeout");
    crate::util::jsonio::parse_sync::<IdleTimeoutEvent>(&path, "idle timeout event").ok()
}

/// Human-readable correlation notes between observed failures and the last
/// recorded launch (v26.3-alpha.3). Pure function so heuristics are unit
/// testable; rendering stays in the CLI layer.
///
/// Heuristics (deliberately conservative — each states its evidence):
/// 1. Idle watchdog fired → the game stopped producing output and was killed
///    (hang/freeze class, not a JVM crash).
/// 2. Crash reports exist but the last launch never reached ready → likely
///    crashed during startup/loading.
/// 3. Crash on the same calendar day as the last launch → temporal link.
pub fn build_correlation_notes(
    has_crashes: bool,
    idle_event: Option<&IdleTimeoutEvent>,
    launch: Option<&crate::util::metrics::LaunchMetrics>,
) -> Vec<String> {
    let mut notes = Vec::new();

    if let Some(ev) = idle_event {
        notes.push(format!(
            "The game was terminated by the idle watchdog after {}s of silence \
             at {} — this is a hang/freeze signature, not a JVM crash",
            ev.idle_seconds, ev.timestamp
        ));
    }

    if has_crashes {
        if let Some(m) = launch {
            if m.ready_secs.is_none() {
                notes.push(
                    "The last recorded launch never reached the ready state — \
                     the crash likely happened during startup or world load"
                        .to_string(),
                );
            }
            // Same-calendar-day link: crash report timestamps carry only the
            // date (from the filename), so compare date prefixes.
            let launch_date = &m.timestamp[..10.min(m.timestamp.len())];
            if !launch_date.is_empty() {
                notes.push(format!(
                    "Crash report(s) dated {} may correspond to the last launch at {}",
                    launch_date, m.timestamp
                ));
            }
        }
    }

    notes
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
            idle_timeout_event: read_idle_timeout_marker(&self.instance_dir),
            last_launch_metrics: crate::util::metrics::load_latest(&self.instance_dir),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::metrics::LaunchMetrics;
    use tempfile::TempDir;

    fn sample_event() -> IdleTimeoutEvent {
        IdleTimeoutEvent {
            instance: "t".into(),
            pid: 42,
            idle_seconds: 60,
            timestamp: "2026-08-23T00:00:00Z".into(),
        }
    }

    #[test]
    fn test_read_idle_marker_roundtrip() {
        let dir = TempDir::new().unwrap();
        let rt = dir.path().join("runtime");
        std::fs::create_dir_all(&rt).unwrap();
        std::fs::write(
            rt.join("idle_timeout"),
            r#"{"instance":"t","pid":42,"idle_seconds":60,"timestamp":"2026-08-23T00:00:00Z"}"#,
        )
        .unwrap();
        let ev = read_idle_timeout_marker(dir.path()).unwrap();
        assert_eq!(ev.pid, 42);
        assert_eq!(ev.idle_seconds, 60);
    }

    #[test]
    fn test_read_idle_marker_missing() {
        let dir = TempDir::new().unwrap();
        assert!(read_idle_timeout_marker(dir.path()).is_none());
    }

    #[test]
    fn test_correlation_idle_only() {
        let ev = sample_event();
        let notes = build_correlation_notes(false, Some(&ev), None);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("idle watchdog"));
    }

    #[test]
    fn test_correlation_crash_without_ready() {
        let m = LaunchMetrics {
            timestamp: "2026-08-23T01:00:00Z".into(),
            instance: "t".into(),
            pid: 1,
            detached: true,
            spawn_secs: 2.0,
            ready_secs: None,
            download_bytes: 0,
            downloads: 0,
            cache_hits: 0,
        };
        let notes = build_correlation_notes(true, None, Some(&m));
        assert!(notes.iter().any(|n| n.contains("never reached the ready state")));
        assert!(notes.iter().any(|n| n.contains("2026-08-23")));
    }

    #[test]
    fn test_correlation_healthy_launch_no_notes() {
        // Ready launch, no crashes -> no speculative notes.
        let m = LaunchMetrics {
            timestamp: "2026-08-23T01:00:00Z".into(),
            instance: "t".into(),
            pid: 1,
            detached: true,
            spawn_secs: 2.0,
            ready_secs: Some(20.0),
            download_bytes: 0,
            downloads: 0,
            cache_hits: 0,
        };
        assert!(build_correlation_notes(false, None, Some(&m)).is_empty());
    }
}
