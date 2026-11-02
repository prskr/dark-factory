//! The `df:` reasons of the push checks (iter2-push-checks design D2): the Git proxy writes them
//! for a rejected ref, the runtime reads them from `git push`, the control plane quotes them.

use crate::gateway::v1::PushCheck;

/// Reason of a ref rejected because the push body, or the commits a ref adds, are too large.
pub const PUSH_TOO_LARGE: &str = "df:push-too-large";

/// Reason of a ref the Git proxy could not check.
pub const CHECK_FAILED: &str = "df:check-failed";

/// Reason of a ref update that is not a fast-forward of the ref's current tip.
pub const NON_FAST_FORWARD: &str = "df:non-fast-forward";

impl PushCheck {
    /// Every check, without [`PushCheck::Unspecified`].
    pub const ALL: [Self; 5] = [
        Self::SecretScan,
        Self::Identity,
        Self::ProtectedPath,
        Self::BlobTooLarge,
        Self::MergeCommit,
    ];

    /// The `df:` reason of a ref this check rejected; `None` for [`PushCheck::Unspecified`].
    pub fn reason_code(self) -> Option<&'static str> {
        match self {
            Self::Unspecified => None,
            Self::SecretScan => Some("df:secret-scan"),
            Self::Identity => Some("df:identity"),
            Self::ProtectedPath => Some("df:protected-path"),
            Self::BlobTooLarge => Some("df:blob-too-large"),
            Self::MergeCommit => Some("df:merge-commit"),
        }
    }

    /// The check whose [`reason_code`](Self::reason_code) is `code`.
    pub fn from_reason_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|check| check.reason_code() == Some(code))
    }
}

/// One finding as a rejection reason: `df:<check> <path>[:<line>] (<detail>)`, without the path
/// or the parentheses where they are empty. A finding without a check reads [`CHECK_FAILED`].
pub fn finding_reason(check: PushCheck, path: &str, line: Option<u32>, detail: &str) -> String {
    let mut reason = check.reason_code().unwrap_or(CHECK_FAILED).to_owned();
    match (path, line) {
        ("", _) => {}
        (path, None) => reason.push_str(&format!(" {path}")),
        (path, Some(line)) => reason.push_str(&format!(" {path}:{line}")),
    }
    if !detail.is_empty() {
        reason.push_str(&format!(" ({detail})"));
    }
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_round_trips_through_its_reason_code() {
        for check in PushCheck::ALL {
            let code = check.reason_code().unwrap();
            assert!(code.starts_with("df:"), "{code}");
            assert_eq!(PushCheck::from_reason_code(code), Some(check));
        }
    }

    #[test]
    fn unknown_codes_and_the_unspecified_check_have_no_mapping() {
        assert_eq!(PushCheck::Unspecified.reason_code(), None);
        for code in [
            PUSH_TOO_LARGE,
            CHECK_FAILED,
            "df:non-fast-forward",
            "secret-scan",
            "",
        ] {
            assert_eq!(PushCheck::from_reason_code(code), None, "{code}");
        }
    }

    #[test]
    fn a_check_reason_puts_the_detail_in_parentheses_after_path_and_line() {
        assert_eq!(
            finding_reason(
                PushCheck::SecretScan,
                "config.py",
                Some(3),
                "betterleaks.github-pat ghp_********"
            ),
            "df:secret-scan config.py:3 (betterleaks.github-pat ghp_********)"
        );
        assert_eq!(
            finding_reason(
                PushCheck::ProtectedPath,
                "CODEOWNERS",
                None,
                "matches CODEOWNERS"
            ),
            "df:protected-path CODEOWNERS (matches CODEOWNERS)"
        );
        assert_eq!(
            finding_reason(
                PushCheck::MergeCommit,
                "",
                None,
                "commit 0a1b has 2 parents"
            ),
            "df:merge-commit (commit 0a1b has 2 parents)"
        );
    }

    #[test]
    fn a_check_reason_without_a_detail_has_no_parentheses() {
        assert_eq!(
            finding_reason(PushCheck::ProtectedPath, "CODEOWNERS", None, ""),
            "df:protected-path CODEOWNERS"
        );
        assert_eq!(
            finding_reason(PushCheck::Identity, "", None, ""),
            "df:identity"
        );
        assert_eq!(
            finding_reason(PushCheck::Unspecified, "", None, ""),
            CHECK_FAILED
        );
    }
}
