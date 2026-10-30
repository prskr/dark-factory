//! Work-in-progress title prefixes: the draft form that works in every repository.

pub(crate) const DEFAULT_PREFIXES: &str = "WIP:,[WIP]";

pub(crate) fn parse_prefixes(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Splits a title into `(has a prefix, title without it)`, matching prefixes case-insensitively.
pub(crate) fn split_title(title: &str, prefixes: &[String]) -> (bool, String) {
    let trimmed = title.trim_start();
    for prefix in prefixes {
        if let Some((head, rest)) = trimmed.split_at_checked(prefix.len())
            && head.eq_ignore_ascii_case(prefix)
        {
            return (true, rest.trim_start().to_string());
        }
    }
    (false, title.to_string())
}

/// Title as sent to GitHub in `title-prefix` mode: the first prefix is added for drafts.
pub(crate) fn apply_title(title: &str, draft: bool, prefixes: &[String]) -> String {
    match prefixes.first() {
        Some(prefix) if draft => format!("{prefix} {title}"),
        _ => title.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes() -> Vec<String> {
        parse_prefixes(DEFAULT_PREFIXES)
    }

    #[test]
    fn splits_known_prefixes() {
        assert_eq!(
            split_title("WIP: DF-1: Fix", &prefixes()),
            (true, "DF-1: Fix".into())
        );
        assert_eq!(split_title("[wip] Fix", &prefixes()), (true, "Fix".into()));
        assert_eq!(
            split_title("Fix WIP: x", &prefixes()),
            (false, "Fix WIP: x".into())
        );
        assert_eq!(split_title("WÏ", &prefixes()), (false, "WÏ".into()));
    }

    #[test]
    fn applies_marker_only_to_drafts() {
        assert_eq!(apply_title("Fix", true, &prefixes()), "WIP: Fix");
        assert_eq!(apply_title("Fix", false, &prefixes()), "Fix");
    }
}
