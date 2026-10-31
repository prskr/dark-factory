//! `cargo xtask check-crates`: every workspace member is built, tested and linted in both build systems.
//!
//! Reads files only (no Bazel, no `cargo metadata`). Per directory in `crates/*` and `plugins/*`:
//!
//! 1. `crates/*` inherit the workspace lints; `plugins/*` keep their own `[lints.clippy]` table, which
//!    must contain every entry of `[workspace.lints.clippy]` (they differ only in `unsafe_code`).
//! 2. `BUILD.bazel` declares `extract_cargo_lints` and every Rust rule sets `lint_config`.
//! 3. Every `tests/<stem>.rs` has a Bazel target (`tests/<stem>.rs` or `"<stem>"` in `BUILD.bazel`).
//! 4. `MODULE.bazel` lists `//<dir>:Cargo.toml` in the `crate.from_cargo` manifests.
//!
//! A member listed with a reason in `[workspace.metadata.check-crates.exempt]` of the root
//! `Cargo.toml` (a crate that only `cargo` builds) skips items 2 to 4; an entry for a missing
//! directory, for a member that has a `BUILD.bazel`, or without a reason is a violation.
//!
//! `tools/xtask` (a standalone workspace) must mirror `[workspace.lints]` completely.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};

use crate::common::{lookup, read_toml, report};

const MEMBER_DIRS: [&str; 2] = ["crates", "plugins"];
const RUST_RULES: [&str; 4] = [
    "rust_library",
    "rust_binary",
    "rust_test",
    "rust_shared_library",
];

pub(crate) fn run(root: &Path) -> Result<()> {
    report("check-crates", &check(root)?)
}

/// Returns one message per violation, each with the file and the fix.
fn check(root: &Path) -> Result<Vec<String>> {
    let workspace = read_toml(&root.join("Cargo.toml"))?;
    let module = std::fs::read_to_string(root.join("MODULE.bazel")).context("MODULE.bazel")?;
    let exempt = lookup(
        &workspace,
        &["workspace", "metadata", "check-crates", "exempt"],
    )
    .and_then(toml::Value::as_table);
    let mut out = Vec::new();
    let mut members = BTreeSet::new();
    for parent in MEMBER_DIRS {
        let entries = match std::fs::read_dir(root.join(parent)) {
            Ok(entries) => entries,
            // A repository may have no plugins (or no crates) at all.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("read {parent}")),
        };
        let mut dirs = entries
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("read {parent}"))?;
        dirs.sort();
        for dir in dirs.iter().filter(|d| d.join("Cargo.toml").is_file()) {
            let rel = format!("{parent}/{}", file_name(dir));
            let exempt = exempt.is_some_and(|e| e.contains_key(&rel));
            check_member(
                root,
                &rel,
                Member {
                    inherits: parent == "crates",
                    exempt,
                },
                &workspace,
                &module,
                &mut out,
            )?;
            members.insert(rel);
        }
    }
    for (rel, reason) in exempt.into_iter().flatten() {
        check_exemption(root, rel, reason, members.contains(rel), &mut out);
    }
    let xtask = read_toml(&root.join("tools/xtask/Cargo.toml"))?;
    for table in ["rust", "clippy"] {
        compare_lints(
            &workspace,
            &xtask,
            table,
            &[],
            "tools/xtask/Cargo.toml",
            &mut out,
        );
    }
    Ok(out)
}

/// How a member directory is checked.
struct Member {
    /// Inherits `[workspace.lints]` (`crates/*`) instead of copying them (`plugins/*`).
    inherits: bool,
    /// Built by `cargo` only: no Bazel files are required.
    exempt: bool,
}

/// An exemption needs a reason, an existing workspace member and no `BUILD.bazel`.
fn check_exemption(
    root: &Path,
    rel: &str,
    reason: &toml::Value,
    is_member: bool,
    out: &mut Vec<String>,
) {
    if !root.join(rel).join("Cargo.toml").is_file() {
        out.push(format!("{rel}: exempt but missing; remove the entry"));
    } else if !is_member {
        out.push(format!(
            "{rel}: exempt but not a workspace member (crates/* or plugins/*); remove the entry"
        ));
    } else if root.join(rel).join("BUILD.bazel").is_file() {
        out.push(format!(
            "{rel}: exempt but has a BUILD.bazel; remove the entry"
        ));
    }
    if reason.as_str().is_none_or(|r| r.trim().is_empty()) {
        out.push(format!("{rel}: exemption needs a reason"));
    }
}

