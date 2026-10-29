//! Helpers shared by the `check-*` tasks.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Fails with all `violations` listed, or succeeds when there are none.
pub(crate) fn report(task: &str, violations: &[String]) -> Result<()> {
    if violations.is_empty() {
        return Ok(());
    }
    bail!(
        "{task} found {} violation(s):\n{}",
        violations.len(),
        violations.join("\n")
    )
}

/// The given `files`, or every tracked file (`git ls-files`) when none are given.
pub(crate) fn files_or_tracked(root: &Path, files: &[String]) -> Result<Vec<String>> {
    if !files.is_empty() {
        return Ok(files.to_vec());
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .output()
        .context("git ls-files")?;
    if !out.status.success() {
        bail!("git ls-files failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

pub(crate) fn read_toml(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    text.parse().with_context(|| path.display().to_string())
}

/// Follows `path` through nested tables.
pub(crate) fn lookup<'a>(table: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(table.get(*first)?, |v, key| v.get(*key))
}
