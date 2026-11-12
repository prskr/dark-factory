//! The Linux part of the confinement: closing inherited descriptors and applying the Landlock
//! domain. Landlock exists on Linux only (the `landlock` crate does not compile elsewhere).

use anyhow::Context as _;
use std::path::Path;

use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, PathFdError, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
};

use super::ruleset::{self, Rights};
use crate::config::Layout;

/// Closes the inherited descriptors and applies the domain for `layout` below `root`.
pub(super) fn apply_domain(root: &Path, layout: &Layout) -> anyhow::Result<()> {
    close_inherited_fds().context("cannot close the inherited descriptors")?;
    confine(root, layout).context("cannot apply the Landlock domain")
}

/// Closes every descriptor above stderr, so the harness inherits only 0, 1 and 2 (design D1). Rust
/// opens its own descriptors close-on-exec, but one that the parent leaked without the flag (a job
/// server pipe of the test runner) would pass through `exec`. Landlock doesn't restrict
/// descriptors that are already open, so a leaked one would also keep its access.
fn close_inherited_fds() -> anyhow::Result<()> {
    let names = std::fs::read_dir("/proc/self/fd")
        .context("list /proc/self/fd")?
        .map(|entry| entry.map(|entry| entry.file_name()));
    for fd in descriptors_above_stderr(names)? {
        // The listing's own descriptor is gone by now: EBADF.
        match nix::unistd::close(fd) {
            Ok(()) | Err(nix::errno::Errno::EBADF) => {}
            Err(error) => return Err(error).with_context(|| format!("close descriptor {fd}")),
        }
    }
    Ok(())
}

/// The descriptor numbers above 2 among the entry names of `/proc/self/fd`. An entry that can't
/// be read is an error: its descriptor would stay open. Names that aren't numbers are skipped.
fn descriptors_above_stderr(
    names: impl IntoIterator<Item = std::io::Result<std::ffi::OsString>>,
) -> anyhow::Result<Vec<i32>> {
    let mut fds = Vec::new();
    for name in names {
        let name = name.context("read an entry of /proc/self/fd")?;
        if let Some(fd) = name.to_str().and_then(|name| name.parse::<i32>().ok())
            && fd > 2
        {
            fds.push(fd);
        }
    }
    Ok(fds)
}

/// Builds the rules for `layout` below `root` and restricts the calling thread (and everything it
/// starts) to them, with ABI 6 as a hard requirement.
fn confine(root: &Path, layout: &Layout) -> anyhow::Result<()> {
    ruleset::verify_writable_roots(layout)?;
    let entries = ruleset::list_root(root).with_context(|| format!("list {}", root.display()))?;
    let rules = ruleset::ruleset(root, layout, &entries);

    let abi = ABI::V6;
    let mut created = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|ruleset| ruleset.scope(Scope::from_all(abi)))
        .and_then(Ruleset::create)
        .context("Landlock ABI 6 or newer is required")?;
    for (path, rights) in &rules {
        let access = match rights {
            Rights::ReadExec => AccessFs::from_read(abi),
            Rights::Read => AccessFs::ReadFile | AccessFs::ReadDir,
            Rights::Full => AccessFs::from_all(abi),
        };
        let Some(fd) = open_rule_path(path, *rights)? else {
            continue;
        };
        created = created
            .add_rule(PathBeneath::new(fd, access))
            .with_context(|| path.display().to_string())?;
    }
    let status = created.restrict_self().context("restrict the process")?;
    anyhow::ensure!(
        status.ruleset == RulesetStatus::FullyEnforced,
        "the domain is not fully enforced: {:?}",
        status.ruleset
    );
    Ok(())
}

/// Opens the path of a rule. A top-level read and execute entry that can't be opened for lack of
/// permission (`/lost+found`, a `0700` `/root`) is skipped, `None`: file permissions already deny
/// it to this user, and without a rule Landlock denies it too. Any other failure, and every
/// failure of a writable directory or the CA directory, fails closed.
fn open_rule_path(path: &Path, rights: Rights) -> anyhow::Result<Option<PathFd>> {
    match PathFd::new(path) {
        Ok(fd) => Ok(Some(fd)),
        Err(PathFdError::OpenCall { source, .. })
            if rights == Rights::ReadExec
                && source.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Ok(None)
        }
        Err(error) => Err(error).with_context(|| path.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// A directory without search permission: its entries can't be opened, like the top-level
    /// entries of a root the user may list but not enter. `None` when the permission bits don't
    /// block the open here (root, or `CAP_DAC_OVERRIDE`): the case can't be set up then.
    fn unsearchable_entry() -> anyhow::Result<Option<(tempfile::TempDir, PathBuf)>> {
        let temp = tempfile::tempdir()?;
        let parent = temp.path().join("root");
        std::fs::create_dir_all(parent.join("opt"))?;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o444))?;
        let entry = parent.join("opt");
        if PathFd::new(&entry).is_ok() {
            eprintln!("skipping: this user can open the entry despite mode 0444 (privileged)");
            return Ok(None);
        }
        Ok(Some((temp, entry)))
    }

    #[test]
    fn a_top_level_entry_that_cannot_be_opened_for_lack_of_permission_is_skipped()
    -> anyhow::Result<()> {
        let Some((_temp, entry)) = unsearchable_entry()? else {
            return Ok(());
        };
        assert!(open_rule_path(&entry, Rights::ReadExec)?.is_none());
        Ok(())
    }

    #[test]
    fn a_writable_or_ca_directory_that_cannot_be_opened_fails_closed() -> anyhow::Result<()> {
        let Some((_temp, entry)) = unsearchable_entry()? else {
            return Ok(());
        };
        assert!(open_rule_path(&entry, Rights::Full).is_err());
        assert!(open_rule_path(&entry, Rights::Read).is_err());
        Ok(())
    }

    #[test]
    fn descriptors_above_stderr_are_listed_and_non_numbers_skipped() -> anyhow::Result<()> {
        let names = ["0", "2", "3", "17", "not-a-number"].map(|name| Ok(name.into()));
        assert_eq!(descriptors_above_stderr(names)?, [3, 17]);
        Ok(())
    }

    #[test]
    fn an_unreadable_directory_entry_is_an_error_so_no_descriptor_stays_open() {
        let names = [
            Ok("3".into()),
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        ];
        assert!(descriptors_above_stderr(names).is_err());
    }

    #[test]
    fn a_missing_top_level_entry_still_fails_closed() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(open_rule_path(&temp.path().join("gone"), Rights::ReadExec).is_err());
        Ok(())
    }

    #[test]
    fn an_openable_entry_is_opened() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(open_rule_path(temp.path(), Rights::ReadExec)?.is_some());
        Ok(())
    }
}
