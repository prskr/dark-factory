//! Keeps hidden paths out of the working tree (iter3-hidden-paths design D3, D4): applies the
//! sparse patterns and removes files that are still there.

use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;
use df_core::glob::{GlobSet, hidden_glob_set};
use df_proto::transcript::v1::{Notice, NoticeLevel};

use crate::git::{Git, nul_separated};

/// Paths a `workspace.hidden_paths_removed` notice lists.
const MAX_LISTED_PATHS: usize = 50;

/// Applies the sparse patterns of `.git/info/sparse-checkout` to the working tree, then removes
/// every file below a `hidden` glob that is still there and returns their paths. Git leaves
/// modified files in place and ignored files are never part of the sparse set, so they are
/// removed by hand.
///
/// Runs while no agent process exists: a path component can't be swapped during the removal.
/// A path with a symlink among its parent directories is skipped, so the removal never leaves
/// the repository.
pub async fn enforce(
    git: &Git,
    repo_dir: &Path,
    hidden: &[String],
) -> anyhow::Result<Vec<PathBuf>> {
    let set = hidden_glob_set(hidden)?;
    reapply(git).await?;
    if hidden.is_empty() {
        return Ok(Vec::new());
    }

    let tracked = remove_matching(
        repo_dir,
        &set,
        listed_paths(git, &["ls-files", "-z"]).await?,
    )?;
    let untracked = remove_matching(
        repo_dir,
        &set,
        listed_paths(git, &["ls-files", "--others", "-z"]).await?,
    )?;
    if tracked.is_empty() && untracked.is_empty() {
        return Ok(Vec::new());
    }

    git.run_ok_chunked(&["--literal-pathspecs", "reset", "-q", "--"], &tracked)
        .await
        .context("git reset of hidden paths failed")?;
    // Sets the skip-worktree bit again for the entries that were reset
    reapply(git).await?;
    Ok(tracked.into_iter().chain(untracked).collect())
}

/// The notice that lists `removed`; `None` if nothing was removed.
pub fn removed_notice(removed: &[PathBuf]) -> Option<Notice> {
    if removed.is_empty() {
        return None;
    }
    let mut listed: Vec<String> = removed
        .iter()
        .take(MAX_LISTED_PATHS)
        .map(|path| path.display().to_string())
        .collect();
    if let Some(more) = removed
        .len()
        .checked_sub(MAX_LISTED_PATHS)
        .filter(|n| *n > 0)
    {
        listed.push(format!("and {more} more"));
    }
    Some(Notice {
        level: NoticeLevel::Warning as i32,
        code: "workspace.hidden_paths_removed".to_owned(),
        message: format!("Removed files under hidden paths: {}", listed.join(", ")),
    })
}

async fn reapply(git: &Git) -> anyhow::Result<()> {
    git.run_ok(&["sparse-checkout", "reapply"])
        .await
        .context("hidden paths need Git 2.27 or newer")?;
    Ok(())
}

/// The paths a `git ls-files -z …` lists.
async fn listed_paths(git: &Git, args: &[&str]) -> anyhow::Result<Vec<PathBuf>> {
    let out = git.run_ok(args).await?;
    Ok(nul_separated(&out.stdout).map(PathBuf::from).collect())
}

/// Removes the files of `paths` that match `set` and returns them. A path that is not a plain
/// relative path (`..`, an absolute path) can only come from a tampered index: it is skipped.
fn remove_matching(
    repo_dir: &Path,
    set: &GlobSet,
    paths: Vec<PathBuf>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for path in paths {
        if !set.is_match(&path) {
            continue;
        }
        if !is_plain_relative(&path) {
            tracing::warn!(path = %path.display(), "a hidden path is not a plain relative path; not removed");
            continue;
        }
        if remove(repo_dir, &path)? {
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Whether every component of `path` is a normal name: no `..`, `.`, root or prefix.
fn is_plain_relative(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// Removes `relative` below `repo_dir` and the directories it leaves empty. Returns false if
/// nothing was removed: the path is absent, or a parent directory is not a real directory.
fn remove(repo_dir: &Path, relative: &Path) -> std::io::Result<bool> {
    let mut parents: Vec<&Path> = relative
        .ancestors()
        .skip(1)
        .filter(|parent| !parent.as_os_str().is_empty())
        .collect();
    parents.reverse();
    for parent in &parents {
        match std::fs::symlink_metadata(repo_dir.join(parent)) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                tracing::warn!(path = %relative.display(), "a parent of a hidden path is not a directory; not removed");
                return Ok(false);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        }
    }

    let full = repo_dir.join(relative);
    let meta = match std::fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if meta.file_type().is_dir() {
        // An embedded repository: git lists it as one untracked entry
        std::fs::remove_dir_all(&full)?;
    } else {
        std::fs::remove_file(&full)?;
    }
    for parent in parents.iter().rev() {
        // Stops at the first directory that still has content
        if std::fs::remove_dir(repo_dir.join(parent)).is_err() {
            break;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_of_normal_components_are_plain_relative() {
        assert!(is_plain_relative(Path::new("talos/sub/x.yaml")));
        for path in [
            "../outside",
            "talos/../../outside",
            "/etc/passwd",
            "./talos",
        ] {
            assert!(!is_plain_relative(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_path_outside_the_repository_is_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(temp.path().join("outside.txt"), "precious").unwrap();
        let set = hidden_glob_set(&["**"]).unwrap();

        let removed = remove_matching(&repo, &set, vec![PathBuf::from("../outside.txt")]).unwrap();

        assert!(removed.is_empty());
        assert!(temp.path().join("outside.txt").exists());
    }

    fn removed(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|n| PathBuf::from(format!("talos/{n}")))
            .collect()
    }

    #[test]
    fn no_notice_when_nothing_was_removed() {
        assert!(removed_notice(&[]).is_none());
    }

    #[test]
    fn fifty_paths_are_listed_without_a_count() {
        let notice = removed_notice(&removed(50)).unwrap();

        assert_eq!(notice.code, "workspace.hidden_paths_removed");
        assert_eq!(notice.message.matches("talos/").count(), 50);
        assert!(!notice.message.contains("more"), "{}", notice.message);
    }

    #[test]
    fn paths_after_the_fiftieth_are_only_counted() {
        let notice = removed_notice(&removed(51)).unwrap();

        assert!(notice.message.contains("talos/49"), "{}", notice.message);
        assert!(!notice.message.contains("talos/50"), "{}", notice.message);
        assert!(
            notice.message.ends_with(", and 1 more"),
            "{}",
            notice.message
        );
    }
}
