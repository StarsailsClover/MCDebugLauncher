// Recent-update digest shown at program startup (Alpha 8.1).
//
// The CHANGELOG is embedded at compile time (`include_str!`) so the digest
// works from any install location without extra files. At startup MDL prints
// the four most recent versions with their headline changes — a quick
// orientation for users returning after an update. The block is printed
// before command output, once per process, and never interferes with
// `--format json` consumers (skipped when JSON output is requested).

use crate::util::i18n;

/// CHANGELOG embedded at build time.
static CHANGELOG: &str = include_str!("../../CHANGELOG.md");

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
pub fn print_recent_updates(json_mode: bool) {
    if json_mode {
        return;
    }
    let digests = recent_versions(CHANGELOG, 4, 4);
    if digests.is_empty() {
        return;
    }
    println!("{}", i18n::t("Recent updates:", "最近更新:"));
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
