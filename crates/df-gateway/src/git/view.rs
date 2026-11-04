//! What one pushed ref adds, as the push checks see it (design D1): the commits reachable from
//! the pushed tip and not from a mirror ref that counts as upstream ([`TaskRefs`]), the paths
//! each of them changes, and the blobs they add.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

use gix::bstr::BStr;
use gix::diff::tree::recorder::Change;
use gix::diff::tree::{Recorder, State};
use gix::objs::TreeRefIter;
use gix::objs::tree::EntryMode;

use super::refs::ObjectId;
use crate::upstream::BoxError;

/// Most commits one pushed ref may add; above it the push is rejected with `df:push-too-large`.
pub const MAX_COMMITS: usize = 10_000;

/// The commits, changed paths and added blobs of one pushed ref.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushView {
    /// In walk order, the pushed tip first.
    pub commits: Vec<AddedCommit>,
    /// New blobs of the commits' changes, ordered by id.
    pub blobs: Vec<AddedBlob>,
}

/// A commit the push adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedCommit {
    pub id: gix::ObjectId,
    pub parents: Vec<gix::ObjectId>,
    /// Escaped like [`PathChange::path`].
    pub author_email: String,
    pub committer_email: String,
    /// Changes against each parent (the empty tree for a root commit), sorted. A rename is a
    /// deletion plus an addition. For a merge only the paths whose entry differs from the entry
    /// in every parent (combined diff), so merging the base branch adds none of its changes.
    pub changes: Vec<PathChange>,
}

/// The branches of the pushing run, as full ref names. They decide which mirror refs count as
/// upstream: all but the factory's WIP branches and the task branches of other tasks, whose
/// commits passed fewer checks or another task's exceptions.
#[derive(Debug, Clone, Copy)]
pub struct TaskRefs<'a> {
    pub task_branch: &'a str,
    pub wip_branch: &'a str,
}

impl TaskRefs<'_> {
    /// Whether the commits of the mirror ref `name` are already checked for this run.
    fn counts_as_upstream(&self, name: &str) -> bool {
        name == self.task_branch
            || !(in_namespace_of(name, self.task_branch) || in_namespace_of(name, self.wip_branch))
    }
}

/// A path a commit adds, modifies or deletes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathChange {
    /// Path in the repository; `\` as `\\`, control characters and bytes that are not UTF-8 as
    /// `\xNN`.
    pub path: String,
    /// Blob at the path before the change; `None` if there was none (an addition, a directory or
    /// a submodule).
    pub old: Option<gix::ObjectId>,
    /// Blob at the path after the change; `None` if there is none.
    pub new: Option<gix::ObjectId>,
}

/// A blob the push adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedBlob {
    pub id: gix::ObjectId,
    /// From the object header; the content is not read.
    pub size: u64,
    /// Paths the blob is added at, sorted.
    pub paths: Vec<String>,
}

/// The view could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error("the push adds more than {0} commits")]
    TooManyCommits(usize),
    #[error("building the push view was interrupted")]
    Interrupted,
    #[error("invalid object id {0}")]
    ObjectId(String),
    #[error("reading the pushed objects failed: {0}")]
    Git(#[source] BoxError),
}

