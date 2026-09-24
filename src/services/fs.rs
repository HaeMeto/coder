//! Async file IO and directory scanning.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Scans a directory and returns (path, is_dir) entries. Directories first, then alphabetical.
pub fn scan_dir(dir: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // A symlink to a directory is shown (and expanded) as a directory. The
        // tree loads lazily, one level per expand, so a link cycle can't recurse.
        let is_dir = match entry.file_type() {
            Ok(t) if t.is_symlink() => std::fs::metadata(&path).is_ok_and(|m| m.is_dir()),
            Ok(t) => t.is_dir(),
            Err(_) => false,
        };
        // Hide the .git directory.
        if path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }
        entries.push((path, is_dir));
    }
    entries.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => {
            a.0.file_name()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .cmp(&b.0.file_name().unwrap_or_default().to_ascii_lowercase())
        }
    });
    Ok(entries)
}

/// Creates an empty file. Errors if the path already exists (checked
/// atomically by `create_new`, so a concurrent creator is never truncated).
pub fn create_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("'{}' already exists", path.display())
        }
        Err(e) => Err(e.into()),
    }
}

/// Creates a directory. Errors if the path already exists.
pub fn create_dir(path: &Path) -> Result<()> {
    if path.exists() {
        anyhow::bail!("'{}' already exists", path.display());
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}

/// Renames `from` to `to`. Errors if `to` already exists (never overwrites).
///
/// A regular file is moved by hard-linking it to `to` (which fails atomically
/// if `to` exists) and then unlinking `from`, so there is no window where a
/// concurrently created `to` gets clobbered. Directories, symlinks, and file
/// systems without hard links fall back to check-then-rename (the check uses
/// `symlink_metadata`, so a dangling symlink at `to` still counts as taken).
pub fn rename_path(from: &Path, to: &Path) -> Result<()> {
    let taken = || anyhow::anyhow!("'{}' already exists", to.display());
    if std::fs::symlink_metadata(from).is_ok_and(|m| m.is_file()) {
        match std::fs::hard_link(from, to) {
            Ok(()) => {
                std::fs::remove_file(from)?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(taken()),
            Err(_) => {} // e.g. no hard-link support: fall back below
        }
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(taken());
    }
    std::fs::rename(from, to)?;
    Ok(())
}

/// Deletes a file, or a directory with everything under it.
pub fn delete_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub async fn read_file(path: &Path) -> Result<String> {
    Ok(tokio::fs::read_to_string(path).await?)
}

/// Writes `contents` to `path` atomically (see [`write_atomic`]).
pub async fn write_file(path: &Path, contents: &str) -> Result<()> {
    let path = path.to_path_buf();
    let bytes = contents.as_bytes().to_vec();
    tokio::task::spawn_blocking(move || write_atomic(&path, &bytes)).await??;
    Ok(())
}

/// Replaces `path`'s contents atomically: the bytes go to a temp file in the
/// same directory, are flushed, and the temp file is renamed over `path`, so a
/// crash or full disk never leaves a truncated file behind. An existing file's
/// permissions are carried over; a symlink is resolved first so the link
/// itself survives and its target is what gets replaced. Blocking.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_impl(path, bytes, None)
}

/// Like [`write_atomic`], but the file is only readable by its owner (mode
/// 0600 on Unix) — for data such as unsaved buffer contents.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_impl(path, bytes, Some(0o600))
}

fn write_atomic_impl(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let target: PathBuf = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    };
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.coder-tmp.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let existing = std::fs::metadata(&target).ok();

    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(mode);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        match (mode, existing) {
            // Private files keep their restrictive mode even over an old file.
            (Some(_), _) => {}
            (None, Some(meta)) => std::fs::set_permissions(&tmp, meta.permissions())?,
            (None, None) => {}
        }
        std::fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coder-fs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_atomic_replaces_content_and_leaves_no_temp() {
        let d = temp_dir("atomic");
        let f = d.join("a.txt");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn write_atomic_keeps_permissions_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp_dir("perm");
        let f = d.join("run.sh");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o750)).unwrap();
        let link = d.join("link.sh");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new");
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);

        let p = d.join("private.toml");
        write_atomic_private(&p, b"x").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn create_and_rename_never_clobber() {
        let d = temp_dir("noclobber");
        let a = d.join("a");
        let b = d.join("b");
        create_file(&a).unwrap();
        assert!(create_file(&a).is_err());
        std::fs::write(&b, "keep").unwrap();
        assert!(rename_path(&a, &b).is_err());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "keep");
        let c = d.join("c");
        rename_path(&a, &c).unwrap();
        assert!(!a.exists() && c.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
