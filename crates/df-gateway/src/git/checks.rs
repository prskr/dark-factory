//! The push checks (design D2): pure functions from a [`PushView`] to findings, and the verdict
//! over the findings that are left after the task's exceptions.

use std::collections::HashSet;

use df_core::glob::{GlobError, hidden_glob_set, protected_glob};
use df_proto::gateway::v1::{CheckMode, MaskedSecret, PushCheck, PushPolicy};
use globset::{GlobMatcher, GlobSet, GlobSetBuilder};

use super::view::{AddedBlob, PushView};
use crate::upstream::BoxError;

/// Always protected with mode [`Mode::Reject`], next to the project's globs.
pub const DARK_FACTORY_GLOB: &str = ".dark-factory/**";

/// A check of a push; [`PushCheck::reason_code`] names its `df:` rejection reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    SecretScan,
    Identity,
    ProtectedPath,
    BlobTooLarge,
    MergeCommit,
    HiddenPath,
}

impl From<Check> for PushCheck {
    fn from(check: Check) -> Self {
        match check {
            Check::SecretScan => Self::SecretScan,
            Check::Identity => Self::Identity,
            Check::ProtectedPath => Self::ProtectedPath,
            Check::BlobTooLarge => Self::BlobTooLarge,
            Check::MergeCommit => Self::MergeCommit,
            Check::HiddenPath => Self::HiddenPath,
        }
    }
}

/// What a finding does to the push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The push is rejected.
    Reject,
    /// The push is rejected unless the task has an exception for the finding.
    Approval,
}

impl From<CheckMode> for Mode {
    fn from(mode: CheckMode) -> Self {
        match mode {
            CheckMode::Approval => Self::Approval,
            CheckMode::Reject | CheckMode::Unspecified => Self::Reject,
        }
    }
}

impl From<Mode> for CheckMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Reject => Self::Reject,
            Mode::Approval => Self::Approval,
        }
    }
}

/// The ref a push updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    TaskBranch,
    /// Only the secret scan and the size limit run, every finding with [`Mode::Reject`].
    Wip,
}

/// The commit identity the agent's commits must carry.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    pub email: &'a str,
}

/// One finding of a check. Never contains a matched secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    pub mode: Mode,
    /// Key of an exception: `secret:<rule>:<blob>:<line>`, `path:<path>` or
    /// `size:<path>:<blob>`; `None` for identity and merge commit findings.
    pub fingerprint: Option<String>,
    /// Empty for identity and merge commit findings.
    pub path: String,
    /// 1-based line of a secret.
    pub line: Option<u32>,
    /// Masked, human-readable detail.
    pub detail: String,
    /// What a secret scan finding matched.
    pub secret: Option<MaskedSecret>,
}

/// A project's push policy, with its globs compiled.
pub struct CheckPolicy {
    always_protected: GlobMatcher,
    protected: GlobSet,
    globs: Vec<String>,
    hidden: GlobSet,
    hidden_globs: Vec<String>,
    protected_mode: Mode,
    secret_mode: Mode,
    size_mode: Mode,
    max_blob_bytes: u64,
    linear_history: bool,
}

impl CheckPolicy {
    /// Compiles the globs of `policy`; `*` does not match `/`.
    pub fn new(policy: &PushPolicy) -> Result<Self, GlobError> {
        let mut protected = GlobSetBuilder::new();
        for text in &policy.protected_globs {
            protected.add(protected_glob(text)?);
        }
        Ok(Self {
            always_protected: protected_glob(DARK_FACTORY_GLOB)?.compile_matcher(),
            protected: protected.build()?,
            globs: policy.protected_globs.clone(),
            hidden: hidden_glob_set(&policy.hidden_globs)?,
            hidden_globs: policy.hidden_globs.clone(),
            protected_mode: policy.protected_mode().into(),
            secret_mode: policy.secret_mode().into(),
            size_mode: policy.size_mode().into(),
            max_blob_bytes: policy.max_blob_bytes,
            linear_history: policy.linear_history,
        })
    }

