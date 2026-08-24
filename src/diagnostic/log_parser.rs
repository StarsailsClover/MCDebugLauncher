// Log parser (v26.2-alpha.2)
//
// Parses Minecraft and launcher log lines into structured entries with
// severity, source, category, exception type, and crash-marker detection.
// Based on the standalone mclog-analyzer tool by StarsailsClover, adapted
// for embedded use within MDL's diagnostic pipeline.
//
// Supported formats:
//   - Minecraft: [HH:MM:SS.mmm] [Thread/LEVEL] [Source]: Message
//   - PCL2:      HH:MM:SS.mmm L <thread> [Source] Message
//   - Stack frames: "at package.Class.method(File.java:123)"
//   - Caused by: ...
//   - Crash markers: "---- Minecraft Crash Report ----"

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Log severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
    Fatal = 4,
}

impl Severity {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "debug" | "trace" | "t" => Some(Severity::Debug),
            "info" | "information" | "i" => Some(Severity::Info),
            "warn" | "warning" | "w" => Some(Severity::Warn),
            "error" | "err" | "e" => Some(Severity::Error),
            "fatal" | "critical" | "f" | "c" => Some(Severity::Fatal),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Debug => "DEBUG",
            Severity::Info => "INFO",
            Severity::Warn => "WARN",
            Severity::Error => "ERROR",
            Severity::Fatal => "FATAL",
        }
    }
}

/// A parsed log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub line_number: u64,
    pub timestamp: Option<String>,
    pub severity: Severity,
    pub source: Option<String>,
    pub message: String,
    pub is_exception: bool,
    pub exception_type: Option<String>,
    pub is_crash_marker: bool,
    pub categories: Vec<String>,
}

/// Regex patterns for log parsing
struct Patterns {
    /// PCL2 format: 11:06:52.030 W <19 · MyImage PicLoader 105#> [MyImage] message
    pcl2_line: Regex,
    /// Minecraft format: [11:07:26.886] [main/WARN] [mixin/]: message
    mc_line: Regex,
    /// Exception line: java.lang.Exception: message
    exception_type: Regex,
    /// Caused by: ...
    caused_by: Regex,
    /// Crash report markers
    crash_marker: Regex,
}

static PATTERNS: OnceLock<Patterns> = OnceLock::new();

fn patterns() -> &'static Patterns {
    PATTERNS.get_or_init(|| Patterns {
        pcl2_line: Regex::new(
            r"^(\d{2}:\d{2}:\d{2}(?:\.\d+)?)\s+([A-Z])\s+<[^>]+>\s+\[([^\]]+)\]\s*(.*)$",
        ).unwrap(),
        mc_line: Regex::new(
            r"^\[(\d{2}:\d{2}:\d{2}(?:\.\d+)?)\]\s+\[([^/]+)/([A-Z]+)\]\s+\[([^\]]+)\]:\s*(.*)$",
        ).unwrap(),
        exception_type: Regex::new(
            r"^([\w.]+(?:Exception|Error|Throwable))(?::\s*(.*))?$",
        ).unwrap(),
        caused_by: Regex::new(
            r"^Caused by:\s+([\w.]+(?:Exception|Error|Throwable))(?::\s*(.*))?$",
        ).unwrap(),
        crash_marker: Regex::new(
            r"(?i)(----\s*minecraft crash report\s*----|preparing crash report|崩溃报告|游戏崩溃|Minecraft 已崩溃|Minecraft 尚未加载完成)",
        ).unwrap(),
    })
}

