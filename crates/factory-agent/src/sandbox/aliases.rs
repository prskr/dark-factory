//! Runtime-owned files the harness must not reach through an alias: the CA copy it may read and
//! the files under the outbox directory that an earlier unrestricted run may have hard-linked
//! (design D2).

use anyhow::Context as _;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Copies `<tls_dir>/ca.crt` to `<ca_dir>/ca.crt` through a temporary file and a rename, so a
/// reader sees the old or the new file and the destination never holds a partial one.
///
/// The CSI driver rotates `tls_dir` by giving `ca.crt` a new inode on every renewal; the harness
/// gets a rule on `ca_dir` instead of the file, which would stay bound to the old inode.
pub fn publish_ca(tls_dir: &Path, ca_dir: &Path) -> io::Result<()> {
    let ca = fs::read(tls_dir.join("ca.crt"))?;
    fs::create_dir_all(ca_dir)?;
    replace_file(&ca_dir.join("ca.crt"), ca.as_slice())
}

/// Checks that the outbox directory is a real directory (or absent) without a symlink in its path
/// and removes the symlinks below it, like D2 treats a symlinked writable root. An unrestricted run
/// could have replaced `dir` or a segment with a link into the writable home, which would redirect
/// the runtime's own writes there. Only the links are removed, never their targets. Returns how
/// many links were removed.
pub fn secure_outbox_dir(dir: &Path) -> anyhow::Result<usize> {
    match fs::symlink_metadata(dir) {
        Ok(_) => ensure_real_directory(dir, "outbox directory")?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e).with_context(|| format!("stat {}", dir.display())),
    }
    remove_symlinks(dir)
}

/// Fails unless `dir` is a directory whose canonical path is `dir` itself: no symlink in the path
/// and none as the directory. `what` names the directory in the error.
pub fn ensure_real_directory(dir: &Path, what: &str) -> anyhow::Result<()> {
    let canonical = fs::canonicalize(dir).with_context(|| format!("{what} {}", dir.display()))?;
    anyhow::ensure!(
        canonical == dir && canonical.is_dir(),
        "{what} {} resolves to {}: it must be a directory, not a symlink",
        dir.display(),
        canonical.display()
    );
    Ok(())
}

fn remove_symlinks(dir: &Path) -> anyhow::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir).with_context(|| format!("list {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("list {}", dir.display()))?
            .path();
        let file_type = fs::symlink_metadata(&path)
            .with_context(|| format!("stat {}", path.display()))?
            .file_type();
        if file_type.is_symlink() {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            removed += 1;
        } else if file_type.is_dir() {
            removed += remove_symlinks(&path)?;
        }
    }
    Ok(removed)
}

/// Replaces every file below `dir` that has more than one hard link with a copy, so a link that
/// the harness holds from an earlier unrestricted run no longer reaches the file. Symlinks are
/// neither followed nor replaced. Returns how many files were replaced; a missing `dir` has none.
///
/// Runs before the outbox is opened: an open file would keep writing to the old inode.
pub fn break_hard_links(dir: &Path) -> io::Result<usize> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut broken = 0;
    for entry in entries {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            broken += break_hard_links(&path)?;
        } else if meta.is_file() && meta.nlink() > 1 {
            replace_file(&path, fs::File::open(&path)?)?;
            fs::set_permissions(&path, meta.permissions())?;
            broken += 1;
        }
    }
    Ok(broken)
}

