//! `cargo xtask check-test-names [<file>...]`: tests are named after behavior, not after review history.
//!
//! Scans `.rs` files (all tracked ones, or the given ones) for `#[test]` / `#[tokio::test...]` and
//! reads the name of the next `fn`. A name fails when one of its `_`-separated words is `finding`,
//! `round<n>`, `r<n>` or `td<n>` (case-insensitive). A false positive is renamed.
//!
//! The bare words `review` and `round` are allowed: `review` is a task status of the product and
//! `round_trips` is common. A review-history name has a number or `finding` next to them anyway.

use std::path::Path;

use anyhow::Result;

use crate::common;

pub fn run(root: &Path, files: &[String]) -> Result<()> {
    let files = common::files_or_tracked(root, files)?;
    let mut violations = Vec::new();
    for file in files.iter().filter(|f| f.ends_with(".rs")) {
        // Unreadable means deleted in the index; nothing to check.
        let Ok(source) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        for (line, name) in forbidden_tests(&source) {
            violations.push(format!(
                "{file}:{line}: test `{name}` is named after review history; name it after the behavior it checks"
            ));
        }
    }
    common::report("check-test-names", &violations)
}

/// `(line, name)` of every test function with a forbidden word in its name.
fn forbidden_tests(source: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut pending = false;
    for (i, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.starts_with("#[test]") || line.starts_with("#[tokio::test") {
            pending = true;
        }
        if !pending {
            continue;
        }
        if let Some(name) = fn_name(line) {
            pending = false;
            if name.split('_').any(is_forbidden_word) {
                found.push((i + 1, name));
            }
        }
    }
    found
}

fn fn_name(line: &str) -> Option<&str> {
    let rest = line
        .split("fn ")
        .nth(1)
        .filter(|_| !line.starts_with("//"))?;
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(&rest[..end])
}

fn is_forbidden_word(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    let numbered = |prefix: &str| {
        word.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    word == "finding" || ["round", "r", "td"].into_iter().any(numbered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<&str> {
        forbidden_tests(source)
            .into_iter()
            .map(|(_, n)| n)
            .collect()
    }

    #[test]
    fn numbered_review_name_under_test_attribute_is_reported() {
        assert_eq!(
            names("#[test]\nfn test_review_r12_x() {}"),
            ["test_review_r12_x"]
        );
    }

    #[test]
    fn forbidden_word_under_tokio_test_with_arguments_is_reported() {
        let src = "#[tokio::test(flavor = \"multi_thread\")]\nasync fn finding_3_is_fixed() {}";
        assert_eq!(names(src), ["finding_3_is_fixed"]);
    }

    #[test]
    fn name_after_further_attributes_and_docs_is_reported() {
        let src = "#[test]\n#[ignore = \"slow\"]\n/// doc\npub fn test_td41_gone() {}";
        assert_eq!(names(src), ["test_td41_gone"]);
    }

    #[test]
    fn every_forbidden_word_is_reported() {
        for name in [
            "a_finding",
            "a_round2",
            "a_r7",
            "a_td12",
            "a_ROUND3",
            "Finding_a",
        ] {
            assert_eq!(
                names(&format!("#[test]\nfn {name}() {{}}")),
                [name],
                "{name}"
            );
        }
    }

    #[test]
    fn words_inside_longer_words_are_allowed() {
        for name in [
            "test_error_kind",
            "in_review_status",
            "run_round_trips",
            "test_reviewer_role",
            "test_around",
            "test_trace",
            "test_tdd",
            "test_r",
        ] {
            assert!(
                names(&format!("#[test]\nfn {name}() {{}}")).is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn helper_function_without_test_attribute_passes() {
        assert!(names("fn review_helper() {}\n#[test]\nfn works() {}\nfn round3() {}").is_empty());
    }

    #[test]
    fn reported_line_is_the_one_of_the_fn() {
        assert_eq!(
            forbidden_tests("\n#[test]\nfn finding_x() {}"),
            [(3, "finding_x")]
        );
    }
}
