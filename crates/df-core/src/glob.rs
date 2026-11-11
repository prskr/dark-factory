//! Repository path globs of a project's push policy: protected paths and hidden paths
//! (iter3-hidden-paths design D1, D2). The Git proxy, the control plane and the runtime compile
//! them through this module, so all of them match the same paths.

use globset::{GlobBuilder, GlobSetBuilder};

pub use globset::{Glob, GlobSet};

/// A glob is invalid.
#[derive(Debug, thiserror::Error)]
pub enum GlobError {
    #[error(transparent)]
    Syntax(#[from] globset::Error),
    /// Gitignore-style sparse patterns have no alternation.
    #[error("alternation with {{}} is not supported")]
    Alternation,
}

/// A protected path glob as the Git proxy matches it: `*` does not match `/`.
pub fn protected_glob(text: &str) -> Result<Glob, GlobError> {
    Ok(GlobBuilder::new(text).literal_separator(true).build()?)
}

/// A hidden path glob: a [`protected_glob`] without `{` and `}`.
pub fn hidden_glob(text: &str) -> Result<Glob, GlobError> {
    if text.contains(['{', '}']) {
        return Err(GlobError::Alternation);
    }
    protected_glob(text)
}

/// The set of the hidden `globs`, in their order.
pub fn hidden_glob_set<S: AsRef<str>>(globs: &[S]) -> Result<GlobSet, GlobError> {
    let mut set = GlobSetBuilder::new();
    for text in globs {
        set.add(hidden_glob(text.as_ref())?);
    }
    Ok(set.build()?)
}

/// The lines of `.git/info/sparse-checkout` (non-cone mode) that exclude the hidden `globs`.
///
/// Every glob is anchored with `/`. A sparse pattern that matches a directory excludes its whole
/// subtree, a glob only the paths it names; so a glob that does not end in `/**` is followed by
/// a directory-only re-include that brings the subtrees back.
pub fn sparse_patterns<S: AsRef<str>>(globs: &[S]) -> Vec<String> {
    let mut lines = vec!["/*".to_owned()];
    for glob in globs {
        let glob = glob.as_ref();
        lines.push(format!("!/{glob}"));
        if !glob.ends_with("/**") {
            lines.push(format!("/{glob}/"));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtree_glob_is_excluded_without_reinclude() {
        assert_eq!(sparse_patterns(&["talos/**"]), ["/*", "!/talos/**"]);
    }

    #[test]
    fn root_file_glob_is_anchored_and_reincludes_directories() {
        assert_eq!(
            sparse_patterns(&["CODEOWNERS"]),
            ["/*", "!/CODEOWNERS", "/CODEOWNERS/"]
        );
    }

    #[test]
    fn single_level_glob_reincludes_nested_directories() {
        assert_eq!(
            sparse_patterns(&["configs/*"]),
            ["/*", "!/configs/*", "/configs/*/"]
        );
    }

    #[test]
    fn no_globs_give_full_checkout() {
        assert_eq!(sparse_patterns::<&str>(&[]), ["/*"]);
    }

    #[test]
    fn alternation_is_rejected() {
        assert!(matches!(
            hidden_glob("{a,b}/**"),
            Err(GlobError::Alternation)
        ));
        assert!(hidden_glob_set(&["ok/**", "x}"]).is_err());
    }

    #[test]
    fn invalid_syntax_is_rejected() {
        assert!(matches!(hidden_glob("a[b"), Err(GlobError::Syntax(_))));
    }

    #[test]
    fn star_does_not_cross_directories() {
        let set = hidden_glob_set(&["configs/*"]).unwrap();
        assert!(set.is_match("configs/top.txt"));
        assert!(!set.is_match("configs/nested/visible.txt"));
    }

    #[test]
    fn double_star_crosses_directories() {
        let matcher = protected_glob("talos/**").unwrap().compile_matcher();

        assert!(matcher.is_match("talos/a/b"));
        assert!(!matcher.is_match("other/talos/a"));
    }
}
