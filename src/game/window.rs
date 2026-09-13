// Game window discovery for Windows.
//
// MDL tries to launch Minecraft with `--title "MDL: <instance>"`, but some
// modded/older clients ignore that argument entirely. The reliable anchor is
// therefore the game process PID, which MDL records in `runtime/pid` when it
// spawns the game. Discovery is PID-first with the title prefix as a fallback.

use anyhow::{Context, Result};
use serde::Serialize;
use windows_capture::window::Window;

/// Information about a discovered game window.
#[derive(Debug, Clone, Serialize)]
pub struct GameWindowInfo {
    /// Instance name (parsed from the title when possible, else from the
    /// caller-provided mapping).
    pub instance: String,
    pub title: String,
    pub pid: Option<u32>,
    pub width: i32,
    pub height: i32,
    /// Opaque window handle (HWND) as an integer, for diagnostics.
    pub handle: u64,
}

/// A resolved game window, owning the underlying capture-capable handle.
pub struct ResolvedWindow {
    pub info: GameWindowInfo,
    pub window: Window,
}

/// The title prefix MDL gives launched game windows (best-effort; may be
/// ignored by the game, in which case PID matching is used).
pub const TITLE_PREFIX: &str = "MDL: ";

/// Extract the instance name from a window title like "MDL: name [mods]".
fn instance_from_title(title: &str) -> Option<String> {
    let rest = title.strip_prefix(TITLE_PREFIX)?;
    let name = match rest.find('[') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn window_info(window: &Window, instance: String) -> Option<GameWindowInfo> {
    let title = window.title().ok()?;
    if !window.is_valid() {
        return None;
    }
    let pid = window.process_id().ok();
    let (width, height) = match window.rect() {
        Ok(r) => ((r.right - r.left) as i32, (r.bottom - r.top) as i32),
        Err(_) => (0, 0),
    };
    Some(GameWindowInfo {
        instance,
        title,
        pid,
        width,
        height,
        handle: window.as_raw_hwnd() as u64,
    })
}

/// Find the window owned by a specific process, matching PID first.
fn find_by_pid(pid: u32) -> Option<(Window, GameWindowInfo)> {
    let windows = Window::enumerate().ok()?;
    for window in windows {
        if window.process_id().ok() == Some(pid) {
            if !window.is_valid() {
                continue;
            }
            let title = window.title().unwrap_or_default();
            let instance = instance_from_title(&title).unwrap_or_else(|| format!("pid-{}", pid));
            if let Some(info) = window_info(&window, instance) {
                return Some((window, info));
            }
        }
    }
    None
}

/// Enumerate windows that look like MDL-launched Minecraft windows, matched
/// either by the title prefix or by the owning process being a known running
/// instance (passed as `known_pids`: instance name -> pid).
pub fn list_mdl_windows(known_pids: &[(String, u32)]) -> Vec<GameWindowInfo> {
    let mut results = Vec::new();
    let windows = match Window::enumerate() {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("Failed to enumerate windows: {:?}", e);
            return results;
        }
    };

    for window in windows {
        let title = match window.title() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !window.is_valid() {
            continue;
        }
        let window_pid = window.process_id().ok();

        // Match 1: title carries the MDL prefix.
        if let Some(instance) = instance_from_title(&title) {
            if let Some(info) = window_info(&window, instance) {
                results.push(info);
            }
            continue;
        }

        // Match 2: owning process is a known running MDL instance. This
        // covers games that ignored the --title argument.
        if let Some(pid) = window_pid {
            if let Some((instance, _)) = known_pids.iter().find(|(_, p)| *p == pid) {
                if let Some(info) = window_info(&window, instance.clone()) {
                    results.push(info);
                }
            }
        }
    }
    results
}

