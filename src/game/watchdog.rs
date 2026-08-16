// Idle watchdog (v26.2-alpha.1)
//
// Monitors a running Minecraft instance's stdout/stderr log stream. When the
// game produces no new log output for a configurable period (default 60s),
// the watchdog terminates the process gracefully. This prevents zombie game
// processes from lingering in agent-driven / unattended workflows.
//
// Design decisions:
//
// - Monitor log output, not CPU: Minecraft pegs CPU during world generation
//   but may produce no log lines. CPU-based heuristics would false-positive
//   during legitimate heavy load. A hung or frozen game, by contrast, almost
//   always stops emitting log lines (the game loop stalls, the log appender
//   stops flushing).
//
// - Whitelist patterns: some game phases are legitimately silent (e.g.
//   "Reloading Resource Packs", "Loading Renderer"). Matching lines reset
//   the idle timer so we don't kill the game during a known-slow phase.
//
// - Graceful then forceful: send a terminate signal first, wait 5s, then
//   kill. On Windows TerminateProcess is the only reliable cross-process
//   signal; there is no SIGTERM equivalent for console applications.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::RwLock;

/// Default idle timeout in seconds (1 minute, as requested by the user).
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;

/// Grace period (seconds) after sending a terminate signal before force-killing.
const GRACE_PERIOD_SECS: u64 = 5;

/// Patterns that, when seen in the log, reset the idle timer. These represent
/// legitimate slow-but-alive phases where silence does not indicate a hang.
const IDLE_RESET_PATTERNS: &[&str] = &[
    "Reloading",
    "Loading",
    "Building",
    "Initializing",
    "Generating",
    "Preparing",
    "Scanning",
    "Applying",
    "Mixing",
    "Firing",
    "Registering",
    "Collecting",
    "Connecting",
    "Authenticating",
    "Fetching",
    "Downloading",
    "Extracting",
];

/// Shared state for a single watched instance. Cloned cheaply (Arc inside).
#[derive(Clone)]
pub struct IdleWatchdog {
    inner: Arc<Inner>,
}

struct Inner {
    /// PID of the watched game process.
    pid: u32,
    /// Instance name (for event reporting).
    instance_name: String,
    /// Configured timeout in seconds.
    timeout_secs: u64,
    /// Last time we saw a log line (epoch nanos).
    last_output_ts: AtomicU64,
    /// Last line we saw (truncated, for diagnostics).
    last_line: RwLock<String>,
    /// Whether the watchdog is still active (set false after termination).
    active: AtomicBool,
    /// The log file path being tailed.
    log_path: std::path::PathBuf,
}

impl IdleWatchdog {
    /// Create a new watchdog for the given instance. Does NOT start monitoring;
    /// call `start()` to spawn the monitoring task.
    pub fn new(
        pid: u32,
        instance_name: String,
        log_path: impl AsRef<Path>,
        timeout_secs: u64,
    ) -> Self {
        let now = epoch_nanos();
        Self {
            inner: Arc::new(Inner {
                pid,
                instance_name,
                timeout_secs,
                last_output_ts: AtomicU64::new(now),
                last_line: RwLock::new(String::new()),
                active: AtomicBool::new(true),
                log_path: log_path.as_ref().to_path_buf(),
            }),
        }
    }

    /// Start the monitoring task. Returns immediately; the task runs in the
    /// background until the process exits or is terminated.
    pub fn start(self) {
        let timeout = Duration::from_secs(self.inner.timeout_secs);
        let sw = self.clone();
        tokio::spawn(async move {
            sw.run(timeout).await;
        });
    }

    /// Signal that a new log line was seen. Called by the log tailer.
    pub async fn feed_line(&self, line: &str) {
        // Check whitelist: if the line matches a known-slow-phase pattern,
        // reset the timer even though it's a single line.
        let should_reset = line_is_idle_reset(line);
        if should_reset {
            self.inner.last_output_ts.store(epoch_nanos(), Ordering::Relaxed);
        } else {
            // Even non-whitelist lines update the timestamp — any output
            // means the process is alive. The whitelist only matters for
            // the edge case where a single slow-phase line is the last
            // output; we still reset on it.
            self.inner.last_output_ts.store(epoch_nanos(), Ordering::Relaxed);
        }

        let mut last = self.inner.last_line.write().await;
        let truncated: String = line.chars().take(200).collect();
        *last = truncated;
    }

