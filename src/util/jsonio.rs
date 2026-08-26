// BOM-tolerant JSON reading helpers (v26.3-alpha.1).
//
// Consumes finding F3 of the v26.2 robustness assessment: serde_json rejects
// a UTF-8 BOM, and common Windows tooling (PowerShell 5.1 `Set-Content
// -Encoding UTF8`, legacy Notepad) writes one. A user who edits
// instance.json in such an editor gets the cryptic "expected value at line 1
// column 1" with no hint which file failed.
//
// Every helper here:
//   - strips a leading UTF-8 BOM (EF BB BF) before parsing,
//   - attaches the file path to parse errors so failures are actionable.
//
// UTF-16 BOMs are NOT handled: all MDL-written files are UTF-8 and no
// supported editor round-trips these configs as UTF-16 by default.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::path::Path;

/// Strip ALL leading UTF-8 BOMs. Returns the rest of the buffer.
///
/// v26.4-alpha.5: originally stripped at most one BOM. The cargo-fuzz
/// `jsonio_bom_parse` target found the gap within seconds (crash input
/// `EF BB BF EF BB BF`): stacked BOMs appear when buggy tooling prepends a
/// BOM to an already-BOM'd file, and MDL's tolerance promise should cover
/// them - top-level JSON values can never legitimately begin with those
/// bytes, so loop-stripping is unambiguous and strictly more forgiving.
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
    let mut rest = bytes;
    while let Some(stripped) = rest.strip_prefix(BOM) {
        rest = stripped;
    }
    rest
}

/// Parse a JSON file synchronously with BOM tolerance and path context.
pub fn parse_sync<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let raw = std::fs::read(path)?;
    let text = String::from_utf8(strip_bom(&raw).to_vec())?;
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse {label} {}", path.display()))
}

/// Async twin of [`parse_sync`] for call sites already on the tokio runtime
/// (instance configs are read from async contexts).
pub async fn parse_async<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let raw = tokio::fs::read(path).await?;
    let text = String::from_utf8(strip_bom(&raw).to_vec())?;
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse {label} {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Cfg {
        name: String,
    }

    const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

    #[test]
    fn test_strip_bom() {
        assert_eq!(strip_bom(BOM), b"");
        assert_eq!(strip_bom(&[0xEF, 0xBB, 0xBF, b'a']), b"a");
        // Non-BOM content must pass through untouched.
        assert_eq!(strip_bom(b"{"), b"{");
        assert_eq!(strip_bom(&[0xEE, 0xBB, 0xBF]), &[0xEE, 0xBB, 0xBF]);
    }

    #[test]
    fn test_strip_bom_handles_stacked_boms() {
        // Regression (v26.4-alpha.5): found by the cargo-fuzz
        // `jsonio_bom_parse` target within seconds of its first run.
        // Crash input: EF BB BF EF BB BF - a double BOM left parse_sync
        // with a leading U+FEFF and a cryptic serde error.
        let stacked = [BOM, BOM, br#"{"name":"x"}"#.as_slice()].concat();
        assert_eq!(strip_bom(&stacked), br#"{"name":"x"}"#.as_slice());
    }

    #[test]
    fn test_parse_sync_tolerates_stacked_boms() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("stacked.json");
        std::fs::write(&p, [BOM, BOM, br#"{"name":"y"}"#.as_slice()].concat()).unwrap();
        let cfg: Cfg = parse_sync(&p, "test config").unwrap();
        assert_eq!(cfg.name, "y");
    }

    #[test]
    fn test_parse_sync_tolerates_bom() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("cfg.json");
        std::fs::write(&p, [BOM, br#"{"name":"x"}"#.as_slice()].concat()).unwrap();
        let cfg: Cfg = parse_sync(&p, "test config").unwrap();
        assert_eq!(cfg.name, "x");
    }

    #[test]
    fn test_parse_sync_error_includes_path() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("broken.json");
        std::fs::write(&p, "{invalid").unwrap();
        let err = parse_sync::<Cfg>(&p, "test config").unwrap_err();
        assert!(err.to_string().contains("broken.json"), "{err}");
    }

    #[tokio::test]
    async fn test_parse_async_roundtrip() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("cfg.json");
        std::fs::write(&p, [BOM, br#"{"name":"y"}"#.as_slice()].concat()).unwrap();
        let cfg: Cfg = parse_async(&p, "test config").await.unwrap();
        assert_eq!(cfg.name, "y");
    }
}