    /// The first hidden glob that matches `path`.
    fn hidden_by(&self, path: &str) -> Option<&str> {
        let first = self.hidden.matches(path).into_iter().next()?;
        self.hidden_globs.get(first).map(String::as_str)
    }

    /// The mode and the matching glob if `path` is protected.
    fn protection(&self, path: &str) -> Option<(Mode, &str)> {
        if self.always_protected.is_match(path) {
            return Some((Mode::Reject, DARK_FACTORY_GLOB));
        }
        let first = self.protected.matches(path).into_iter().next()?;
        let glob = self.globs.get(first)?;
        Some((self.protected_mode, glob))
    }
}

/// A secret in a blob, as the secret matcher reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMatch {
    /// Id of the rule that matched.
    pub rule: String,
    /// 1-based line of the match.
    pub line: u32,
    /// The match with all but its first characters masked.
    pub masked: String,
}

/// Reading a blob for the secret scan failed.
#[derive(Debug, thiserror::Error)]
#[error("scanning blob {blob} for secrets failed: {source}")]
pub struct ScanError {
    pub blob: gix::ObjectId,
    #[source]
    pub source: BoxError,
}

/// Finds secrets in the blobs a push adds (design D3).
pub trait SecretMatcher {
    /// The secrets in `blob`, at most one per rule and line. Reads the blob's content.
    fn find(&self, blob: &AddedBlob) -> Result<Vec<SecretMatch>, ScanError>;
}

/// What happens to a pushed ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Forward,
    Reject {
        /// The findings without a matching exception.
        findings: Vec<Finding>,
        /// Only [`Mode::Approval`] findings are left: a human may approve them.
        approval_requested: bool,
    },
}

/// The findings of a push of `view` to `target`, each fingerprint at most once.
pub fn check_push(
    view: &PushView,
    policy: &CheckPolicy,
    identity: &Identity<'_>,
    target: Target,
    secrets: &dyn SecretMatcher,
) -> Result<Vec<Finding>, ScanError> {
    let mut findings = Vec::new();
    if target == Target::TaskBranch {
        findings.extend(identity_findings(view, identity));
        if policy.linear_history {
            findings.extend(merge_findings(view));
        }
    }
    // Before the protected paths: both use the fingerprint `path:<path>`, so a path that is
    // both yields only the hidden finding.
    findings.extend(hidden_path_findings(view, policy));
    if target == Target::TaskBranch {
        findings.extend(protected_path_findings(view, policy));
    }
    findings.extend(size_findings(view, policy));
    findings.extend(secret_findings(view, policy, secrets)?);
    if target == Target::Wip {
        for finding in &mut findings {
            finding.mode = Mode::Reject;
        }
    }
    let mut seen = HashSet::new();
    findings.retain(|finding| {
        finding
            .fingerprint
            .as_ref()
            .is_none_or(|fingerprint| seen.insert(fingerprint.clone()))
    });
    Ok(findings)
}

/// The verdict over `findings` once those with an exception of the task are removed. An
/// exception only passes an [`Mode::Approval`] finding.
pub fn verdict(findings: Vec<Finding>, exceptions: &[String]) -> Verdict {
    let findings: Vec<Finding> = findings
        .into_iter()
        .filter(|finding| {
            let excepted = finding.mode == Mode::Approval
                && finding
                    .fingerprint
                    .as_ref()
                    .is_some_and(|fingerprint| exceptions.contains(fingerprint));
            !excepted
        })
        .collect();
    if findings.is_empty() {
        return Verdict::Forward;
    }
    let approval_requested = findings
        .iter()
        .all(|finding| finding.mode == Mode::Approval);
    Verdict::Reject {
        findings,
        approval_requested,
    }
}

