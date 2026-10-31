//! Quarantines the pack of a push next to the repository mirror (design D4/D5).
//!
//! The pack is indexed into `<dir>/q-*/objects/pack`, with `objects/info/alternates`
//! pointing at the mirror, so a thin pack resolves its bases from the mirror while the
//! mirror itself stays unchanged. Ancestry checks then see mirror and pack together.
//! Only SHA-1 repositories are supported; a SHA-256 mirror is rejected.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::hash::Kind;
use tempfile::TempDir;

use super::refs::{Ancestry, ObjectId};

/// Failure to quarantine a pack or to answer an ancestry query.
#[derive(Debug, thiserror::Error)]
pub enum QuarantineError {
    #[error("opening the mirror failed: {0}")]
    OpenMirror(String),
    #[error("the mirror uses {0:?} object ids; only SHA-1 is supported")]
    UnsupportedHash(Kind),
    #[error("creating the quarantine failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("indexing the pushed pack failed: {0}")]
    Pack(String),
    #[error("invalid object id {0}")]
    ObjectId(String),
    #[error("ancestry lookup failed: {0}")]
    Ancestry(String),
}

/// An indexed push pack, removed from disk on drop.
pub struct Quarantine {
    dir: TempDir,
    repo: gix::Repository,
}

impl Quarantine {
    /// The quarantine's `objects` directory (pack plus alternates to the mirror).
    pub fn objects_dir(&self) -> PathBuf {
        self.dir.path().join("objects")
    }
}

impl std::fmt::Debug for Quarantine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quarantine")
            .field("dir", &self.dir.path())
            .finish_non_exhaustive()
    }
}

/// Indexes `pack` (possibly thin) into a new quarantine directory in `dir`, resolving
/// missing bases from the bare repository at `mirror`. `alloc_limit` bounds single
/// allocations while resolving deltas; setting `interrupt` aborts the indexing with an error.
/// Blocking: call it from `spawn_blocking`.
pub fn quarantine(
    mirror: &Path,
    pack: impl Read,
    dir: &Path,
    alloc_limit: usize,
    interrupt: &AtomicBool,
) -> Result<Quarantine, QuarantineError> {
    let mirror_repo = open_isolated(mirror)?;
    let kind = mirror_repo.object_hash();
    if kind != Kind::Sha1 {
        return Err(QuarantineError::UnsupportedHash(kind));
    }
    std::fs::create_dir_all(dir)?;
    let qdir = tempfile::Builder::new().prefix("q-").tempdir_in(dir)?;
    let objects = qdir.path().join("objects");
    std::fs::create_dir_all(objects.join("pack"))?;
    std::fs::create_dir_all(objects.join("info"))?;
    let mirror_objects = mirror_repo.path().join("objects").canonicalize()?;
    std::fs::write(
        objects.join("info/alternates"),
        format!("{}\n", mirror_objects.display()),
    )?;

    let options = gix_pack::bundle::write::Options {
        alloc_limit_bytes: Some(alloc_limit),
        ..Default::default()
    };
    gix_pack::Bundle::write_to_directory(
        &mut BufReader::new(pack),
        Some(&objects.join("pack")),
        &mut gix::progress::Discard,
        interrupt,
        Some(mirror_repo.objects.clone()),
        kind,
        options,
    )
    .map_err(|e| QuarantineError::Pack(e.to_string()))?;

    let mut repo = mirror_repo;
    let store = gix::odb::at(&objects, kind).map_err(|e| QuarantineError::Pack(e.to_string()))?;
    repo.objects = gix::odb::memory::Proxy::new(store, kind);
    Ok(Quarantine { dir: qdir, repo })
}

impl Ancestry for Quarantine {
    type Error = QuarantineError;

    fn is_ancestor(&self, ancestor: &ObjectId, descendant: &ObjectId) -> Result<bool, Self::Error> {
        is_ancestor_in(&self.repo, ancestor, descendant)
    }
}

/// Ancestry from the mirror alone, for a push without a pack (e.g. a pushed commit that
/// already exists upstream).
pub struct MirrorAncestry {
    repo: gix::Repository,
}

impl MirrorAncestry {
    /// Opens the bare repository at `mirror`. Blocking: call it from `spawn_blocking`.
    pub fn open(mirror: &Path) -> Result<Self, QuarantineError> {
        let repo = open_isolated(mirror)?;
        Ok(Self { repo })
    }
}

impl Ancestry for MirrorAncestry {
    type Error = QuarantineError;

    fn is_ancestor(&self, ancestor: &ObjectId, descendant: &ObjectId) -> Result<bool, Self::Error> {
        is_ancestor_in(&self.repo, ancestor, descendant)
    }
}

/// Opens the mirror without system or user Git configuration, like the mirror fetch does.
fn open_isolated(mirror: &Path) -> Result<gix::Repository, QuarantineError> {
    gix::open_opts(mirror, gix::open::Options::isolated())
        .map_err(|e| QuarantineError::OpenMirror(e.to_string()))
}

fn is_ancestor_in(
    repo: &gix::Repository,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool, QuarantineError> {
    let old = to_gix(ancestor)?;
    let new = to_gix(descendant)?;
    if old == new {
        return Ok(true);
    }
    let base = repo
        .merge_base(old, new)
        .map_err(|e| QuarantineError::Ancestry(e.to_string()))?;
    Ok(base.is_some_and(|base| base.detach() == old))
}

fn to_gix(id: &ObjectId) -> Result<gix::ObjectId, QuarantineError> {
    gix::ObjectId::from_hex(id.as_str().as_bytes())
        .map_err(|_invalid| QuarantineError::ObjectId(id.to_string()))
}

#[cfg(test)]
mod tests;
