// options.txt helpers for the game control module.
//
// The most important use is enforcing `pauseOnLostFocus:false` before launch
// so the game keeps running (and stays operable) while the user focuses
// other applications — a hard requirement for agent-driven operation.

use anyhow::{Context, Result};
use std::path::Path;

/// Read a `key:value` option from options.txt. Returns `None` when the file
/// or the key does not exist.
pub fn get_option(options_path: &Path, key: &str) -> Result<Option<String>> {
    if !options_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(options_path)
        .with_context(|| format!("Failed to read {}", options_path.display()))?;
    for line in content.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k == key {
                return Ok(Some(v.to_string()));
            }
        }
    }
    Ok(None)
}

/// Set a `key:value` option in options.txt, updating the line in place when
/// the key exists or appending it otherwise. Preserves all other lines.
pub fn set_option(options_path: &Path, key: &str, value: &str) -> Result<()> {
    let mut lines: Vec<String> = if options_path.exists() {
        let content = std::fs::read_to_string(options_path)
            .with_context(|| format!("Failed to read {}", options_path.display()))?;
        content.lines().map(String::from).collect()
    } else {
        Vec::new()
    };

    let mut found = false;
    for line in &mut lines {
        if let Some((k, _)) = line.split_once(':') {
            if k == key {
                *line = format!("{}:{}", key, value);
                found = true;
                break;
            }
        }
    }
    if !found {
        lines.push(format!("{}:{}", key, value));
    }

    let content = lines.join("\n") + "\n";
    std::fs::write(options_path, content)
        .with_context(|| format!("Failed to write {}", options_path.display()))?;
    Ok(())
}

/// Ensure `pauseOnLostFocus:false` in the instance's options.txt so the game
/// does not show the pause menu when the window loses focus. Returns `true`
/// when a change was made, `false` when the option was already correct.
///
/// Note: on first launch options.txt may not exist yet (Minecraft writes it on
/// first exit). In that case we create it with just this option; Minecraft
/// merges missing keys with defaults.
pub fn ensure_no_pause_on_lost_focus(instance_dir: &Path) -> Result<bool> {
    let options_path = instance_dir.join("options.txt");
    match get_option(&options_path, "pauseOnLostFocus")? {
        Some(v) if v == "false" => Ok(false),
        _ => {
            set_option(&options_path, "pauseOnLostFocus", "false")?;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_set_and_get_option() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("options.txt");
        fs::write(&path, "fov:0.5\nrenderDistance:12\n").unwrap();

        set_option(&path, "fov", "0.8").unwrap();
        assert_eq!(get_option(&path, "fov").unwrap().as_deref(), Some("0.8"));
        // untouched line preserved
        assert_eq!(
            get_option(&path, "renderDistance").unwrap().as_deref(),
            Some("12")
        );

        // new key appended
        set_option(&path, "newKey", "value").unwrap();
        assert_eq!(get_option(&path, "newKey").unwrap().as_deref(), Some("value"));
    }

    #[test]
    fn test_missing_option_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("options.txt");
        assert_eq!(get_option(&path, "anything").unwrap(), None);
        fs::write(&path, "fov:0.5\n").unwrap();
        assert_eq!(get_option(&path, "other").unwrap(), None);
    }

    #[test]
    fn test_ensure_pause_on_lost_focus() {
        let dir = tempfile::tempdir().unwrap();
        // no options.txt yet -> created and changed
        assert!(ensure_no_pause_on_lost_focus(dir.path()).unwrap());
        let path = dir.path().join("options.txt");
        assert_eq!(
            get_option(&path, "pauseOnLostFocus").unwrap().as_deref(),
            Some("false")
        );
        // second call -> already correct, no change reported
        assert!(!ensure_no_pause_on_lost_focus(dir.path()).unwrap());

        // explicit true -> flipped to false
        set_option(&path, "pauseOnLostFocus", "true").unwrap();
        assert!(ensure_no_pause_on_lost_focus(dir.path()).unwrap());
        assert_eq!(
            get_option(&path, "pauseOnLostFocus").unwrap().as_deref(),
            Some("false")
        );
    }
}
