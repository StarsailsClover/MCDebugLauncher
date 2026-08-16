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

/// Verify the SHA1 hash of a file on disk. Reads in 64KB chunks so the
/// full file never needs to be in memory. This is the disk-based
/// counterpart to `verify_sha1`, used by the streaming downloader to
/// avoid buffering entire downloads in RAM.
pub async fn verify_sha1_file(path: &std::path::Path, expected: &str) -> std::io::Result<bool> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    let hash = format!("{:x}", result);
    Ok(hash.eq_ignore_ascii_case(expected))
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
