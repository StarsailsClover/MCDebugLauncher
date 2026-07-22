// SHA1 checksum verification

use sha1::{Digest, Sha1};

/// Verify SHA1 checksum of data
pub fn verify_sha1(data: &[u8], expected: &str) -> bool {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    let hash = format!("{:x}", result);
    hash.eq_ignore_ascii_case(expected)
}

/// Calculate SHA1 checksum of data
pub fn calculate_sha1(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_sha1() {
        let data = b"hello world";
        let expected = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";
        assert!(verify_sha1(data, expected));
        assert!(verify_sha1(data, "2AAE6C35C94FCFB415DBE95F408B9CE91EE846ED"));
        assert!(!verify_sha1(data, "invalid"));
    }

    #[test]
    fn test_calculate_sha1() {
        let data = b"hello world";
        let hash = calculate_sha1(data);
        assert_eq!(hash, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
    }
}