/// The view of a push of `tip` by the run with the branches `task`, read from `objects` (the
/// quarantine, or the mirror for a push without a pack); `mirror` decides what is already
/// upstream. At most `max_commits` commits; setting `interrupt` aborts with an error. Blocking:
/// call it from `spawn_blocking`.
pub fn push_view(
    objects: &gix::Repository,
    mirror: &gix::Repository,
    task: TaskRefs<'_>,
    tip: &ObjectId,
    max_commits: usize,
    interrupt: &AtomicBool,
) -> Result<PushView, ViewError> {
    let tip = gix::ObjectId::from_hex(tip.as_str().as_bytes())
        .map_err(|_invalid| ViewError::ObjectId(tip.to_string()))?;
    let ids = added_commits(objects, mirror, task, tip, max_commits, interrupt)?;
    let mut state = State::default();
    let commits = ids
        .into_iter()
        .map(|id| {
            check_interrupt(interrupt)?;
            added_commit(objects, id, &mut state)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let blobs = added_blobs(objects, &commits)?;
    Ok(PushView { commits, blobs })
}

/// `bytes` as text that maps distinct byte strings to distinct strings: `\` as `\\`, control
/// characters and bytes that are not UTF-8 as `\xNN`.
pub(crate) fn escape(bytes: &BStr) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                out.push_str("\\\\");
            } else if c.is_control() {
                let mut utf8 = [0; 4];
                c.encode_utf8(&mut utf8)
                    .bytes()
                    .for_each(|b| push_hex(&mut out, b));
            } else {
                out.push(c);
            }
        }
        chunk.invalid().iter().for_each(|&b| push_hex(&mut out, b));
    }
    out
}

/// Walks from `tip` and stops at every commit reachable from a mirror ref that counts as upstream.
fn added_commits(
    objects: &gix::Repository,
    mirror: &gix::Repository,
    task: TaskRefs<'_>,
    tip: gix::ObjectId,
    max_commits: usize,
    interrupt: &AtomicBool,
) -> Result<Vec<gix::ObjectId>, ViewError> {
    let walk = objects
        .rev_walk([tip])
        .with_hidden(mirror_tips(mirror, task)?)
        .use_commit_graph(false)
        .all()
        .map_err(read_failed)?;
    let mut ids = Vec::new();
    for info in walk {
        check_interrupt(interrupt)?;
        let info = info.map_err(read_failed)?;
        if ids.len() == max_commits {
            return Err(ViewError::TooManyCommits(max_commits));
        }
        ids.push(info.id);
    }
    Ok(ids)
}

fn mirror_tips(
    mirror: &gix::Repository,
    task: TaskRefs<'_>,
) -> Result<Vec<gix::ObjectId>, ViewError> {
    let references = mirror.references().map_err(read_failed)?;
    let mut tips = Vec::new();
    for reference in references.all().map_err(read_failed)? {
        let mut reference = reference.map_err(read_failed)?;
        if task.counts_as_upstream(&reference.name().as_bstr().to_string()) {
            tips.push(reference.peel_to_id().map_err(read_failed)?.detach());
        }
    }
    Ok(tips)
}

