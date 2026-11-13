//! Existing branch names and the project's branch rewrite policy (ADR-0025, ADR-0033).

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::ParseEnumError;

/// Longest accepted branch name in bytes.
pub const BRANCH_MAX_BYTES: usize = 200;

/// Agent branch globs of a new project: the branches of dependency bots.
const DEFAULT_AGENT_BRANCH_GLOBS: [&str; 2] = ["renovate/**", "dependabot/**"];

const FORBIDDEN_CHARS: &str = " ~^:?*[\\";

/// Why a branch name can't be used for a task on an existing branch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BranchError {
    #[error("the branch name is empty")]
    Empty,
    #[error("the branch name is longer than {BRANCH_MAX_BYTES} bytes")]
    TooLong,
    #[error("give the branch without `refs/heads/`")]
    RefsPrefix,
    #[error("`{0}` is not a valid branch name")]
    Invalid(String),
    #[error("`{0}` is the base branch of the project")]
    BaseBranch(String),
    #[error("`{0}` is a branch of the factory")]
    FactoryBranch(String),
}

/// A validated branch name for a task on an existing branch. The validated form is the stored
/// and used form; nothing normalizes it afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingBranch(String);

impl ExistingBranch {
    /// Validates `name` with the rules of `git check-ref-format --branch`, and rejects the
    /// project's base branch and `factory/` branches.
    pub fn parse(name: &str, project_base: &str) -> Result<Self, BranchError> {
        if name.is_empty() {
            return Err(BranchError::Empty);
        }
        if name.len() > BRANCH_MAX_BYTES {
            return Err(BranchError::TooLong);
        }
        if name.starts_with("refs/") {
            return Err(BranchError::RefsPrefix);
        }
        if !is_valid_ref_name(name) {
            return Err(BranchError::Invalid(name.to_owned()));
        }
        if name == project_base {
            return Err(BranchError::BaseBranch(name.to_owned()));
        }
        if name.starts_with("factory/") {
            return Err(BranchError::FactoryBranch(name.to_owned()));
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ExistingBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Git's `.lock` rule for ref names is case-sensitive; this is no file extension"
)]
fn is_valid_ref_name(name: &str) -> bool {
    if name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("@{")
        || name.contains("//")
        || name == "@"
    {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_ascii_control() || FORBIDDEN_CHARS.contains(c))
    {
        return false;
    }
    name.split('/')
        .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

/// Whether agents may rewrite their task branch (non-fast-forward pushes with lease).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RewritePolicy {
    /// Task branches accept fast-forward pushes only.
    Never,
    /// Rebases are pushed with `--force-with-lease`.
    #[default]
    WithLease,
}

impl RewritePolicy {
    /// Every policy, in the order of a select.
    pub const ALL: &'static [Self] = &[Self::Never, Self::WithLease];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::WithLease => "with-lease",
        }
    }
}

impl fmt::Display for RewritePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RewritePolicy {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "never" => Ok(Self::Never),
            "with-lease" => Ok(Self::WithLease),
            other => Err(ParseEnumError {
                kind: "rewrite policy",
                value: other.to_owned(),
            }),
        }
    }
}

impl TryFrom<String> for RewritePolicy {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

/// The agent branch globs of a new project.
pub fn default_agent_branch_globs() -> Vec<String> {
    DEFAULT_AGENT_BRANCH_GLOBS.map(str::to_owned).to_vec()
}

/// Whether `branch` matches one of the project's agent branch `globs`. A glob that doesn't
/// compile matches nothing (they are validated when a project is saved).
pub fn matches_agent_branch<S: AsRef<str>>(globs: &[S], branch: &str) -> bool {
    globs.iter().any(|glob| {
        crate::glob::protected_glob(glob.as_ref())
            .is_ok_and(|glob| glob.compile_matcher().is_match(branch))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_branch_globs_match_across_slashes_only_with_double_star() {
        let globs = default_agent_branch_globs();
        assert!(matches_agent_branch(&globs, "renovate/serde-1.x"));
        assert!(matches_agent_branch(&globs, "dependabot/cargo/serde-1.0"));
        assert!(!matches_agent_branch(&globs, "release/1.2"));
        assert!(!matches_agent_branch(&["renovate/*"], "renovate/a/b"));
        assert!(!matches_agent_branch::<&str>(&[], "renovate/serde-1.x"));
        assert!(!matches_agent_branch(&["renovate/["], "renovate/["));
    }

    fn parse(name: &str) -> Result<ExistingBranch, BranchError> {
        ExistingBranch::parse(name, "main")
    }

    #[test]
    fn branch_accepts_valid_names() {
        for name in ["renovate/serde-1.x", "fix/cve-2026", "a", "release/1.2"] {
            assert_eq!(parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn branch_rejects_invalid_names() {
        let long = "a".repeat(201);
        let invalid = [
            "refs/heads/x",
            "-x",
            "a..b",
            "a@{b",
            "a\u{7}b",
            "a b",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "x/",
            "x.",
            "x.lock",
            "a/x.lock",
            "a//b",
            "/a",
            "a/.b",
            "@",
        ];
        assert_eq!(parse(""), Err(BranchError::Empty));
        assert_eq!(parse(&long), Err(BranchError::TooLong));
        for name in invalid {
            assert!(parse(name).is_err(), "{name:?} must be rejected");
        }
    }

    #[test]
    fn branch_rejects_base_and_factory_branches() {
        assert_eq!(parse("main"), Err(BranchError::BaseBranch("main".into())));
        assert_eq!(
            parse("factory/task/DF-7"),
            Err(BranchError::FactoryBranch("factory/task/DF-7".into()))
        );
        assert!(ExistingBranch::parse("develop", "develop").is_err());
    }

    #[test]
    fn rewrite_policy_round_trips_text_and_serde() {
        assert_eq!(RewritePolicy::default(), RewritePolicy::WithLease);
        for policy in [RewritePolicy::Never, RewritePolicy::WithLease] {
            assert_eq!(policy.as_str().parse::<RewritePolicy>().unwrap(), policy);
            let json = serde_json::to_string(&policy).unwrap();
            assert_eq!(json, format!("\"{}\"", policy.as_str()));
            assert_eq!(
                serde_json::from_str::<RewritePolicy>(&json).unwrap(),
                policy
            );
        }
        assert!("always".parse::<RewritePolicy>().is_err());
    }
}
