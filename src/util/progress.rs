// Progress bar utilities for downloads and other long-running operations.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle, ProgressDrawTarget};
use std::sync::Arc;
use console::Term;

/// Check if the terminal supports ANSI colors and progress bars.
/// This is a workaround for Windows terminals that don't properly report ANSI support.
fn supports_ansi() -> bool {
    // On Windows, check if we're running in a modern terminal.
    // We check both stderr and stdout since progress bars typically use stderr.
    if cfg!(windows) {
        // If TERM is set, likely a modern terminal
        if std::env::var("TERM").is_ok() {
            return true;
        }
        
        // Check if stderr supports colors (indicatif uses stderr)
        let term = Term::stderr();
        term.features().colors_supported()
    } else {
        // On Unix, trust the terminal detection
        true
    }
}

/// A progress tracker for multiple concurrent downloads.
pub struct DownloadProgress {
    multi: Arc<MultiProgress>,
}

impl DownloadProgress {
    /// Create a new multi-progress tracker.
    pub fn new() -> Self {
        Self {
            multi: Arc::new(MultiProgress::new()),
        }
    }

    /// Create a progress bar for a single download with known size.
    pub fn add_download(&self, name: &str, total_bytes: u64) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total_bytes));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message(format!("{}", name));
        pb
    }

    /// Create a spinner for a download with unknown size.
    pub fn add_spinner(&self, name: &str) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("{}", name));
        pb
    }

    /// Create a progress bar for a collection of items (e.g., library downloads).
    pub fn add_collection(&self, name: &str, total_items: u64) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total_items));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.green/blue}] {pos}/{len}")
                .unwrap()
                .progress_chars("=>-"),
        );
        pb.set_message(format!("{}", name));
        pb
    }

    /// Get a clone of the underlying MultiProgress for manual control.
    pub fn multi(&self) -> Arc<MultiProgress> {
        self.multi.clone()
    }
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a simple standalone progress bar for a single download.
pub fn create_download_bar(total_bytes: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    
    // On Windows terminals that don't support ANSI, hide the progress bar
    // to avoid cluttering output with escape codes.
    if !supports_ansi() {
        pb.set_draw_target(ProgressDrawTarget::hidden());
        return pb;
    }
    
    // Force progress bar to draw to stderr with explicit refresh rate.
    pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));
    
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}
