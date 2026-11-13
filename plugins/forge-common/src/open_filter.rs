//! Shared rules of `pull-requests.list-open`: the filters that are not supported yet and the
//! check that a pull request's head branch lives in the repository itself.

/// Which filters beyond `head` a `list-open` call sets.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetFilters {
    pub head_prefix: bool,
    pub author: bool,
    pub label: bool,
    pub base: bool,
}

/// `Err` with the message for the first filter beyond `head` that is set; the plugins return it as
/// `invalid-input` until `iter3-ci-tools` adds the filters.
pub fn reject_unsupported_filters(filters: SetFilters) -> Result<(), String> {
    let set = [
        ("head-prefix", filters.head_prefix),
        ("author", filters.author),
        ("label", filters.label),
        ("base", filters.base),
    ];
    match set.iter().find(|(_, set)| *set) {
        Some((name, _)) => Err(format!(
            "the `{name}` filter of `list-open` is not supported yet"
        )),
        None => Ok(()),
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

    #[test]
    fn only_the_head_filter_is_supported() {
        assert!(reject_unsupported_filters(SetFilters::default()).is_ok());
        let both = SetFilters {
            author: true,
            base: true,
            ..SetFilters::default()
        };
        let err = reject_unsupported_filters(both).unwrap_err();
        assert!(err.contains("author"), "{err}");
        let base = SetFilters {
            base: true,
            ..SetFilters::default()
        };
        assert!(reject_unsupported_filters(base).is_err());
    }

    #[test]
    fn the_head_repository_matches_case_insensitively_and_forks_do_not() {
        assert!(head_in_repo(Some("O/R"), "o", "r"));
        assert!(!head_in_repo(Some("fork/r"), "o", "r"));
        assert!(!head_in_repo(None, "o", "r"));
    }
}
