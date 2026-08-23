// server.properties structured editor (v26.3-alpha.4).
//
// Vanilla properties files are line-oriented `key=value` pairs with `#`
// comments and blank lines. The server rewrites the whole file on shutdown,
// so MDL must preserve comment lines and ordering exactly 鈥?otherwise a
// user's annotated file loses its notes on the next restart.
//
// Editing model: load all raw lines; `get` scans parsed pairs; `set`
// replaces the first matching pair in place and drops duplicate later
// occurrences (vanilla itself keeps last-wins semantics, but a single
// entry is what users expect); unknown keys append at the end.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Default)]
pub struct PropertiesFile {
    /// Raw lines, comments and blanks included.
    lines: Vec<String>,
}

/// One parsed `key=value` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyPair {
    pub key: String,
    pub value: String,
}

impl PropertiesFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Ok(Self {
            lines: raw.lines().map(str::to_string).collect(),
        })
    }

    pub fn from_lines(lines: Vec<String>) -> Self {
        Self { lines }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut out = self.lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        std::fs::write(path, out)
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Iterate parsed `key=value` pairs (comments/blanks skipped).
    pub fn pairs(&self) -> Vec<PropertyPair> {
        self.lines.iter().filter_map(|l| parse_pair(l)).collect()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.pairs().into_iter().find(|p| p.key == key).map(|p| p.value)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    /// Set `key = value`: replace the first occurrence in place, drop any
    /// duplicates, or append at the end when the key is new.
    pub fn set(&mut self, key: &str, value: &str) {
        let mut replaced = false;
        self.lines.retain_mut(|line| {
            match parse_pair(line) {
                Some(p) if p.key == key && !replaced => {
                    *line = format!("{key}={value}");
                    replaced = true;
                    true
                }
                Some(p) if p.key == key => false, // drop duplicates
                _ => true,
            }
        });
        if !replaced {
            self.lines.push(format!("{key}={value}"));
        }
    }

    /// Remove every occurrence of `key`. Returns whether anything changed.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines
            .retain(|line| parse_pair(line).map(|p| p.key != key).unwrap_or(true));
        before != self.lines.len()
    }
}

/// Parse one line into a pair; None for comments, blanks and malformed rows.
fn parse_pair(line: &str) -> Option<PropertyPair> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with('!') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some(PropertyPair {
        key: k.trim().to_string(),
        value: v.trim().to_string(),
    })
}

/// Extract player names from a vanilla JSON list file (whitelist.json,
/// ops.json, banned-players.json). Entries are `{"uuid":..,"name":..}`;
/// malformed files yield an empty list rather than failing the command.
pub fn json_names(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE: &str = "# MOTD line\nmotd=hello\n\nserver-port=25565\n# trailing\nonline-mode=false\n";

    fn sample() -> PropertiesFile {
        PropertiesFile::from_lines(SAMPLE.lines().map(String::from).collect())
    }

    #[test]
    fn test_get_scans_pairs_and_skips_comments() {
        let p = sample();
        assert_eq!(p.get("motd"), Some("hello".to_string()));
        assert_eq!(p.get("server-port"), Some("25565".to_string()));
        assert_eq!(p.get_bool("online-mode"), Some(false));
        assert_eq!(p.get("missing"), None);
    }

    #[test]
    fn test_set_existing_preserves_comments_and_order() {
        let mut p = sample();
        p.set("motd", "changed");
        assert_eq!(p.get("motd"), Some("changed".to_string()));
        let joined = p.lines.join("\n");
        assert!(joined.contains("# MOTD line")); // comment survived
        assert!(joined.contains("# trailing"));
        // Order preserved: motd still before server-port.
        let motd_idx = joined.find("motd=").unwrap();
        let port_idx = joined.find("server-port=").unwrap();
        assert!(motd_idx < port_idx);
    }

    #[test]
    fn test_set_new_appends_and_dedups() {
        let mut p = sample();
        p.set("view-distance", "12");
        p.set("view-distance", "8"); // second set updates, not duplicates
        p.set("motd", "x"); // duplicate key collapse: only one motd= remains
        let pairs = p.pairs();
        assert_eq!(pairs.iter().filter(|q| q.key == "view-distance").count(), 1);
        assert_eq!(pairs.iter().filter(|q| q.key == "motd").count(), 1);
        assert_eq!(p.get("view-distance"), Some("8".to_string()));
    }

    #[test]
    fn test_remove_key() {
        let mut p = sample();
        assert!(p.remove("online-mode"));
        assert_eq!(p.get("online-mode"), None);
        assert!(!p.remove("never-existed"));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("server.properties");
        let mut p = sample();
        p.set("max-players", "20");
        p.save(&path).unwrap();
        let back = PropertiesFile::load(&path).unwrap();
        assert_eq!(back.get("max-players"), Some("20".to_string()));
        assert_eq!(back.get_bool("online-mode"), Some(false));
    }

    #[test]
    fn test_json_names_extracts_players() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("whitelist.json");
        std::fs::write(
            &path,
            r#"[{"uuid":"aaa","name":"Alice"},{"uuid":"bbb","name":"Bob"}]"#,
        )
        .unwrap();
        assert_eq!(json_names(&path), vec!["Alice", "Bob"]);
    }
}
