//! Shared rules of `pull-requests.list-open`: which pull request matches a filter, and the check
//! that a pull request's head branch lives in the repository itself.

/// The filter of `list-open`; all set fields apply together.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenFilter<'a> {
    /// Head branch in the repository itself, never in a fork.
    pub head: Option<&'a str>,
    pub head_prefix: Option<&'a str>,
    /// Login, compared case-insensitively.
    pub author: Option<&'a str>,
    pub label: Option<&'a str>,
    pub base: Option<&'a str>,
}

/// What the filter looks at in a pull request.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub head: &'a str,
    /// `owner/name` of the head repository as the forge reports it; `None` for a deleted fork.
    pub head_repo: Option<&'a str>,
    pub base: &'a str,
    pub author: &'a str,
    pub labels: &'a [String],
}

impl OpenFilter<'_> {
    /// Whether `pr` of the repository `owner/name` passes every set field.
    pub fn matches(&self, owner: &str, name: &str, pr: &Candidate<'_>) -> bool {
        self.head
            .is_none_or(|head| pr.head == head && head_in_repo(pr.head_repo, owner, name))
            && self.head_prefix.is_none_or(|p| pr.head.starts_with(p))
            && self.base.is_none_or(|b| pr.base == b)
            && self
                .author
                .is_none_or(|a| pr.author.eq_ignore_ascii_case(a))
            && self.label.is_none_or(|l| pr.labels.iter().any(|x| x == l))
    }
}

/// Whether the head repository `full_name` (`owner/name`, as the forge reports it) is
/// `owner/name`. A missing head repository (a deleted fork) is not.
pub fn head_in_repo(full_name: Option<&str>, owner: &str, name: &str) -> bool {
    full_name.is_some_and(|full| full.eq_ignore_ascii_case(&format!("{owner}/{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr<'a>(head: &'a str, author: &'a str, labels: &'a [String]) -> Candidate<'a> {
        Candidate {
            head,
            head_repo: Some("o/r"),
            base: "main",
            author,
            labels,
        }
    }

    #[test]
    fn no_filter_matches_everything_including_forks() {
        let fork = Candidate {
            head_repo: Some("fork/r"),
            ..pr("x", "a", &[])
        };
        assert!(OpenFilter::default().matches("o", "r", &fork));
    }

    #[test]
    fn prefix_and_label_apply_together() {
        let major = ["major".to_string()];
        let filter = OpenFilter {
            head_prefix: Some("renovate/"),
            label: Some("major"),
            ..OpenFilter::default()
        };
        assert!(filter.matches("o", "r", &pr("renovate/a", "bot", &major)));
        assert!(!filter.matches("o", "r", &pr("renovate/b", "bot", &[])));
        assert!(!filter.matches("o", "r", &pr("fix/login", "bot", &major)));
    }

    #[test]
    fn author_ignores_case_and_base_is_exact() {
        let filter = OpenFilter {
            author: Some("Renovate[bot]"),
            base: Some("main"),
            ..OpenFilter::default()
        };
        assert!(filter.matches("o", "r", &pr("a", "renovate[bot]", &[])));
        assert!(!filter.matches("o", "r", &pr("a", "alice", &[])));
        let develop = Candidate {
            base: "develop",
            ..pr("a", "renovate[bot]", &[])
        };
        assert!(!filter.matches("o", "r", &develop));
    }

    #[test]
    fn the_head_filter_matches_the_exact_branch_and_never_a_fork() {
        let filter = OpenFilter {
            head: Some("fix/login"),
            ..OpenFilter::default()
        };
        assert!(filter.matches("o", "r", &pr("fix/login", "a", &[])));
        assert!(!filter.matches("o", "r", &pr("fix/login2", "a", &[])));
        let fork = Candidate {
            head_repo: Some("fork/r"),
            ..pr("fix/login", "a", &[])
        };
        assert!(!filter.matches("o", "r", &fork));
    }

    #[test]
    fn the_head_repository_matches_case_insensitively_and_forks_do_not() {
        assert!(head_in_repo(Some("O/R"), "o", "r"));
        assert!(!head_in_repo(Some("fork/r"), "o", "r"));
        assert!(!head_in_repo(None, "o", "r"));
    }
}
