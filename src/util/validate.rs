// Name validation for instances and servers (v26.3-alpha.1).
//
// Consumes finding F1 of the v26.2 robustness assessment: names were only
// checked for directory existence, letting reserved Windows device names
// ("CON") create broken instances and invalid characters surface as raw OS
// errors (code 123/3).
//
// Rules enforced here apply uniformly to instance create/rename/clone/import
// targets and managed-server names:
//   - 1..=64 bytes
//   - forbidden anywhere: / \ : * ? " < > | and C0 control chars
//   - no trailing dot or space (Windows strips them silently)
//   - device-name stem blacklist, case-insensitive, applied to the part
//     before the first dot (Windows reserves CON.txt the same as CON):
//     CON PRN AUX NUL COM0-9 LPT0-9

use anyhow::{bail, Result};

const RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", //
    "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", //
    "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub const MAX_NAME_LEN: usize = 64;

/// Validate a user-supplied instance/server name. Unicode names are allowed;
/// only characters that are illegal or dangerous on Windows filesystems (or
/// that enable path traversal) are rejected.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Name must not be empty");
    }
    if name.len() > MAX_NAME_LEN {
        bail!("Name is {} bytes long; max is {}", name.len(), MAX_NAME_LEN);
    }
    if name == "." || name == ".." {
        bail!("Name '{name}' is reserved");
    }
    let forbidden = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    for ch in name.chars() {
        if forbidden.contains(&ch) {
            bail!("Name contains forbidden character '{ch}' (path separators and wildcards are not allowed)");
        }
        if ch.is_control() {
            bail!("Name contains a control character");
        }
    }
    if name.ends_with('.') {
        bail!("Name must not end with '.' (Windows strips it silently)");
    }
    if name.ends_with(' ') {
        bail!("Name must not end with a space (Windows strips it silently)");
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    // v26.4-alpha.1 (robustness finding F1): Windows also reserves the
    // superscript-digit forms COM¹²³ — normalize superscripts before the
    // stem check so "COM¹" cannot slip through.
    let stem = stem
        .replace('¹', "1")
        .replace('²', "2")
        .replace('³', "3");
    if RESERVED_STEMS.contains(&stem.as_str()) {
        bail!("'{stem}' is a reserved Windows device name and cannot be used");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_names() {
        for n in ["vanilla-test", "Vanilla 1.21.4", "测试实例", "a.b_c-d"] {
            assert!(validate_name(n).is_ok(), "{n} should be valid");
        }
    }

    #[test]
    fn test_empty_and_too_long() {
        assert!(validate_name("").is_err());
        assert!(validate_name(&"a".repeat(MAX_NAME_LEN)).is_ok());
        assert!(validate_name(&"a".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn test_reserved_device_stems() {
        for n in ["CON", "con", "Con.txt", "nul.jar", "COM1", "lpt3.save"] {
            assert!(validate_name(n).is_err(), "{n} should be rejected");
        }
        // v26.4-alpha.1: superscript-digit variants (F1).
        for n in ["COM¹", "com².txt", "COM³"] {
            assert!(validate_name(n).is_err(), "{n} should be rejected");
        }
        // Non-reserved stems containing reserved substrings are fine.
        assert!(validate_name("console-test").is_ok());
        assert!(validate_name("control").is_ok());
    }

    #[test]
    fn test_forbidden_chars_and_traversal() {
        for n in ["a/b", "a\\b", "C:x", "a*b", "a?", "a<b", "a|b"] {
            assert!(validate_name(n).is_err(), "{n} should be rejected");
        }
        assert!(validate_name("..\0x").is_err()); // control char via NUL byte literal
    }

    #[test]
    fn test_trailing_dot_space_and_dot_entries() {
        assert!(validate_name("abc.").is_err());
        assert!(validate_name("abc ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
    }
}