/// Best-effort visible window title for a PID (first match). Returns None
/// when the process owns no window or the title is empty.
/// v26.3-alpha.2: used by the OOM guard's pre-kill confirmation listing.
pub fn window_title_for_pid(pid: u32) -> Option<String> {
    let windows = Window::enumerate().ok()?;
    for window in windows {
        if !window.is_valid() {
            continue;
        }
        if window.process_id().ok() == Some(pid) {
            if let Ok(t) = window.title() {
                if !t.trim().is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Locate the game window for a specific instance.
///
/// Resolution order:
/// 1. Window owned by `pid` (the instance's recorded game process). This is
///    the reliable path — it works even when the game ignored `--title`.
/// 2. Any window whose title parses to the instance name.
pub fn find_for_instance(instance_name: &str, pid: Option<u32>) -> Result<ResolvedWindow> {
    // Path 1: PID match (robust regardless of window title) - but only when
    // the PID truly belongs to this instance. v26.5-alpha.7 (field bug
    // report): stale runtime/pid files + Windows PID reuse made Path 1
    // confidently attribute ANOTHER game's window (e.g. an error screen) to
    // this instance, and even rewrite the synthesized pid-N label with the
    // instance name. A failed identity check falls through to title match.
    if let Some(pid) = pid {
        if pid_belongs_to_instance(pid, instance_name) {
            if let Some((window, mut info)) = find_by_pid(pid) {
                // Prefer the real instance name over a synthesized pid-N label.
                if info.instance.starts_with("pid-") {
                    info.instance = instance_name.to_string();
                }
                return Ok(ResolvedWindow { info, window });
            }
        } else {
            tracing::debug!(
                "PID {} does not validate as instance '{}' (stale pid file or PID reuse); using title match",
                pid,
                instance_name
            );
        }
    }

    // Path 2: title match.
    let windows = Window::enumerate().context("Failed to enumerate windows")?;
    for window in windows {
        let title = match window.title() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let Some(instance) = instance_from_title(&title) else {
            continue;
        };
        if instance != instance_name || !window.is_valid() {
            continue;
        }
        if let Some(info) = window_info(&window, instance) {
            return Ok(ResolvedWindow { info, window });
        }
    }

    anyhow::bail!(
        "No game window found for instance '{}' (pid {}). Is it running and not minimized?",
        instance_name,
        pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
    )
}

/// Collect `(instance_name, pid)` pairs for all instances whose game process
/// is currently running, by reading each instance's `runtime/pid` file.
/// Used so window discovery can match games that ignored the `--title` flag.
///
/// v26.5-alpha.7 hardening (field bug report): the map used to trust stale
/// `runtime/pid` files. A crashed game leaves the file behind; Windows later
/// reuses that PID for another instance's java process, and the window
/// mapping then attributed that window - e.g. openlumin's error screen - to
/// the WRONG instance name. Two guards fix it:
///   1. the PID must belong to a live java/javaw process whose command line
///      carries the instance's gameDir marker (`instances\<name>`);
///   2. a PID claimed by several instances is ambiguous (PID reuse) and is
///      dropped from the map entirely.
pub fn collect_running_pids() -> Vec<(String, u32)> {
    let instances_dir = match crate::util::paths::get_instances_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&instances_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let pid_file = entry.path().join("runtime").join("pid");
        if let Ok(content) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                results.push((name, pid));
            }
        }
    }
    // Guard 2: drop pids claimed by multiple instances before identity
    // checks (keeps the ambiguity rule unit-testable in isolation).
    let results = drop_ambiguous_claims(results);
    // Guard 1: identity validation (alive + java + gameDir marker).
    results
        .into_iter()
        .filter(|(name, pid)| pid_belongs_to_instance(*pid, name))
        .collect()
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// Guard 2 (pure): a PID claimed by two or more instances cannot be
/// attributed to either - drop every claim on that PID.
fn drop_ambiguous_claims(claims: Vec<(String, u32)>) -> Vec<(String, u32)> {
    use std::collections::HashMap;
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for (_, pid) in &claims {
        *counts.entry(*pid).or_insert(0) += 1;
    }
    claims
        .into_iter()
        .filter(|(_, pid)| counts[pid] == 1)
        .collect()
}

/// Guard 1 (pure decision over the process facts): the process must be a
/// java/javaw game process whose command line carries this instance's
/// gameDir marker. Case-insensitive; accepts both path separators.
fn identity_matches(process_name: &str, cmd_line: &str, instance: &str) -> bool {
    let base = process_name.to_ascii_lowercase();
    let is_java = base == "java" || base == "javaw"
        || base == "java.exe" || base == "javaw.exe";
    if !is_java {
        return false;
    }
    let cmd = cmd_line.to_ascii_lowercase();
    let inst = instance.to_ascii_lowercase();
    // Path-segment match with a boundary check: the marker
    // `instances\<name>` may sit at end-of-args (gameDir is often the last
    // argument) or be followed by a path separator, space or quote - but
    // never by another name character, so "instance-a" cannot match
    // "instances\instance-ab".
    let win = format!("instances\\{}", inst);
    let nix = format!("instances/{}", inst);
    contains_path_segment(&cmd, &win) || contains_path_segment(&cmd, &nix)
}

/// True when `hay` contains `marker` and the character right after any
/// occurrence is an argument/path boundary (or end of string).
fn contains_path_segment(hay: &str, marker: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = hay[from..].find(marker) {
        let abs = from + pos + marker.len();
        match hay[abs..].chars().next() {
            None => return true,
            Some(c) if matches!(c, '\\' | '/' | ' ' | '"' | '\'') => return true,
            Some(_) => {
                from = abs;
                continue;
            }
        }
    }
    false
}

/// Guard 1 wrapper: resolve the live process facts for `pid` via sysinfo and
/// apply [`identity_matches`]. A dead PID or a process we cannot inspect
/// fails validation (never attribute on partial evidence).
fn pid_belongs_to_instance(pid: u32, instance: &str) -> bool {
    use sysinfo::{ProcessRefreshKind, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::everything());
    let Some(proc) = sys.process(sysinfo::Pid::from_u32(pid)) else {
        return false;
    };
    let name = proc.name().to_string();
    // sysinfo 0.30 exposes the command line as a list of tokens.
    let cmd_line = proc.cmd().join(" ");
    identity_matches(&name, &cmd_line, instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_from_title() {
        assert_eq!(
            instance_from_title("MDL: bc-test [Fabric API, ModX]").as_deref(),
            Some("bc-test")
        );
        assert_eq!(
            instance_from_title("MDL: vanilla-test").as_deref(),
            Some("vanilla-test")
        );
        assert_eq!(instance_from_title("Minecraft 1.21"), None);
        assert_eq!(instance_from_title("MDL: "), None);
        assert_eq!(instance_from_title(""), None);
    }

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    /// v26.5-alpha.7 (field bug report): stale pid files + Windows PID reuse
    /// misattributed windows across instances (openlumin's error screen was
    /// mapped to another instance's name).
    #[test]
    fn test_drop_ambiguous_claims() {
        let claims = vec![
            ("a".to_string(), 100),
            ("b".to_string(), 200),
            // 300 claimed twice (stale file + PID reuse): both dropped.
            ("c".to_string(), 300),
            ("d".to_string(), 300),
        ];
        let kept = drop_ambiguous_claims(claims);
        assert_eq!(kept, vec![
            ("a".to_string(), 100),
            ("b".to_string(), 200),
        ]);
    }

    #[test]
    fn test_identity_matches() {
        // A real game command line (from the field): gameDir marker present.
        let game_dir = r"C:\Users\x\AppData\Roaming\mdl\instances\openlumin-neoforge-26.2";
        let cmd = format!(r#"C:\Java\jdk-25\bin\java.exe -Xmx4G --gameDir {} --assetsDir x"#, game_dir);
        assert!(identity_matches("java.exe", &cmd, "openlumin-neoforge-26.2"));
        assert!(identity_matches("javaw", &cmd, "openlumin-neoforge-26.2"));

        // Case-insensitive + both separators.
        assert!(identity_matches(
            "JAVA.EXE",
            &cmd.to_uppercase().replace('\\', "/"),
            "OPENLUMIN-NEOFORGE-26.2"
        ));

        // A different instance's gameDir must NOT match.
        assert!(!identity_matches("java.exe", &cmd, "despotes-test"));

        // Prefix confusion guard: instance-a must not match instance-ab.
        let cmd_ab = cmd.replace("openlumin-neoforge-26.2", "instance-ab");
        assert!(!identity_matches("java.exe", &cmd_ab, "instance-a"));
        assert!(identity_matches("java.exe", &cmd_ab, "instance-ab"));

        // Non-java processes are never attributed, even with the marker.
        assert!(!identity_matches("werfault.exe", &cmd, "openlumin-neoforge-26.2"));

        // Missing gameDir marker (foreign java) is never attributed.
        assert!(!identity_matches("java.exe", "java -Xmx1G -jar app.jar", "a"));
    }
}
