//! File helpers for runtime-owned files in locations the agent can modify.

use std::io::Write;
use std::path::Path;

/// Writes `contents` to a newly created file at `path`.
///
/// An existing file or symlink at `path` is removed first, so the write never follows a link the
/// agent planted there.
pub fn write_fresh(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(contents.as_ref())
}

/// Creates `dir` (and parents) as a real directory; a symlink at `dir` is replaced.
pub fn create_real_dir(dir: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(dir)?;
    }
    std::fs::create_dir_all(dir)
}
