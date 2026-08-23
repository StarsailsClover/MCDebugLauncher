// Disk usage helpers (v26.3-alpha.9 CLI, shared with the agent API in
// v26.3-alpha.1 so REST handlers and the CLI report identical numbers).

use std::path::Path;

/// Recursively sum file sizes under `dir` using a blocking walkdir on a
/// spawn_blocking task (walkdir is synchronous; avoids starving the runtime).
pub fn dir_size(dir: &Path) -> impl std::future::Future<Output = u64> + Send + '_ {
    let path = dir.to_path_buf();
    async move {
        tokio::task::spawn_blocking(move || {
            walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum::<u64>()
        })
        .await
        .unwrap_or(0)
    }
}

/// Format bytes as a human-readable string.
pub fn format_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
    }

    #[tokio::test]
    async fn test_dir_size_empty_dir() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(d.path()).await, 0);
    }
}
