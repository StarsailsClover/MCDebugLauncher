// Archive extraction utilities

use anyhow::{Context, Result};
use std::path::Path;
use tracing::debug;

/// Extract a ZIP archive
pub async fn extract_zip(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    debug!("Extracting {:?} to {:?}", archive_path, dest_dir);

    let file = std::fs::File::open(archive_path)
        .context("Failed to open archive")?;

    let mut archive = zip::ZipArchive::new(file)
        .context("Failed to read ZIP archive")?;

    tokio::fs::create_dir_all(dest_dir)
        .await
        .context("Failed to create destination directory")?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .context("Failed to read archive entry")?;

        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)
                .context("Failed to create directory")?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create parent directory")?;
            }

            let mut outfile = std::fs::File::create(&outpath)
                .context("Failed to create output file")?;

            std::io::copy(&mut file, &mut outfile)
                .context("Failed to extract file")?;
        }

        // Set permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))
                    .context("Failed to set permissions")?;
            }
        }
    }

    debug!("Extraction complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    
    

    #[tokio::test]
    async fn test_extract_zip() {
        // This would need a test ZIP file
        // Skipping for now
    }
}
