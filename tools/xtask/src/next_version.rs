//! `cargo xtask next-version`: prints the next release version `YYYY.M.N` (openspec change
//! `release-versioning`, design D1).
//!
//! A version is used when a git tag or a GitHub Release (draft or published) `v<version>` exists.
//! `N` is one more than the highest `N` used in the current UTC month, or `0`.

use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Utc};
use serde::Deserialize;

#[expect(clippy::print_stdout, reason = "CLI output")]
pub(crate) fn run() -> Result<()> {
    let mut used = lines(Command::new("git").args(["tag", "--list", "v*"]), "git tag")?;
    let releases = output(
        Command::new("gh").args(["release", "list", "--json", "tagName", "--limit", "1000"]),
        "gh release list",
    )?;
    used.extend(release_tags(&releases)?);
    let (year, month) = utc_year_month();
    println!("{}", next_version(&used, year, month)?);
    Ok(())
}

/// The version after the highest used `N` of `year.month`; names that are not `v<year>.<month>.<n>` are ignored.
/// Fails when the highest `N` is `u32::MAX`.
pub(crate) fn next_version<S: AsRef<str>>(used: &[S], year: u32, month: u32) -> Result<String> {
    let highest = used
        .iter()
        .filter_map(|name| parse(name.as_ref()))
        .filter(|&(y, m, _)| (y, m) == (year, month))
        .map(|(_, _, n)| n)
        .max();
    let next = match highest {
        Some(n) => n
            .checked_add(1)
            .with_context(|| format!("no release counter after {year}.{month}.{n}"))?,
        None => 0,
    };
    Ok(format!("{year}.{month}.{next}"))
}

fn parse(name: &str) -> Option<(u32, u32, u32)> {
    let mut parts = name.strip_prefix('v')?.split('.');
    let mut number = || {
        let part = parts.next()?;
        // reject signs and anything else `u32::from_str` would accept
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}

#[derive(Deserialize)]
struct ReleaseTag {
    #[serde(rename = "tagName")]
    tag_name: String,
}

fn release_tags(json: &str) -> Result<Vec<String>> {
    let tags: Vec<ReleaseTag> = serde_json::from_str(json).context("parse gh release list")?;
    Ok(tags.into_iter().map(|t| t.tag_name).collect())
}

fn utc_year_month() -> (u32, u32) {
    let now = Utc::now();
    (now.year().unsigned_abs(), now.month())
}

fn output(cmd: &mut Command, what: &str) -> Result<String> {
    let out = cmd.output().with_context(|| format!("run {what}"))?;
    if !out.status.success() {
        bail!("{what} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn lines(cmd: &mut Command, what: &str) -> Result<Vec<String>> {
    Ok(output(cmd, what)?.lines().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_release_of_a_month_is_zero() {
        assert_eq!(next_version::<&str>(&[], 2026, 11).unwrap(), "2026.11.0");
    }

    #[test]
    fn counts_up_within_a_month() {
        assert_eq!(
            next_version(&["v2026.11.0", "v2026.11.1"], 2026, 11).unwrap(),
            "2026.11.2"
        );
    }

    #[test]
    fn counter_is_compared_numerically_not_lexically() {
        assert_eq!(
            next_version(&["v2026.11.9", "v2026.11.10"], 2026, 11).unwrap(),
            "2026.11.11"
        );
    }

    #[test]
    fn new_month_starts_at_zero() {
        assert_eq!(
            next_version(&["v2026.11.4"], 2026, 12).unwrap(),
            "2026.12.0"
        );
    }

    #[test]
    fn release_without_tag_still_counts() {
        let mut used = vec!["v2026.11.1".to_owned()];
        used.extend(release_tags(r#"[{"tagName":"v2026.11.2"}]"#).unwrap());
        assert_eq!(next_version(&used, 2026, 11).unwrap(), "2026.11.3");
    }

    #[test]
    fn counter_at_the_maximum_is_an_error() {
        let error = next_version(&["v2026.11.4294967295"], 2026, 11).unwrap_err();
        assert!(error.to_string().contains("2026.11.4294967295"), "{error}");
    }

    #[test]
    fn ignores_unparsable_names_and_other_months() {
        let used = [
            "v1.2.3-rc1",
            "foo",
            "v2026.11",
            "v2026.11.1.2",
            "v2026.11.+3",
            "v2025.11.7",
        ];
        assert_eq!(next_version(&used, 2026, 11).unwrap(), "2026.11.0");
    }
}
