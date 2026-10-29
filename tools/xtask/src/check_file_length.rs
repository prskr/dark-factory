//! `cargo xtask check-file-length [<file>...]`: source files stay within the line limit.
//!
//! Without arguments every tracked file is checked (`git ls-files`), with arguments only those.
//! The limit and the allowlist live in the root `Cargo.toml`:
//!
//! ```toml
//! [workspace.metadata.file-length]
//! max-lines = 1500
//!
//! [workspace.metadata.file-length.allow]
//! "path/to/file.rs" = "reason"
//! ```
//!
//! An entry for a missing file, a file within the limit, or without a reason fails the check.

use std::path::Path;

use anyhow::{Context, Result};

use crate::common;

const SOURCE_EXTENSIONS: [&str; 13] = [
    "rs", "proto", "wit", "bzl", "bazel", "sh", "js", "ts", "css", "html", "toml", "yaml", "yml",
];

pub(crate) fn run(root: &Path, files: &[String]) -> Result<()> {
    let files = common::files_or_tracked(root, files)?;
    common::report("check-file-length", &check(root, &files)?)
}

fn check(root: &Path, files: &[String]) -> Result<Vec<String>> {
    let manifest = common::read_toml(&root.join("Cargo.toml"))?;
    let config = common::lookup(&manifest, &["workspace", "metadata", "file-length"])
        .context("Cargo.toml: missing [workspace.metadata.file-length]")?;
    let max = config
        .get("max-lines")
        .and_then(toml::Value::as_integer)
        .and_then(|m| usize::try_from(m).ok())
        .context("Cargo.toml: `max-lines` of [workspace.metadata.file-length] must be a number")?;
    let allow = config.get("allow").and_then(toml::Value::as_table);
    let reason = |file: &str| {
        allow
            .and_then(|a| a.get(file))
            .and_then(toml::Value::as_str)
    };

    let mut out = Vec::new();
    for file in files.iter().filter(|f| is_source(f)) {
        let Some(lines) = count_lines(root, file) else {
            continue;
        };
        if lines > max && reason(file).is_none() {
            out.push(format!(
                "{file}: {lines} lines (limit {max}); split it, or allowlist it with a reason in [workspace.metadata.file-length.allow]"
            ));
        }
    }
    for (file, reason) in allow.into_iter().flatten() {
        match count_lines(root, file) {
            None => out.push(format!("{file}: allowlisted but missing; remove the entry")),
            Some(lines) if lines <= max => {
                out.push(format!(
                    "{file}: allowlisted but only {lines} lines (limit {max}); remove the entry"
                ));
            }
            Some(_) if reason.as_str().is_none_or(|r| r.trim().is_empty()) => {
                out.push(format!("{file}: allowlist entry needs a reason"));
            }
            Some(_) => {}
        }
    }
    Ok(out)
}

fn is_source(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|e| SOURCE_EXTENSIONS.iter().any(|s| e == *s))
}

/// `None` when the file cannot be read as text (missing, deleted in the index, binary).
fn count_lines(root: &Path, file: &str) -> Option<usize> {
    std::fs::read_to_string(root.join(file))
        .ok()
        .map(|c| c.lines().count())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn repo(allow: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let manifest = format!(
            "[workspace.metadata.file-length]\nmax-lines = 3\n[workspace.metadata.file-length.allow]\n{allow}\n"
        );
        fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        dir
    }

    fn file(dir: &tempfile::TempDir, name: &str, lines: usize) {
        fs::write(dir.path().join(name), "x\n".repeat(lines)).unwrap();
    }

    fn violations(dir: &tempfile::TempDir, files: &[&str]) -> String {
        let files: Vec<String> = files.iter().map(|f| (*f).to_owned()).collect();
        check(dir.path(), &files).unwrap().join("\n")
    }

    #[test]
    fn file_over_the_limit_fails_with_its_line_count() {
        let dir = repo("");
        file(&dir, "a.rs", 4);
        let v = violations(&dir, &["a.rs"]);
        assert!(v.contains("a.rs") && v.contains("4 lines"), "{v}");
    }

    #[test]
    fn file_within_the_limit_passes() {
        let dir = repo("");
        file(&dir, "a.rs", 3);
        assert_eq!(violations(&dir, &["a.rs"]), "");
    }

    #[test]
    fn allowlisted_file_over_the_limit_passes() {
        let dir = repo("\"a.rs\" = \"generated\"");
        file(&dir, "a.rs", 9);
        assert_eq!(violations(&dir, &["a.rs"]), "");
    }

    #[test]
    fn allowlist_entry_for_a_short_file_is_stale() {
        let dir = repo("\"a.rs\" = \"generated\"");
        file(&dir, "a.rs", 3);
        assert!(violations(&dir, &[]).contains("a.rs: allowlisted but only 3 lines"));
    }

    #[test]
    fn allowlist_entry_for_a_missing_file_fails() {
        let dir = repo("\"gone.rs\" = \"generated\"");
        assert!(violations(&dir, &[]).contains("gone.rs: allowlisted but missing"));
    }

    #[test]
    fn allowlist_entry_without_reason_fails() {
        let dir = repo("\"a.rs\" = \" \"");
        file(&dir, "a.rs", 9);
        assert!(violations(&dir, &[]).contains("needs a reason"));
    }

    #[test]
    fn non_source_extensions_are_ignored() {
        let dir = repo("");
        for name in ["a.md", "Cargo.lock", "a.snap", "a.json"] {
            file(&dir, name, 99);
        }
        assert_eq!(
            violations(&dir, &["a.md", "Cargo.lock", "a.snap", "a.json"]),
            ""
        );
    }

    #[test]
    fn only_the_given_files_are_checked() {
        let dir = repo("");
        file(&dir, "big.rs", 9);
        file(&dir, "small.rs", 1);
        assert_eq!(violations(&dir, &["small.rs"]), "");
        assert!(violations(&dir, &["big.rs"]).contains("big.rs"));
    }

    #[test]
    fn deleted_staged_file_is_skipped() {
        let dir = repo("");
        assert_eq!(violations(&dir, &["deleted.rs"]), "");
    }
}