/// Streams `content` to a fresh sibling file and renames it over `path`.
fn replace_file(path: &Path, mut content: impl io::Read) -> io::Result<()> {
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = Path::new(&tmp_name);
    match fs::remove_file(tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(tmp)?;
    io::copy(&mut content, &mut file)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ca_copy_follows_a_renamed_ca_file() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let tls = temp.path().join("tls");
        let ca_dir = temp.path().join("run/ca");
        fs::create_dir_all(&tls)?;
        fs::write(tls.join("ca.crt"), "first")?;
        publish_ca(&tls, &ca_dir)?;
        assert_eq!(fs::read_to_string(ca_dir.join("ca.crt"))?, "first");

        // What the CSI driver does on renewal: a new file takes the place of the old one.
        fs::write(tls.join("ca.crt.new"), "second")?;
        fs::rename(tls.join("ca.crt.new"), tls.join("ca.crt"))?;
        publish_ca(&tls, &ca_dir)?;
        assert_eq!(fs::read_to_string(ca_dir.join("ca.crt"))?, "second");
        assert_eq!(
            fs::read_dir(&ca_dir)?.count(),
            1,
            "no temporary file is left"
        );
        Ok(())
    }

    #[test]
    fn publishing_without_a_ca_file_fails() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(publish_ca(temp.path(), &temp.path().join("ca")).is_err());
        Ok(())
    }

    #[test]
    fn a_file_with_two_links_is_replaced_and_keeps_its_content() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let outbox = temp.path().join("runtime");
        let nested = outbox.join("outbox");
        fs::create_dir_all(&nested)?;
        let segment = nested.join("seg-1");
        fs::write(&segment, "messages")?;
        let alias = temp.path().join("home-alias");
        fs::hard_link(&segment, &alias)?;
        let single = outbox.join("run.json");
        fs::write(&single, "{}")?;
        assert_eq!(fs::metadata(&segment)?.nlink(), 2);

        assert_eq!(break_hard_links(&outbox)?, 1);

        assert_eq!(fs::metadata(&segment)?.nlink(), 1);
        assert_eq!(fs::read_to_string(&segment)?, "messages");
        fs::write(&alias, "attacker")?;
        assert_eq!(fs::read_to_string(&segment)?, "messages");
        assert_eq!(fs::read_to_string(&single)?, "{}");
        Ok(())
    }

    #[test]
    fn symlinks_are_left_alone_and_a_missing_directory_is_fine() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("target");
        fs::write(&target, "x")?;
        fs::hard_link(&target, temp.path().join("other"))?;
        let outbox = temp.path().join("runtime");
        fs::create_dir(&outbox)?;
        std::os::unix::fs::symlink(&target, outbox.join("link"))?;
        assert_eq!(break_hard_links(&outbox)?, 0);
        assert_eq!(fs::metadata(&target)?.nlink(), 2);
        assert_eq!(break_hard_links(&temp.path().join("missing"))?, 0);
        Ok(())
    }

    #[test]
    fn an_outbox_directory_that_is_a_symlink_into_home_is_refused() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let home_target = temp.path().join("home/rt");
        fs::create_dir_all(&home_target)?;
        let outbox = temp.path().join("runtime");
        std::os::unix::fs::symlink(&home_target, &outbox)?;

        let error = secure_outbox_dir(&outbox).unwrap_err().to_string();

        assert!(error.contains("not a symlink"), "{error}");
        assert_eq!(
            fs::read_dir(&home_target)?.count(),
            0,
            "nothing was written"
        );
        Ok(())
    }

    #[test]
    fn a_symlink_inside_the_outbox_is_removed_and_its_target_kept() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let home_target = temp.path().join("home/ob");
        fs::create_dir_all(&home_target)?;
        fs::write(home_target.join("kept"), "x")?;
        let outbox = temp.path().join("runtime");
        fs::create_dir(&outbox)?;
        std::os::unix::fs::symlink(&home_target, outbox.join("outbox"))?;

        assert_eq!(secure_outbox_dir(&outbox)?, 1);

        assert!(fs::symlink_metadata(outbox.join("outbox")).is_err());
        assert_eq!(fs::read_to_string(home_target.join("kept"))?, "x");
        Ok(())
    }

    #[test]
    fn a_real_or_missing_outbox_directory_passes() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let outbox = temp.path().join("runtime");
        assert_eq!(secure_outbox_dir(&outbox)?, 0);
        fs::create_dir_all(outbox.join("outbox"))?;
        assert_eq!(secure_outbox_dir(&outbox)?, 0);
        Ok(())
    }
}