fn check_member(
    root: &Path,
    rel: &str,
    member: Member,
    workspace: &toml::Table,
    module: &str,
    out: &mut Vec<String>,
) -> Result<()> {
    let dir = root.join(rel);
    let manifest = format!("{rel}/Cargo.toml");
    let cargo = read_toml(&dir.join("Cargo.toml"))?;
    if member.inherits {
        let ok =
            lookup(&cargo, &["lints", "workspace"]).and_then(toml::Value::as_bool) == Some(true);
        if !ok {
            out.push(format!("{manifest}: add `[lints]\nworkspace = true`"));
        }
    } else {
        // Plugins may set `unsafe_code` differently (generated bindings), but must declare it.
        compare_lints(workspace, &cargo, "rust", &["unsafe_code"], &manifest, out);
        compare_lints(workspace, &cargo, "clippy", &[], &manifest, out);
    }

    if member.exempt {
        return Ok(());
    }

    let manifest_label = format!("\"//{rel}:Cargo.toml\"");
    if !module.contains(&manifest_label) {
        out.push(format!(
            "MODULE.bazel: add {manifest_label} to the `crate.from_cargo` manifests"
        ));
    }

    let build_path = format!("{rel}/BUILD.bazel");
    let Ok(build) = std::fs::read_to_string(dir.join("BUILD.bazel")) else {
        out.push(format!(
            "{build_path}: missing; add one with `extract_cargo_lints` and a target per crate"
        ));
        return Ok(());
    };
    if !build.contains("extract_cargo_lints(") {
        out.push(format!(
            "{build_path}: declare `extract_cargo_lints(name = \"lints\", ...)`"
        ));
    }
    for rule in rust_rules(&build).filter(|r| !has_lint_config(&r.body)) {
        out.push(format!(
            "{build_path}: `{}` (line {}) has no `lint_config = \":lints\"`",
            rule.name, rule.line
        ));
    }
    for stem in test_stems(&dir.join("tests")).with_context(|| format!("read {rel}/tests"))? {
        if !build.contains(&format!("tests/{stem}.rs")) && !build.contains(&format!("\"{stem}\"")) {
            out.push(format!(
                "{rel}/tests/{stem}.rs: no Bazel test target in {build_path}"
            ));
        }
    }
    Ok(())
}

/// Reports every entry of `workspace.lints.<table>` that `other.lints.<table>` lacks or sets
/// differently; for the names in `any_value` only presence is required.
fn compare_lints(
    workspace: &toml::Table,
    other: &toml::Table,
    table: &str,
    any_value: &[&str],
    label: &str,
    out: &mut Vec<String>,
) {
    let Some(expected) =
        lookup(workspace, &["workspace", "lints", table]).and_then(toml::Value::as_table)
    else {
        return;
    };
    let actual = lookup(other, &["lints", table]).and_then(toml::Value::as_table);
    for (lint, value) in expected {
        let found = actual.and_then(|a| a.get(lint));
        let ok = if any_value.contains(&lint.as_str()) {
            found.is_some()
        } else {
            found == Some(value)
        };
        if !ok {
            out.push(if any_value.contains(&lint.as_str()) {
                format!("{label}: `[lints.{table}]` must declare `{lint}` (any level, like `[workspace.lints.{table}]`)")
            } else {
                format!("{label}: `[lints.{table}]` must contain `{lint} = {value}` like `[workspace.lints.{table}]`")
            });
        }
    }
}

struct Rule<'a> {
    name: &'a str,
    line: usize,
    body: String,
}

/// Splits `BUILD.bazel` into Rust rule calls by line-start rule names and parenthesis depth.
fn rust_rules(build: &str) -> impl Iterator<Item = Rule<'_>> {
    let mut lines = build.lines().enumerate();
    std::iter::from_fn(move || {
        for (i, line) in lines.by_ref() {
            let Some(name) = RUST_RULES
                .iter()
                .find(|r| line.trim_start().starts_with(&format!("{r}(")))
            else {
                continue;
            };
            let mut body = String::new();
            let mut depth = 0isize;
            let mut rest = std::iter::once(line).chain(lines.by_ref().map(|(_, l)| l));
            for l in rest.by_ref() {
                body.push_str(l);
                body.push('\n');
                depth +=
                    l.matches('(').count().cast_signed() - l.matches(')').count().cast_signed();
                if depth <= 0 {
                    break;
                }
            }
            return Some(Rule {
                name,
                line: i + 1,
                body,
            });
        }
        None
    })
}

/// True if the rule body sets `lint_config = ":lints"` on a line that is not a comment.
fn has_lint_config(body: &str) -> bool {
    body.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("lint_config") && l.contains("\":lints\"")
    })
}

