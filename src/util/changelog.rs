// Recent-update digest shown at program startup (Alpha 9).
//
// The CHANGELOG is embedded at compile time (`include_str!`) so the digest
// works from any install location without extra files. 
//
// Alpha 9 behavior: Only show the changelog when the version has been updated
// since the last run. This dramatically reduces startup noise for daily users.

use crate::util::i18n;
use std::fs;
use std::path::PathBuf;

/// CHANGELOG embedded at build time.
pub static CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// One parsed version section from the CHANGELOG.
#[derive(Debug, Clone)]
pub struct VersionDigest {
    pub version: String,
    pub date: String,
    pub highlights: Vec<String>,
}

/// Parse CHANGELOG sections (`## [x.y.z] - date`) and return the `n` most
/// recent, each with up to `max_lines` headline bullets.
pub fn recent_versions(text: &str, n: usize, max_lines: usize) -> Vec<VersionDigest> {
    let mut out: Vec<VersionDigest> = Vec::new();
    let mut current: Option<VersionDigest> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## [") {
            // Start of a new version section: "## [1.2.3] - 2026-01-01"
            if let Some(digest) = current.take() {
                out.push(digest);
                if out.len() >= n * 2 {
                    break; // safety cap while parsing
                }
            }
            let (version, date) = match rest.split_once("] - ") {
                Some((v, d)) => (v.trim().to_string(), d.trim().to_string()),
                None => (rest.trim_end_matches(']').to_string(), String::new()),
            };
            current = Some(VersionDigest {
                version,
                date,
                highlights: Vec::new(),
            });
        } else if let Some(digest) = current.as_mut() {
            // Collect bullet points, skip sub-headings without content.
            if let Some(bullet) = trimmed.strip_prefix("- ") {
                let text = strip_md_bold(bullet.trim());
                if !text.is_empty() && digest.highlights.len() < max_lines {
                    digest.highlights.push(text);
                }
            }
        }
    }
    if let Some(digest) = current.take() {
        out.push(digest);
    }
    out.truncate(n);
    out
}

/// Remove Markdown bold markers (**) so terminal output stays clean.
fn strip_md_bold(s: &str) -> String {
    s.replace("**", "")
}

/// Print the recent-update digest (4 versions). Silent when JSON output is
/// requested so machine-readable output stays parseable.
/// 
/// Alpha 9: Only shows changelog if the version has been updated since last run.
pub fn print_recent_updates(json_mode: bool) {
    if json_mode {
        return;
    }

    let current_version = env!("CARGO_PKG_VERSION");
    
    // Check if we should show the changelog
    if !has_version_changed(current_version) {
        return;
    }

    // Show the full changelog
    let digests = recent_versions(CHANGELOG, 4, 4);
    if digests.is_empty() {
        return;
    }
    println!("{}", i18n::t("Recent updates:", "最近更新："));
    for d in &digests {
        println!(
            "  {} {}{}",
            d.version,
            if d.date.is_empty() { String::new() } else { format!("({}) ", d.date) },
            ""
        );
        for h in &d.highlights {
            let line = if h.chars().count() > 96 {
                format!("{}…", h.chars().take(93).collect::<String>())
            } else {
                h.clone()
            };
            println!("    - {}", line);
        }
    }
    println!();

    // Update the last version file
    let _ = update_last_version(current_version);
}

/// Get the path to the last_version tracking file
fn get_last_version_path() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    Some(data_dir.join("mdl").join("last_version"))
}

/// Check if the version has changed since the last run
fn has_version_changed(current_version: &str) -> bool {
    let path = match get_last_version_path() {
        Some(p) => p,
        None => return true, // Show changelog if we can't determine
    };

    if !path.exists() {
        return true; // First run or file deleted
    }

    match fs::read_to_string(&path) {
        Ok(last_version) => {
            let last_version = last_version.trim();
            last_version != current_version
        }
        Err(_) => true, // Show changelog if we can't read the file
    }
}

/// Update the last_version tracking file
fn update_last_version(current_version: &str) -> std::io::Result<()> {
    let path = match get_last_version_path() {
        Some(p) => p,
        None => return Ok(()), // Silently fail if we can't determine path
    };

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, current_version)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Changelog

## [2.0.0] - 2026-02-02

### Added
- **Big feature** — does a lot of things
- Another bullet

## [1.9.0] - 2026-01-01

### Added
- Old feature
"#;

    #[test]
    fn test_recent_versions() {
        let d = recent_versions(SAMPLE, 2, 4);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].version, "2.0.0");
        assert_eq!(d[0].date, "2026-02-02");
        assert_eq!(d[0].highlights.len(), 2);
        assert_eq!(d[0].highlights[0], "Big feature — does a lot of things");
        assert_eq!(d[1].version, "1.9.0");
    }

    #[test]
    fn test_truncate_limit() {
        let d = recent_versions(SAMPLE, 1, 1);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].highlights.len(), 1);
    }

    #[test]
    fn test_embedded_changelog_parses() {
        let d = recent_versions(CHANGELOG, 4, 4);
        assert!(!d.is_empty(), "embedded CHANGELOG must parse");
        assert!(d[0].version.contains("alpha") || d[0].version.starts_with("2"));
    }
}