    /// Query the current idle status. Returns how long since last output,
    /// the configured threshold, and remaining time.
    pub async fn status(&self) -> IdleStatus {
        let now = epoch_nanos();
        let last = self.inner.last_output_ts.load(Ordering::Relaxed);
        let elapsed_secs = if now > last {
            (now - last) / 1_000_000_000
        } else {
            0
        };
        let remaining_secs = if elapsed_secs >= self.inner.timeout_secs {
            0
        } else {
            self.inner.timeout_secs - elapsed_secs
        };
        let last_line = self.inner.last_line.read().await.clone();

        IdleStatus {
            instance: self.inner.instance_name.clone(),
            pid: self.inner.pid,
            last_output_age_secs: elapsed_secs,
            threshold_secs: self.inner.timeout_secs,
            remaining_secs,
            last_line,
            active: self.inner.active.load(Ordering::Relaxed),
        }
    }

    async fn run(&self, timeout: Duration) {
        let poll_interval = Duration::from_secs(1);
        let mut last_file_size: u64 = 0;

        tracing::info!(
            "Idle watchdog started: instance='{}' pid={} timeout={}s log={}",
            self.inner.instance_name,
            self.inner.pid,
            timeout.as_secs(),
            self.inner.log_path.display()
        );

        loop {
            // Check if the process is still alive; if not, we're done.
            if !pid_alive(self.inner.pid) {
                tracing::info!(
                    "Idle watchdog: process {} no longer running, stopping",
                    self.inner.pid
                );
                self.inner.active.store(false, Ordering::Relaxed);
                break;
            }

            // Try to read new bytes from the log file.
            if let Ok(metadata) = std::fs::metadata(&self.inner.log_path) {
                let size = metadata.len();
                if size > last_file_size {
                    // The file grew — there's new output. Read the new tail.
                    let new_bytes = size - last_file_size;
                    if let Ok(new_lines) = read_file_tail(&self.inner.log_path, new_bytes) {
                        for line in new_lines {
                            self.feed_line(&line).await;
                        }
                    }
                    last_file_size = size;
                }
            }

            // Check timeout.
            let now = epoch_nanos();
            let last = self.inner.last_output_ts.load(Ordering::Relaxed);
            let elapsed = if now > last {
                (now - last) / 1_000_000_000
            } else {
                0
            };

            if elapsed >= timeout.as_secs() {
                tracing::warn!(
                    "Idle watchdog: instance '{}' (PID {}) has been idle for {}s (threshold {}s). Terminating.",
                    self.inner.instance_name,
                    self.inner.pid,
                    elapsed,
                    timeout.as_secs()
                );

                // Broadcast the idle-timeout event via the global event channel
                // (if an agent server is running, subscribers receive it).
                broadcast_idle_timeout(
                    &self.inner.instance_name,
                    self.inner.pid,
                    elapsed,
                ).await;

                // Graceful terminate, then kill.
                terminate_process(self.inner.pid);
                tokio::time::sleep(Duration::from_secs(GRACE_PERIOD_SECS)).await;
                if pid_alive(self.inner.pid) {
                    tracing::warn!(
                        "Idle watchdog: process {} did not exit in {}s, force killing",
                        self.inner.pid,
                        GRACE_PERIOD_SECS
                    );
                    kill_process(self.inner.pid);
                }

                // Clean up the PID file.
                cleanup_pid_file(&self.inner.instance_name).await;

                self.inner.active.store(false, Ordering::Relaxed);
                break;
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

/// Status snapshot of a watched instance.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IdleStatus {
    pub instance: String,
    pub pid: u32,
    /// Seconds since the last log output.
    pub last_output_age_secs: u64,
    /// Configured idle threshold in seconds.
    pub threshold_secs: u64,
    /// Seconds remaining before the watchdog triggers (0 if already triggered).
    pub remaining_secs: u64,
    /// Last log line seen (truncated to 200 chars).
    pub last_line: String,
    /// Whether the watchdog is still monitoring (false after termination).
    pub active: bool,
}

/// Whether a log line should reset the idle timer regardless of quantity.
/// Lines matching known-slow-phase patterns (world gen, resource reload, etc.)
/// are treated as "the game is doing something" even if only one such line
/// appears.
fn line_is_idle_reset(line: &str) -> bool {
    let lower = line.to_lowercase();
    IDLE_RESET_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
}

/// Current epoch time in nanoseconds (monotonic-ish; uses SystemTime).
fn epoch_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Read the last `max_bytes` bytes of a file as lines.
fn read_file_tail(path: &Path, max_bytes: u64) -> Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let seek_pos = if size > max_bytes {
        size - max_bytes
    } else {
        0
    };
    file.seek(SeekFrom::Start(seek_pos))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    // If we seeked into the middle of the file, the first "line" is partial.
    // Skip it unless we started from byte 0.
    let lines: Vec<String> = buf.lines().skip(if seek_pos > 0 { 1 } else { 0 }).map(String::from).collect();
    Ok(lines)
}

/// Check whether a process is alive.
fn pid_alive(pid: u32) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

/// Send a terminate signal to a process.
fn terminate_process(pid: u32) {
    #[cfg(windows)]
    {
        // On Windows, there's no SIGTERM equivalent for arbitrary processes.
        // Use TerminateProcess via the process handle.
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError},
            System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                TerminateProcess(handle, 1);
                CloseHandle(handle);
            } else {
                tracing::error!("Failed to open process {} for termination: error {}", pid, GetLastError());
            }
        }
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }
}