/// Whether `name` is in the directory of `branch`, e.g. `refs/heads/factory/wip/` of
/// `refs/heads/factory/wip/DF-7`.
fn in_namespace_of(name: &str, branch: &str) -> bool {
    let Some((namespace, _key)) = branch.rsplit_once('/') else {
        return false;
    };
    name.strip_prefix(namespace)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn added_commit(
    repo: &gix::Repository,
    id: gix::ObjectId,
    state: &mut State,
) -> Result<AddedCommit, ViewError> {
    let commit = repo.find_commit(id).map_err(read_failed)?;
    let parents: Vec<gix::ObjectId> = commit.parent_ids().map(gix::Id::detach).collect();
    let tree = commit.tree_id().map_err(read_failed)?.detach();
    let mut bases = Vec::new();
    for parent in &parents {
        let parent_tree = repo
            .find_commit(*parent)
            .map_err(read_failed)?
            .tree_id()
            .map_err(read_failed)?;
        bases.push(Some(parent_tree.detach()));
    }
    if bases.is_empty() {
        bases.push(None);
    }
    let mut diffs = Vec::new();
    for base in bases {
        let mut diff = Vec::new();
        diff_trees(repo, base, tree, state, &mut diff)?;
        diffs.push(diff);
    }
    let changed: Vec<HashSet<&str>> = diffs
        .iter()
        .map(|diff| diff.iter().map(|change| change.path.as_str()).collect())
        .collect();
    let mut changes: Vec<PathChange> = diffs
        .iter()
        .flatten()
        .filter(|change| {
            changed
                .iter()
                .all(|paths| paths.contains(change.path.as_str()))
        })
        .cloned()
        .collect();
    changes.sort();
    changes.dedup();
    Ok(AddedCommit {
        id,
        parents,
        author_email: escape(commit.author().map_err(read_failed)?.email),
        committer_email: escape(commit.committer().map_err(read_failed)?.email),
        changes,
    })
}

/// Appends the changes from `old` (the empty tree if `None`) to `new` to `out`.
fn diff_trees(
    repo: &gix::Repository,
    old: Option<gix::ObjectId>,
    new: gix::ObjectId,
    state: &mut State,
    out: &mut Vec<PathChange>,
) -> Result<(), ViewError> {
    let kind = repo.object_hash();
    let old = old
        .map(|id| repo.find_tree(id))
        .transpose()
        .map_err(read_failed)?;
    let new = repo.find_tree(new).map_err(read_failed)?;
    let lhs = TreeRefIter::from_bytes(old.as_ref().map_or(&[][..], |tree| &tree.data), kind);
    let rhs = TreeRefIter::from_bytes(&new.data, kind);
    let mut recorder = Recorder::default();
    gix::diff::tree(lhs, rhs, &mut *state, &repo.objects, &mut recorder).map_err(read_failed)?;
    out.extend(recorder.records.into_iter().filter_map(path_change));
    Ok(())
}

/// A change of a blob, symlink or submodule; `None` for a change of a directory itself.
fn path_change(change: Change) -> Option<PathChange> {
    let blob = |mode: EntryMode, id: gix::ObjectId| mode.is_blob_or_symlink().then_some(id);
    let (path, old, new) = match change {
        Change::Addition {
            entry_mode,
            oid,
            path,
            ..
        } => (path, None, Some((entry_mode, oid))),
        Change::Deletion {
            entry_mode,
            oid,
            path,
            ..
        } => (path, Some((entry_mode, oid)), None),
        Change::Modification {
            previous_entry_mode,
            previous_oid,
            entry_mode,
            oid,
            path,
        } => (
            path,
            Some((previous_entry_mode, previous_oid)),
            Some((entry_mode, oid)),
        ),
    };
    let is_tree =
        |side: Option<(EntryMode, gix::ObjectId)>| side.is_none_or(|(mode, _)| mode.is_tree());
    if is_tree(old) && is_tree(new) {
        return None;
    }
    Some(PathChange {
        path: escape(path.as_ref()),
        old: old.and_then(|(mode, id)| blob(mode, id)),
        new: new.and_then(|(mode, id)| blob(mode, id)),
    })
}

/// The new blobs of `commits`' changes, also those the mirror has (it holds WIP and other tasks'
/// content), with their size from the header.
fn added_blobs(
    objects: &gix::Repository,
    commits: &[AddedCommit],
) -> Result<Vec<AddedBlob>, ViewError> {
    let mut added: BTreeMap<gix::ObjectId, AddedBlob> = BTreeMap::new();
    for change in commits.iter().flat_map(|commit| &commit.changes) {
        let Some(id) = change.new else { continue };
        if let Some(blob) = added.get_mut(&id) {
            blob.paths.push(change.path.clone());
            continue;
        }
        let size = objects.find_header(id).map_err(read_failed)?.size();
        let paths = vec![change.path.clone()];
        added.insert(id, AddedBlob { id, size, paths });
    }
    Ok(added
        .into_values()
        .map(|mut blob| {
            blob.paths.sort();
            blob.paths.dedup();
            blob
        })
        .collect())
}

fn push_hex(out: &mut String, byte: u8) {
    // Writing to a `String` cannot fail.
    write!(out, "\\x{byte:02x}").ok();
}

fn check_interrupt(interrupt: &AtomicBool) -> Result<(), ViewError> {
    if interrupt.load(Ordering::Relaxed) {
        Err(ViewError::Interrupted)
    } else {
        Ok(())
    }
}

/// A failure of `gix` while reading the pushed objects, with the error as its source.
fn read_failed(error: impl std::error::Error + Send + Sync + 'static) -> ViewError {
    ViewError::Git(Box::new(error))
}

#[cfg(test)]
mod tests;