fn identity_findings<'a>(
    view: &'a PushView,
    identity: &'a Identity<'_>,
) -> impl Iterator<Item = Finding> + 'a {
    view.commits.iter().filter_map(|commit| {
        let roles: Vec<String> = [
            ("author", &commit.author_email),
            ("committer", &commit.committer_email),
        ]
        .into_iter()
        .filter(|(_, email)| *email != identity.email)
        .map(|(role, email)| format!("{role} {email}"))
        .collect();
        (!roles.is_empty()).then(|| Finding {
            check: Check::Identity,
            mode: Mode::Reject,
            fingerprint: None,
            path: String::new(),
            line: None,
            detail: format!("commit {} {}", commit.id, roles.join(", ")),
            secret: None,
        })
    })
}

fn merge_findings(view: &PushView) -> impl Iterator<Item = Finding> + '_ {
    view.commits
        .iter()
        .filter(|commit| commit.parents.len() > 1)
        .map(|commit| Finding {
            check: Check::MergeCommit,
            mode: Mode::Reject,
            fingerprint: None,
            path: String::new(),
            line: None,
            detail: format!("commit {} has {} parents", commit.id, commit.parents.len()),
            secret: None,
        })
}

fn hidden_path_findings<'a>(
    view: &'a PushView,
    policy: &'a CheckPolicy,
) -> impl Iterator<Item = Finding> + 'a {
    view.commits
        .iter()
        .flat_map(|commit| &commit.changes)
        .filter_map(|change| {
            let glob = policy.hidden_by(&change.path)?;
            Some(Finding {
                check: Check::HiddenPath,
                mode: Mode::Reject,
                fingerprint: Some(format!("path:{}", change.path)),
                path: change.path.clone(),
                line: None,
                detail: format!("matches {glob}"),
                secret: None,
            })
        })
}

fn protected_path_findings<'a>(
    view: &'a PushView,
    policy: &'a CheckPolicy,
) -> impl Iterator<Item = Finding> + 'a {
    view.commits
        .iter()
        .flat_map(|commit| &commit.changes)
        .filter_map(|change| {
            let (mode, glob) = policy.protection(&change.path)?;
            Some(Finding {
                check: Check::ProtectedPath,
                mode,
                fingerprint: Some(format!("path:{}", change.path)),
                path: change.path.clone(),
                line: None,
                detail: format!("matches {glob}"),
                secret: None,
            })
        })
}

fn size_findings<'a>(
    view: &'a PushView,
    policy: &'a CheckPolicy,
) -> impl Iterator<Item = Finding> + 'a {
    view.blobs
        .iter()
        .filter(|blob| blob.size > policy.max_blob_bytes)
        .flat_map(move |blob| {
            blob.paths.iter().map(move |path| Finding {
                check: Check::BlobTooLarge,
                mode: policy.size_mode,
                fingerprint: Some(format!("size:{path}:{}", blob.id)),
                path: path.clone(),
                line: None,
                detail: format!("{} bytes, limit {} bytes", blob.size, policy.max_blob_bytes),
                secret: None,
            })
        })
}

/// One finding per secret, reported at the first path of its blob.
fn secret_findings(
    view: &PushView,
    policy: &CheckPolicy,
    secrets: &dyn SecretMatcher,
) -> Result<Vec<Finding>, ScanError> {
    let mut findings = Vec::new();
    for blob in &view.blobs {
        let path = blob.paths.first().cloned().unwrap_or_default();
        findings.extend(secrets.find(blob)?.into_iter().map(|found| Finding {
            check: Check::SecretScan,
            mode: policy.secret_mode,
            fingerprint: Some(format!("secret:{}:{}:{}", found.rule, blob.id, found.line)),
            path: path.clone(),
            line: Some(found.line),
            detail: format!("{} {}", found.rule, found.masked),
            secret: Some(MaskedSecret {
                rule: found.rule,
                masked: found.masked,
            }),
        }));
    }
    Ok(findings)
}

#[cfg(test)]
mod tests;