/// Error category detection
fn categorize(message: &str) -> Vec<String> {
    let mut categories = Vec::new();
    let msg = message.to_lowercase();

    if msg.contains("ssl") || msg.contains("handshake") || msg.contains("tls") {
        categories.push("network/ssl".to_string());
    }
    if msg.contains("timeout") || msg.contains("timed out") || msg.contains("连接超时") {
        categories.push("network/timeout".to_string());
    }
    if msg.contains("packet") || msg.contains("decode") || msg.contains("netty") || msg.contains("网络包") {
        categories.push("network/packet".to_string());
    }
    if msg.contains("classnotfound") || msg.contains("noclassdeffound") || msg.contains("找不到类") {
        categories.push("mod/class-missing".to_string());
    }
    if msg.contains("mixin") {
        categories.push("mod/mixin".to_string());
    }
    if msg.contains("model") || msg.contains("blockstate") || msg.contains("资源包") || msg.contains("pack_format") {
        categories.push("resource/model".to_string());
    }
    if msg.contains("outofmemory") || msg.contains("heap") || msg.contains("内存") {
        categories.push("performance/memory".to_string());
    }
    if msg.contains("stackoverflowerror") {
        categories.push("performance/stack-overflow".to_string());
    }
    if msg.contains("java version") || msg.contains("jni") || msg.contains("unsafe") || msg.contains("java 版本") {
        categories.push("environment/java-version".to_string());
    }
    if msg.contains("lwjgl") || msg.contains("opengl") || msg.contains("graphics") || msg.contains("显卡") {
        categories.push("environment/graphics".to_string());
    }
    if msg.contains("crash") || msg.contains("崩溃") {
        categories.push("crash".to_string());
    }
    if msg.contains("json") || msg.contains("parse") || msg.contains("解析") {
        categories.push("data/json-parse".to_string());
    }
    if msg.contains("filenotfound") || msg.contains("文件不存在") || msg.contains("missing") {
        categories.push("data/file-missing".to_string());
    }
    if msg.contains("nullpointer") || msg.contains("空指针") {
        categories.push("runtime/null-pointer".to_string());
    }
    if msg.contains("argumentexception") || msg.contains("illegalargument") {
        categories.push("runtime/illegal-argument".to_string());
    }

    if categories.is_empty() {
        categories.push("other".to_string());
    }

    categories
}

/// Parse a single log line
pub fn parse_line(line: &str, line_number: u64) -> LogEntry {
    let p = patterns();

    // Try PCL2 format first
    if let Some(caps) = p.pcl2_line.captures(line) {
        let timestamp = caps.get(1).map(|m| m.as_str().to_string());
        let severity_char = caps.get(2).map(|m| m.as_str()).unwrap_or("I");
        let source = caps.get(3).map(|m| m.as_str().to_string());
        let message = caps.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();

        let severity = match severity_char {
            "T" => Severity::Debug,
            "D" => Severity::Debug,
            "I" => Severity::Info,
            "W" => Severity::Warn,
            "E" => Severity::Error,
            "F" => Severity::Fatal,
            _ => Severity::Info,
        };

        let is_exception = p.exception_type.is_match(&message) || p.caused_by.is_match(&message);
        let exception_type = p.exception_type.captures(&message)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .or_else(|| p.caused_by.captures(&message)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string())));

        let is_crash_marker = p.crash_marker.is_match(line);
        let categories = categorize(&message);

        return LogEntry {
            line_number,
            timestamp,
            severity,
            source,
            message,
            is_exception,
            exception_type,
            is_crash_marker,
            categories,
        };
    }

    // Try Minecraft format
    if let Some(caps) = p.mc_line.captures(line) {
        let timestamp = caps.get(1).map(|m| m.as_str().to_string());
        let _thread = caps.get(2).map(|m| m.as_str().to_string());
        let severity_str = caps.get(3).map(|m| m.as_str()).unwrap_or("INFO");
        let source = caps.get(4).map(|m| m.as_str().to_string());
        let message = caps.get(5).map(|m| m.as_str().to_string()).unwrap_or_default();

        let severity = Severity::from_str(severity_str).unwrap_or(Severity::Info);

        let is_exception = p.exception_type.is_match(&message) || p.caused_by.is_match(&message);
        let exception_type = p.exception_type.captures(&message)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .or_else(|| p.caused_by.captures(&message)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string())));

        let is_crash_marker = p.crash_marker.is_match(line);
        let categories = categorize(&message);

        return LogEntry {
            line_number,
            timestamp,
            severity,
            source,
            message,
            is_exception,
            exception_type,
            is_crash_marker,
            categories,
        };
    }

    // Stack frame line
    if line.trim_start().starts_with("at ") && (line.contains(".java:") || line.contains("(")) {
        return LogEntry {
            line_number,
            timestamp: None,
            severity: Severity::Error,
            source: None,
            message: line.trim().to_string(),
            is_exception: true,
            exception_type: None,
            is_crash_marker: false,
            categories: vec!["stack-trace".to_string()],
        };
    }

    // Caused by line
    if let Some(caps) = p.caused_by.captures(line.trim()) {
        let exception_type = caps.get(1).map(|m| m.as_str().to_string());
        let message = line.trim().to_string();
        let categories = categorize(&message);
        return LogEntry {
            line_number,
            timestamp: None,
            severity: Severity::Error,
            source: None,
            message,
            is_exception: true,
            exception_type,
            is_crash_marker: false,
            categories,
        };
    }

    // Exception type line
    if let Some(caps) = p.exception_type.captures(line.trim()) {
        let exception_type = caps.get(1).map(|m| m.as_str().to_string());
        let message = line.trim().to_string();
        let categories = categorize(&message);
        return LogEntry {
            line_number,
            timestamp: None,
            severity: Severity::Error,
            source: None,
            message,
            is_exception: true,
            exception_type,
            is_crash_marker: false,
            categories,
        };
    }

    // Crash marker
    if p.crash_marker.is_match(line) {
        return LogEntry {
            line_number,
            timestamp: None,
            severity: Severity::Fatal,
            source: None,
            message: line.trim().to_string(),
            is_exception: false,
            exception_type: None,
            is_crash_marker: true,
            categories: vec!["crash".to_string()],
        };
    }

    // Default: treat as info
    let message = line.trim().to_string();
    let categories = if message.is_empty() {
        vec!["empty".to_string()]
    } else {
        categorize(&message)
    };
    LogEntry {
        line_number,
        timestamp: None,
        severity: Severity::Info,
        source: None,
        message,
        is_exception: false,
        exception_type: None,
        is_crash_marker: false,
        categories,
    }
}

