//! Async file IO and directory scanning.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Scans a directory and returns (path, is_dir) entries. Directories first, then alphabetical.
pub fn scan_dir(dir: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // Hide the .git directory.
        if path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }
        entries.push((path, is_dir));
    }
    entries.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .0
            .file_name()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .cmp(&b.0.file_name().unwrap_or_default().to_ascii_lowercase()),
    });
    Ok(entries)
}

pub async fn read_file(path: &Path) -> Result<String> {
    Ok(tokio::fs::read_to_string(path).await?)
}

pub async fn write_file(path: &Path, contents: &str) -> Result<()> {
    tokio::fs::write(path, contents).await?;
    Ok(())
}
