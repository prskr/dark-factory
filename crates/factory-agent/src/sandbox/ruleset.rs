//! The filesystem rules of the harness's Landlock domain (design D2), built from a listing of
//! the root directory so that the function stays pure.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::Layout;

/// Access a rule grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rights {
    /// Read files and directories, execute.
    ReadExec,
    /// Read files and directories.
    Read,
    /// Every filesystem right of ABI 6.
    Full,
}

/// What a top-level entry is, without following symlinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntry {
    pub name: OsString,
    pub kind: EntryKind,
}

/// Lists the entries of `root` once; nothing below them is read.
pub fn list_root(root: &Path) -> io::Result<Vec<RootEntry>> {
    std::fs::read_dir(root)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::Other
            };
            Ok(RootEntry {
                name: entry.file_name(),
                kind,
            })
        })
        .collect()
}

/// The rules for `layout`. Every top-level directory that is not a symlink gets read and
/// execute, except the ancestors of the data, run and TLS directories, which only get the
/// grants below them. `/tmp` and `/dev` get full access when they are top-level directories.
///
/// A rule binds to the inode its path resolves to, so a top-level symlink (`/x` to `/`) would
/// be a grant on its target and is skipped.
pub fn ruleset(root: &Path, layout: &Layout, entries: &[RootEntry]) -> Vec<(PathBuf, Rights)> {
    let protected = [&layout.data_dir, &layout.run_dir, &layout.tls_dir];
    let mut dirs: Vec<&RootEntry> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Dir)
        .collect();
    dirs.sort_by(|a, b| a.name.cmp(&b.name));

    let mut rules: Vec<(PathBuf, Rights)> = dirs
        .into_iter()
        .map(|entry| (root.join(&entry.name), entry))
        .filter(|(path, _)| !protected.iter().any(|dir| dir.starts_with(path)))
        .map(|(path, entry)| {
            let rights = if entry.name == "tmp" || entry.name == "dev" {
                Rights::Full
            } else {
                Rights::ReadExec
            };
            (path, rights)
        })
        .collect();
    rules.push((layout.repo_dir(), Rights::Full));
    rules.push((layout.home_dir(), Rights::Full));
    rules.push((layout.ca_dir(), Rights::Read));
    rules
}

/// Checks that the writable roots of the layout are directories without a symlink in their path.
///
/// A symlink left by an earlier unrestricted run (`/data/home` to `/data/runtime`) would turn the
/// full-access rule into a grant on protected files. Symlinks below the roots are not checked:
/// Landlock checks the resolved path, so they can't reach outside the granted trees.
pub fn verify_writable_roots(layout: &Layout) -> anyhow::Result<()> {
    for dir in [layout.repo_dir(), layout.home_dir()] {
        super::aliases::ensure_real_directory(&dir, "writable directory")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout {
            tls_dir: "/run/factory/tls".into(),
            data_dir: "/data".into(),
            run_dir: "/run/factory".into(),
        }
    }

    fn root(spec: &[(&str, EntryKind)]) -> Vec<RootEntry> {
        spec.iter()
            .map(|(name, kind)| RootEntry {
                name: (*name).into(),
                kind: *kind,
            })
            .collect()
    }

    fn rules(layout: &Layout, spec: &[(&str, EntryKind)]) -> Vec<(String, Rights)> {
        ruleset(Path::new("/"), layout, &root(spec))
            .into_iter()
            .map(|(path, rights)| (path.display().to_string(), rights))
            .collect()
    }

    fn has(rules: &[(String, Rights)], path: &str, rights: Rights) -> bool {
        rules.contains(&(path.to_string(), rights))
    }

    fn mentions(rules: &[(String, Rights)], path: &str) -> bool {
        rules.iter().any(|(p, _)| p == path)
    }

    use EntryKind::{Dir, Symlink};

    #[test]
    fn data_and_run_are_excluded_and_other_top_level_directories_read_only() {
        let rules = rules(
            &layout(),
            &[
                ("usr", Dir),
                ("opt", Dir),
                ("etc", Dir),
                ("data", Dir),
                ("run", Dir),
            ],
        );
        for dir in ["/usr", "/opt", "/etc"] {
            assert!(has(&rules, dir, Rights::ReadExec), "{dir}");
        }
        assert!(!mentions(&rules, "/data"));
        assert!(!mentions(&rules, "/run"));
    }

    #[test]
    fn writable_directories_come_from_the_layout() {
        let layout = Layout {
            tls_dir: "/srv/run/tls".into(),
            data_dir: "/srv/df".into(),
            run_dir: "/srv/run".into(),
        };
        let rules = rules(
            &layout,
            &[("usr", Dir), ("srv", Dir), ("tmp", Dir), ("dev", Dir)],
        );
        assert!(has(&rules, "/srv/df/repo", Rights::Full));
        assert!(has(&rules, "/srv/df/home", Rights::Full));
        assert!(has(&rules, "/tmp", Rights::Full));
        assert!(has(&rules, "/dev", Rights::Full));
        assert!(!mentions(&rules, "/srv"));
        assert!(!mentions(&rules, "/data/repo"));
    }

    #[test]
    fn ca_directory_is_read_only_and_the_tls_directory_is_not_granted() {
        let rules = rules(&layout(), &[("usr", Dir), ("run", Dir)]);
        assert!(has(&rules, "/run/factory/ca", Rights::Read));
        assert!(!rules.iter().any(|(p, _)| p.contains("tls")));
        assert!(!mentions(&rules, "/run/factory"));
    }

    #[test]
    fn top_level_symlinks_are_skipped() {
        let rules = rules(
            &layout(),
            &[("usr", Dir), ("x", Symlink), ("bin", Symlink), ("var", Dir)],
        );
        assert!(!mentions(&rules, "/x"));
        assert!(!mentions(&rules, "/bin"));
        assert!(has(&rules, "/var", Rights::ReadExec));
    }

    #[test]
    fn a_missing_tmp_gets_no_rule() {
        let rules = rules(&layout(), &[("usr", Dir)]);
        assert!(!mentions(&rules, "/tmp"));
    }

    #[test]
    fn a_symlinked_tmp_gets_no_rule() {
        let rules = rules(&layout(), &[("usr", Dir), ("tmp", Symlink)]);
        assert!(!mentions(&rules, "/tmp"));
    }

    fn temp_layout(dir: &Path) -> Layout {
        Layout {
            tls_dir: dir.join("run/tls"),
            data_dir: dir.join("data"),
            run_dir: dir.join("run"),
        }
    }

    #[test]
    fn writable_roots_that_are_real_directories_pass() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let layout = temp_layout(temp.path());
        std::fs::create_dir_all(layout.repo_dir())?;
        std::fs::create_dir_all(layout.home_dir())?;
        verify_writable_roots(&layout)
    }

    #[test]
    fn a_symlinked_home_is_refused() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let layout = temp_layout(temp.path());
        std::fs::create_dir_all(layout.repo_dir())?;
        std::fs::create_dir_all(layout.outbox_dir())?;
        std::os::unix::fs::symlink(layout.outbox_dir(), layout.home_dir())?;
        let error = verify_writable_roots(&layout).unwrap_err().to_string();
        assert!(error.contains("symlink"), "{error}");
        Ok(())
    }

    #[test]
    fn a_missing_writable_root_is_refused() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let layout = temp_layout(temp.path());
        std::fs::create_dir_all(layout.repo_dir())?;
        assert!(verify_writable_roots(&layout).is_err());
        Ok(())
    }
}