/// Check if a line is a continuation of a previous log entry (stack trace, etc.)
pub fn is_continuation(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("at ") ||
    trimmed.starts_with("Caused by:") ||
    trimmed.starts_with("...") ||
    trimmed.starts_with('\t') ||
    (trimmed.starts_with(' ') && !trimmed.is_empty() && !trimmed.contains('[') && !trimmed.contains(':'))
}

/// Extract a stack trace from a block of text (e.g. a crash report or log
/// excerpt). Returns the top N frames in order.
pub fn extract_stack_trace(text: &str, max_frames: usize) -> Vec<String> {
    let mut stack_trace = Vec::new();
    let mut in_stack = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("at ") || trimmed.starts_with("Caused by:"))
            && (trimmed.contains(".java:") || trimmed.contains("("))
        {
            in_stack = true;
            stack_trace.push(trimmed.to_string());
            if stack_trace.len() >= max_frames {
                break;
            }
        } else if in_stack && !trimmed.is_empty() && !trimmed.starts_with('\t') && !trimmed.starts_with("at ") {
            break;
        }
    }

    stack_trace
}

/// Detect the crash type from a crash report or log excerpt.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl CrashType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CrashType::OutOfMemory => "OutOfMemory",
            CrashType::ModConflict => "ModConflict",
            CrashType::MissingDependency => "MissingDependency",
            CrashType::GraphicsDriver => "GraphicsDriver",
            CrashType::JavaVersion => "JavaVersion",
            CrashType::CorruptedFile => "CorruptedFile",
            CrashType::NetworkIssue => "NetworkIssue",
            CrashType::Unknown => "Unknown",
        }
    }
}

/// Detect the crash type from the full crash text.
pub fn detect_crash_type(crash_report: &str) -> CrashType {
    if crash_report.contains("OutOfMemoryError") || crash_report.contains("Java heap space") {
        CrashType::OutOfMemory
    } else if crash_report.contains("ClassNotFoundException")
        || crash_report.contains("NoClassDefFoundError")
        || crash_report.contains("NoSuchMethodError")
    {
        CrashType::MissingDependency
    } else if crash_report.contains("OpenGL")
        || crash_report.contains("GLFW")
        || crash_report.contains("graphics driver")
        || crash_report.contains("GLException")
    {
        CrashType::GraphicsDriver
    } else if crash_report.contains("UnsupportedClassVersionError")
        || crash_report.contains("java.lang.VerifyError")
    {
        CrashType::JavaVersion
    } else if crash_report.contains("ZipException")
        || crash_report.contains("corrupt")
        || crash_report.contains("invalid")
    {
        CrashType::CorruptedFile
    } else if crash_report.contains("IOException")
        || crash_report.contains("Connection")
        || crash_report.contains("SocketException")
    {
        CrashType::NetworkIssue
    } else if crash_report.contains("conflict")
        || crash_report.contains("incompatible")
        || crash_report.lines().filter(|l| l.contains(".jar") || l.contains("mod_id")).count() > 3
    {
        CrashType::ModConflict
    } else {
        CrashType::Unknown
    }
}

