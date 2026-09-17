// Platform-neutral game-process identity by command line
// (v26.6-alpha.3).
//
// Extracted from game::window (which is cfg(windows)) because the identity
// logic is NOT Windows-specific: every MDL launch passes
// `--gameDir <data>/instances/<name>` on all platforms, so a live java
// process can be attributed to an instance from its command line alone
// (sysinfo works everywhere). Two consumers:
//   - game::window (Windows): validates pid-file claims before attributing
//     windows to instances (stale pid files + Windows PID reuse made
//     openlumin's error screen appear under another instance's name)
//   - game::lifecycle (all platforms): resolves the live pid for
//     `mdl kill` when runtime/pid is missing or stale
//
// The decision functions are pure and unit-tested; the sysinfo wrappers
// fail closed (never attribute on partial evidence).

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

use std::collections::HashMap;

/// Guard 2 (pure): a PID claimed by two or more instances cannot be
/// attributed to either - drop every claim on that PID.
pub fn drop_ambiguous_claims(claims: Vec<(String, u32)>) -> Vec<(String, u32)> {
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
pub fn identity_matches(process_name: &str, cmd_line: &str, instance: &str) -> bool {
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
pub fn pid_belongs_to_instance(pid: u32, instance: &str) -> bool {
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

/// Find the live game PID for an instance by command-line attribution.
/// Used as a fallback when `runtime/pid` is missing or stale - the field
/// case that made `mdl kill` report "not running" while the game was alive
/// (a crashed/replaced process left the pid file pointing at nothing).
/// Returns None when no java process carries this instance's gameDir
/// marker, or when several do (ambiguous: never guess).
pub fn find_pid_by_instance_identity(instance: &str) -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::everything());
    let mut hits: Vec<u32> = Vec::new();
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string();
        let cmd_line = proc.cmd().join(" ");
        if identity_matches(&name, &cmd_line, instance) {
            hits.push(pid.as_u32());
        }
    }
    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    /// Field regression (v26.5-alpha.7): stale pid files + Windows PID reuse
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