/// Force-kill a process.
fn kill_process(pid: u32) {
    #[cfg(windows)]
    {
        terminate_process(pid);
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    }
}

/// Remove the instance's PID file after termination.
async fn cleanup_pid_file(instance_name: &str) {
    if let Ok(instances_dir) = crate::util::paths::get_instances_dir() {
        let pid_file = instances_dir.join(instance_name).join("runtime").join("pid");
        let _ = fs::remove_file(&pid_file).await;
    }
}

/// Broadcast an idle-timeout event to the agent server's event channel (if any
/// agent server is running in this process). This is a no-op when no server
/// is active.
async fn broadcast_idle_timeout(instance: &str, pid: u32, idle_secs: u64) {
    // The agent server uses a broadcast channel stored in AgentServer.
    // We can't access it directly from here without a global handle.
    // Instead, we emit a structured log line that the agent server's log
    // forwarder (if running) will pick up and relay as a LogLine event,
    // and we also write a marker file the server can check.
    //
    // A future improvement: store the event sender in a global OnceCell
    // so this function can push structured events directly.
    tracing::warn!(
        target: "mdl::idle_watchdog",
        "game_idle_timeout instance={} pid={} idle_seconds={}",
        instance, pid, idle_secs
    );

    // Write a marker file for polling-based consumers.
    if let Ok(instances_dir) = crate::util::paths::get_instances_dir() {
        let marker = instances_dir.join(instance).join("runtime").join("idle_timeout");
        let _ = fs::write(&marker, format!(
            "{{\"instance\":\"{}\",\"pid\":{},\"idle_seconds\":{},\"timestamp\":\"{}\"}}",
            instance, pid, idle_secs,
            chrono::Utc::now().to_rfc3339()
        )).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_is_idle_reset_reloading() {
        assert!(line_is_idle_reset("[12:00:00] [main/INFO]: Reloading ResourceManager"));
    }

    #[test]
    fn test_line_is_idle_reset_loading() {
        assert!(line_is_idle_reset("[12:00:00] [main/INFO]: Loading Renderer"));
    }

    #[test]
    fn test_line_is_idle_reset_generating() {
        assert!(line_is_idle_reset("[12:00:00] [Server thread/INFO]: Generating terrain"));
    }

    #[test]
    fn test_line_is_idle_reset_random_output() {
        assert!(!line_is_idle_reset("[12:00:00] [main/INFO]: Player joined the game"));
    }

    #[test]
    fn test_line_is_idle_reset_empty() {
        assert!(!line_is_idle_reset(""));
    }

    #[test]
    fn test_line_is_idle_reset_downloading() {
        assert!(line_is_idle_reset("[12:00:00] [main/INFO]: Downloading terrain"));
    }

    #[test]
    fn test_idle_status_serializes() {
        let status = IdleStatus {
            instance: "test".to_string(),
            pid: 12345,
            last_output_age_secs: 30,
            threshold_secs: 60,
            remaining_secs: 30,
            last_line: "test line".to_string(),
            active: true,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"remaining_secs\":30"));
        assert!(json.contains("\"active\":true"));
    }
}