/// Extract mod names from a crash report or log excerpt.
pub fn extract_mods(text: &str, max: usize) -> Vec<String> {
    let mut mods = std::collections::HashSet::new();

    let known_mods = [
        ("be_quiet_negotiator", "be_quiet_negotiator"),
        ("sodium", "sodium"),
        ("iris", "iris"),
        ("lithium", "lithium"),
        ("mixin", "mixin"),
        ("neoforge", "neoforge"),
        ("forge", "forge"),
        ("fabric", "fabric"),
        ("optifine", "optifine"),
        ("create", "create"),
        ("arknights", "arknights_endfield"),
        ("crash_assistant", "crash_assistant"),
        ("yumi_mc", "yumi_mc_core"),
        ("replaymod", "replaymod"),
        ("reforgedplaymod", "reforgedplaymod"),
    ];

    let lower = text.to_lowercase();
    for (pattern, name) in known_mods {
        if lower.contains(pattern) {
            mods.insert(name.to_string());
        }
    }

    // Extract "from mod X" pattern
    if let Some(idx) = lower.find("from mod ") {
        let rest = &text[idx + 9..];
        let mod_name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
        if !mod_name.is_empty() {
            mods.insert(mod_name);
        }
    }

    mods.into_iter().take(max).collect()
}

/// Full analysis result for a log file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAnalysis {
    pub total_lines: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub crash_count: u64,
    pub entries: Vec<LogEntry>,
    pub top_errors: Vec<String>,
    pub detected_mods: Vec<String>,
    pub crash_type: Option<String>,
    pub stack_trace_top: Vec<String>,
    pub categories: std::collections::HashMap<String, u64>,
}

/// A streaming log parser that processes files line by line without loading
/// the full file into memory.
pub struct LogParser {
    max_lines: usize,
}

impl LogParser {
    pub fn new() -> Self {
        Self { max_lines: 5000 }
    }

    pub fn with_max_lines(max_lines: usize) -> Self {
        Self { max_lines }
    }

    /// Parse a log file from disk. Streams line by line, capping at
    /// `max_lines` entries to bound memory usage.
    pub fn parse(&self, log_path: &str) -> Result<Vec<LogEntry>> {
        let file = std::fs::File::open(log_path)?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead;

        let mut entries = Vec::new();
        let mut line_num: u64 = 0;

        for line in reader.lines() {
            let line = line?;
            line_num += 1;
            entries.push(parse_line(&line, line_num));
            if entries.len() >= self.max_lines {
                break;
            }
        }

        Ok(entries)
    }

    /// Parse a string of log content (for in-memory analysis).
    pub fn parse_content(&self, content: &str) -> Vec<LogEntry> {
        content
            .lines()
            .enumerate()
            .take(self.max_lines)
            .map(|(i, line)| parse_line(line, (i + 1) as u64))
            .collect()
    }

    /// Analyze parsed entries and produce a structured summary.
    pub fn analyze(&self, entries: &[LogEntry]) -> LogAnalysis {
        let mut error_count = 0u64;
        let mut warning_count = 0u64;
        let mut crash_count = 0u64;
        let mut categories: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let mut top_errors: Vec<String> = Vec::new();

        for entry in entries {
            match entry.severity {
                Severity::Error | Severity::Fatal => {
                    error_count += 1;
                    if top_errors.len() < 20 {
                        top_errors.push(entry.message.clone());
                    }
                }
                Severity::Warn => warning_count += 1,
                _ => {}
            }
            if entry.is_crash_marker {
                crash_count += 1;
            }
            for cat in &entry.categories {
                *categories.entry(cat.clone()).or_insert(0) += 1;
            }
        }

        // Build a combined text for crash detection + stack trace + mods.
        let combined: String = entries
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let crash_type = if crash_count > 0 || error_count > 0 {
            Some(detect_crash_type(&combined).as_str().to_string())
        } else {
            None
        };

        let stack_trace_top = extract_stack_trace(&combined, 10);
        let detected_mods = extract_mods(&combined, 15);

        LogAnalysis {
            total_lines: entries.len() as u64,
            error_count,
            warning_count,
            crash_count,
            entries: entries.to_vec(),
            top_errors,
            detected_mods,
            crash_type,
            stack_trace_top,
            categories,
        }
    }
}