fn test_stems(tests: &Path) -> std::io::Result<Vec<String>> {
    let entries = match std::fs::read_dir(tests) {
        Ok(entries) => entries,
        // A crate without integration tests has no `tests/` directory.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut stems = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "rs")
            && let Some(stem) = path.file_stem()
        {
            stems.push(stem.to_string_lossy().into_owned());
        }
    }
    stems.sort();
    Ok(stems)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const ROOT_MANIFEST: &str = r#"
[workspace]
members = ["crates/*", "plugins/*"]
[workspace.lints.rust]
unsafe_code = "forbid"
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
too_many_lines = "warn"
"#;

    const GOOD_BUILD: &str = r#"
load("@rules_rust//cargo:defs.bzl", "extract_cargo_lints")
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

extract_cargo_lints(
    name = "lints",
    manifest = "Cargo.toml",
    workspace = "//:Cargo.toml",
)

rust_library(
    name = "a",
    lint_config = ":lints",
)

rust_test(
    name = "a_test",
    crate = ":a",
    lint_config = ":lints",
)
"#;

    const XTASK_LINTS: &str = r#"
[lints.rust]
unsafe_code = "forbid"
[lints.clippy]
all = { level = "warn", priority = -1 }
too_many_lines = "warn"
"#;

    fn write(root: &Path, path: &str, content: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A repository with one valid crate `crates/a` and one valid plugin `plugins/p`.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write(r, "Cargo.toml", ROOT_MANIFEST);
        write(
            r,
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\n[lints]\nworkspace = true\n",
        );
        write(r, "crates/a/BUILD.bazel", GOOD_BUILD);
        write(
            r,
            "plugins/p/Cargo.toml",
            "[package]\nname = \"p\"\n[lints.rust]\nunsafe_code = \"allow\"\n[lints.clippy]\nall = { level = \"warn\", priority = -1 }\ntoo_many_lines = \"warn\"\n",
        );
        write(r, "plugins/p/BUILD.bazel", GOOD_BUILD);
        write(r, "tools/xtask/Cargo.toml", XTASK_LINTS);
        write(
            r,
            "MODULE.bazel",
            "manifests = [\n \"//crates/a:Cargo.toml\",\n \"//plugins/p:Cargo.toml\",\n]\n",
        );
        dir
    }

    fn violations(dir: &tempfile::TempDir) -> String {
        check(dir.path()).unwrap().join("\n")
    }

    #[test]
    fn valid_repository_passes() {
        assert_eq!(violations(&repo()), "");
    }

    #[test]
    fn crate_without_workspace_lints_fails() {
        let dir = repo();
        write(
            dir.path(),
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\n",
        );
        assert!(violations(&dir).contains("crates/a/Cargo.toml"));
    }

    #[test]
    fn plugin_missing_a_workspace_clippy_lint_fails() {
        let dir = repo();
        write(
            dir.path(),
            "plugins/p/Cargo.toml",
            "[lints.rust]\nunsafe_code = \"allow\"\n[lints.clippy]\nall = { level = \"warn\", priority = -1 }\n",
        );
        let v = violations(&dir);
        assert!(
            v.contains("plugins/p/Cargo.toml") && v.contains("too_many_lines"),
            "{v}"
        );
    }

    #[test]
    fn target_without_lint_config_fails() {
        let dir = repo();
        write(
            dir.path(),
            "crates/a/BUILD.bazel",
            &GOOD_BUILD.replacen("    lint_config = \":lints\",\n", "", 1),
        );
        let v = violations(&dir);
        assert!(
            v.contains("crates/a/BUILD.bazel") && v.contains("rust_library"),
            "{v}"
        );
    }

    #[test]
    fn commented_out_lint_config_fails() {
        let dir = repo();
        write(
            dir.path(),
            "crates/a/BUILD.bazel",
            &GOOD_BUILD.replacen("    lint_config", "    # lint_config", 1),
        );
        let v = violations(&dir);
        assert!(
            v.contains("crates/a/BUILD.bazel") && v.contains("rust_library"),
            "{v}"
        );
    }

    #[test]
    fn build_file_without_extract_cargo_lints_fails() {
        let dir = repo();
        write(
            dir.path(),
            "crates/a/BUILD.bazel",
            "rust_library(\n    name = \"a\",\n    lint_config = \":lints\",\n)\n",
        );
        assert!(violations(&dir).contains("extract_cargo_lints"));
    }

    #[test]
    fn missing_build_file_fails() {
        let dir = repo();
        fs::remove_file(dir.path().join("crates/a/BUILD.bazel")).unwrap();
        assert!(violations(&dir).contains("crates/a/BUILD.bazel"));
    }

    #[test]
    fn integration_test_without_target_fails() {
        let dir = repo();
        write(dir.path(), "crates/a/tests/smoke.rs", "");
        assert!(violations(&dir).contains("crates/a/tests/smoke.rs"));
    }

    #[test]
    fn integration_test_with_target_passes() {
        let dir = repo();
        write(dir.path(), "crates/a/tests/smoke.rs", "");
        write(dir.path(), "crates/a/tests/load.rs", "");
        let build = format!(
            "{GOOD_BUILD}\nrust_test(\n    srcs = [\"tests/smoke.rs\"],\n    lint_config = \":lints\",\n)\n[\n    rust_test(\n        name = n,\n        lint_config = \":lints\",\n    )\n    for n in [\"load\"]\n]\n"
        );
        write(dir.path(), "crates/a/BUILD.bazel", &build);
        assert_eq!(violations(&dir), "");
    }

    #[test]
    fn crate_missing_from_module_manifests_fails() {
        let dir = repo();
        write(
            dir.path(),
            "MODULE.bazel",
            "manifests = [\n \"//crates/a:Cargo.toml\",\n]\n",
        );
        let v = violations(&dir);
        assert!(
            v.contains("MODULE.bazel") && v.contains("//plugins/p:Cargo.toml"),
            "{v}"
        );
    }

    #[test]
    fn xtask_lints_must_mirror_the_workspace() {
        let dir = repo();
        write(
            dir.path(),
            "tools/xtask/Cargo.toml",
            "[lints.rust]\nunsafe_code = \"forbid\"\n[lints.clippy]\nall = { level = \"warn\", priority = -1 }\n",
        );
        let v = violations(&dir);
        assert!(
            v.contains("tools/xtask/Cargo.toml") && v.contains("too_many_lines"),
            "{v}"
        );
    }

    #[test]
    fn plugin_without_the_workspace_rust_lints_fails() {
        let dir = repo();
        write(
            dir.path(),
            "plugins/p/Cargo.toml",
            "[lints.clippy]\nall = { level = \"warn\", priority = -1 }\ntoo_many_lines = \"warn\"\n",
        );
        let v = violations(&dir);
        assert!(
            v.contains("plugins/p/Cargo.toml")
                && v.contains("must declare `unsafe_code` (any level")
                && !v.contains("forbid"),
            "{v}"
        );
    }

    #[test]
    fn missing_member_directory_is_fine() {
        let dir = repo();
        fs::remove_dir_all(dir.path().join("plugins")).unwrap();
        write(
            dir.path(),
            "MODULE.bazel",
            "manifests = [\"//crates/a:Cargo.toml\"]\n",
        );
        assert_eq!(violations(&dir), "");
    }

    #[test]
    fn unreadable_member_directory_is_an_error() {
        let dir = repo();
        fs::remove_dir_all(dir.path().join("plugins")).unwrap();
        write(dir.path(), "plugins", "not a directory");
        assert!(check(dir.path()).is_err());
    }

    /// Adds `crates/e` (cargo only: no `BUILD.bazel`, not in `MODULE.bazel`) to the repository.
    fn with_cargo_only_crate(dir: &tempfile::TempDir, exempt: &str) {
        let r = dir.path();
        write(
            r,
            "crates/e/Cargo.toml",
            "[package]\nname = \"e\"\n[lints]\nworkspace = true\n",
        );
        write(
            r,
            "Cargo.toml",
            &format!("{ROOT_MANIFEST}\n[workspace.metadata.check-crates.exempt]\n{exempt}\n"),
        );
    }

    #[test]
    fn exempt_crate_needs_no_bazel_files() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"crates/e\" = \"cargo only\"");
        assert_eq!(violations(&dir), "");
    }

    #[test]
    fn exempt_crate_must_still_inherit_the_workspace_lints() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"crates/e\" = \"cargo only\"");
        write(
            dir.path(),
            "crates/e/Cargo.toml",
            "[package]\nname = \"e\"\n",
        );
        assert!(violations(&dir).contains("crates/e/Cargo.toml"));
    }

    #[test]
    fn crate_without_exemption_still_needs_bazel_files() {
        let dir = repo();
        with_cargo_only_crate(&dir, "");
        assert!(violations(&dir).contains("crates/e/BUILD.bazel"));
    }

    #[test]
    fn exemption_for_a_crate_with_a_build_file_is_stale() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"crates/a\" = \"cargo only\"");
        assert!(violations(&dir).contains("crates/a: exempt but has a BUILD.bazel"));
    }

    #[test]
    fn exemption_for_a_missing_crate_fails() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"crates/gone\" = \"cargo only\"");
        assert!(violations(&dir).contains("crates/gone: exempt but missing"));
    }

    #[test]
    fn exemption_for_a_directory_outside_the_workspace_members_fails() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"tools/xtask\" = \"cargo only\"");
        assert!(violations(&dir).contains("tools/xtask: exempt but not a workspace member"));
    }

    #[test]
    fn exemption_without_a_reason_fails() {
        let dir = repo();
        with_cargo_only_crate(&dir, "\"crates/e\" = \"\"");
        assert!(violations(&dir).contains("crates/e: exemption needs a reason"));
    }

    #[test]
    fn unreadable_tests_directory_is_an_error() {
        let dir = repo();
        write(dir.path(), "crates/a/tests", "not a directory");
        assert!(check(dir.path()).is_err());
    }
}