impl Default for LogParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minecraft_format() {
        let line = "[11:07:26.886] [main/WARN] [mixin/]: Error loading class";
        let entry = parse_line(line, 1);
        assert_eq!(entry.timestamp.as_deref(), Some("11:07:26.886"));
        assert_eq!(entry.severity, Severity::Warn);
        assert_eq!(entry.source.as_deref(), Some("mixin/"));
        assert!(entry.message.contains("Error loading class"));
    }

    #[test]
    fn test_exception_line() {
        let line = "java.lang.NullPointerException: Cannot invoke method on null";
        let entry = parse_line(line, 1);
        assert!(entry.is_exception);
        assert_eq!(entry.exception_type.as_deref(), Some("java.lang.NullPointerException"));
    }

    #[test]
    fn test_stack_frame() {
        let line = "\tat java.base/java.lang.Thread.run(Thread.java:1516)";
        let entry = parse_line(line, 1);
        assert!(entry.is_exception);
        assert_eq!(entry.severity, Severity::Error);
    }

    #[test]
    fn test_crash_marker() {
        let line = "---- Minecraft Crash Report ----";
        let entry = parse_line(line, 1);
        assert!(entry.is_crash_marker);
        assert_eq!(entry.severity, Severity::Fatal);
    }

    #[test]
    fn test_category_network() {
        let line = "[12:00:00] [main/ERROR] [net/]: Connection timed out";
        let entry = parse_line(line, 1);
        assert!(entry.categories.contains(&"network/timeout".to_string()));
    }

    #[test]
    fn test_category_memory() {
        let line = "[12:00:00] [main/FATAL] [jvm/]: OutOfMemoryError: Java heap space";
        let entry = parse_line(line, 1);
        assert!(entry.categories.contains(&"performance/memory".to_string()));
    }

    #[test]
    fn test_extract_stack_trace() {
        let text = "Some preamble\nat com.example.Foo.bar(Foo.java:10)\nat com.example.Baz.qux(Baz.java:20)\nNot a stack frame";
        let trace = extract_stack_trace(text, 10);
        assert_eq!(trace.len(), 2);
        assert!(trace[0].contains("com.example.Foo.bar"));
    }

    #[test]
    fn test_detect_crash_type_oom() {
        let crash = "java.lang.OutOfMemoryError: Java heap space";
        assert!(matches!(detect_crash_type(crash), CrashType::OutOfMemory));
    }

    #[test]
    fn test_detect_crash_type_graphics() {
        let crash = "org.lwjgl.opengl.GLException: Cannot create context";
        assert!(matches!(detect_crash_type(crash), CrashType::GraphicsDriver));
    }

    #[test]
    fn test_extract_mods() {
        let text = "Error loading class from mod sodium\nSomething about neoforge";
        let mods = extract_mods(text, 10);
        assert!(mods.contains(&"sodium".to_string()));
        assert!(mods.contains(&"neoforge".to_string()));
    }

    #[test]
    fn test_parser_analyze() {
        let parser = LogParser::new();
        let entries = vec![
            parse_line("[12:00:00] [main/ERROR] [net/]: Connection timed out", 1),
            parse_line("[12:00:01] [main/WARN] [mixin/]: Error loading class", 2),
            parse_line("[12:00:02] [main/INFO] [game/]: Loaded world", 3),
        ];
        let analysis = parser.analyze(&entries);
        assert_eq!(analysis.error_count, 1);
        assert_eq!(analysis.warning_count, 1);
        assert!(analysis.categories.contains_key("network/timeout"));
    }

    #[test]
    fn test_parse_content() {
        let parser = LogParser::new();
        let content = "[12:00:00] [main/INFO] [game/]: Starting\n[12:00:01] [main/ERROR] [net/]: Failed\n";
        let entries = parser.parse_content(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].severity, Severity::Error);
    }

    #[test]
    fn test_is_continuation() {
        assert!(is_continuation("\tat com.example.Foo.bar(Foo.java:10)"));
        assert!(is_continuation("Caused by: java.lang.Exception"));
        assert!(is_continuation("... 10 more"));
        assert!(!is_continuation("[12:00:00] [main/INFO]: Normal line"));
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    /// Fuzz-inspired (v26.3-alpha.8): parse_line must never panic on
    /// arbitrary bytes-as-text, including control chars, huge lines and
    /// malformed bracket soup. Severity may be anything; contract is
    /// "returns an entry".
    #[test]
    fn test_parse_line_survives_garbage() {
        let cases = [
            "",
            "   ",
            "[[[[[[",
            "]]]]]]",
            "\u{0}\u{1}\u{2}",
            "[not a timestamp] [unclosed",
            "[12:99:88] [thread/] [] :",
            &"x".repeat(200_000),
            "at ",
            "Caused by:",
            "---- minecraft crash report ---- but mangled \u{7}",
        ];
        for (i, line) in cases.iter().enumerate() {
            let e = parse_line(line, i as u64);
            assert!(!e.message.is_empty() || line.trim().is_empty(), "case {i}");
        }
    }

    #[test]
    fn test_analyze_survives_adversarial_entries() {
        let parser = LogParser::new();
        let content = "[[[[\n\n\u{0}at \nCaused by: \n---- Minecraft Crash Report ----\n\
                       [t] [a/ERROR] [b/]: OutOfMemoryError x\u{1}";
        let entries = parser.parse_content(content);
        let analysis = parser.analyze(&entries);
        // Categories map always constructible; crash_type Some when errors exist.
        assert!(analysis.categories.len() >= 1);
    }
}
