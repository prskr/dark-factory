//! Checks that declared dependency requirements are not already behind the latest release.
//!
//! Usage: `cargo xtask outdated [--since <git-rev>]`.
//!
//! A dependency is outdated when the newest stable, non-yanked version on crates.io does not
//! satisfy its requirement (for example `tonic = "0.12"` while 0.14 is out). Semver-compatible
//! updates are the business of `cargo update`, not of this check.
//!
//! With `--since <rev>` only dependencies that are new or changed compared to `<rev>` are checked;
//! the pre-commit hook uses `--since HEAD` so that a release of some unrelated crate never blocks a
//! commit. Deliberate holds are listed in the root `Cargo.toml`:
//!
//! ```toml
//! [workspace.metadata.outdated.ignore]
//! some-crate = "why it stays on the old version"
//! ```
//!
//! If crates.io cannot be reached the check warns and passes, so commits work offline.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, bail};
use semver::{Version, VersionReq};

const ROOT_MANIFEST: &str = "Cargo.toml";
const MEMBER_DIRS: [&str; 2] = ["crates", "plugins"];
const INDEX_URL: &str = "https://index.crates.io";

/// A registry dependency as declared in a manifest.
#[derive(Debug, Clone, PartialEq)]
struct Dep {
    /// Crate name on crates.io (honors `package = "..."`).
    krate: String,
    requirement: String,
    /// The raw TOML value, used to detect changes against `--since`.
    spec: toml::Value,
}

pub fn run(since: Option<&str>) -> Result<()> {
    let root: toml::Table = std::fs::read_to_string(ROOT_MANIFEST)?.parse()?;
    let ignore = ignore_list(&root);

    let mut deps = declared_deps(&root, &["workspace", "dependencies"]);
    for manifest in member_manifests()? {
        let table: toml::Table = std::fs::read_to_string(&manifest)?.parse()?;
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            deps.extend(declared_deps(&table, &[section]));
        }
    }

    if let Some(rev) = since {
        let old = old_manifest(rev, ROOT_MANIFEST);
        let old_deps = old.map(|t| declared_deps(&t, &["workspace", "dependencies"]));
        if let Some(old_deps) = old_deps {
            deps.retain(|name, dep| old_deps.get(name) != Some(dep));
        }
    }

    let mut outdated = Vec::new();
    for (name, dep) in &deps {
        if let Some(reason) = ignore.get(&dep.krate).or_else(|| ignore.get(name)) {
            eprintln!("skipping {name}: {reason}");
            continue;
        }
        match latest_stable(&dep.krate) {
            Ok(Some(latest)) => {
                if is_outdated(&dep.requirement, &latest)? {
                    outdated.push(format!(
                        "{name}: requirement \"{}\" excludes the latest release {latest}",
                        dep.requirement
                    ));
                }
            }
            Ok(None) => eprintln!("warning: no stable release of {} found, skipped", dep.krate),
            Err(e) => eprintln!("warning: could not check {}: {e:#}", dep.krate),
        }
    }

    if outdated.is_empty() {
        println!("dependencies are up to date ({} checked)", deps.len());
        return Ok(());
    }
    bail!(
        "outdated dependencies:\n  {}\nUpgrade them, or add a reason to [workspace.metadata.outdated.ignore] in {ROOT_MANIFEST}",
        outdated.join("\n  ")
    )
}

fn ignore_list(root: &toml::Table) -> BTreeMap<String, String> {
    root.get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("outdated"))
        .and_then(|o| o.get("ignore"))
        .and_then(toml::Value::as_table)
        .map(|t| {
            t.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str().unwrap_or("no reason given").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Registry dependencies in the table at `path`, keyed by the name used in the manifest.
/// Path, git and `workspace = true` entries are skipped.
fn declared_deps(root: &toml::Table, path: &[&str]) -> BTreeMap<String, Dep> {
    let mut node = Some(root);
    for key in path {
        node = node
            .and_then(|n| n.get(*key))
            .and_then(toml::Value::as_table);
    }
    let mut out = BTreeMap::new();
    for (name, spec) in node.into_iter().flatten() {
        let (requirement, package) = match spec {
            toml::Value::String(req) => (req.clone(), None),
            toml::Value::Table(t) => {
                if t.contains_key("path") || t.contains_key("git") || t.contains_key("workspace") {
                    continue;
                }
                let Some(req) = t.get("version").and_then(toml::Value::as_str) else {
                    continue;
                };
                (
                    req.to_string(),
                    t.get("package").and_then(toml::Value::as_str),
                )
            }
            _ => continue,
        };
        out.insert(
            name.clone(),
            Dep {
                krate: package.unwrap_or(name).to_string(),
                requirement,
                spec: spec.clone(),
            },
        );
    }
    out
}

fn member_manifests() -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    for dir in MEMBER_DIRS {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries {
            let manifest = entry?.path().join("Cargo.toml");
            if manifest.is_file() {
                out.push(manifest);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The manifest at `rev`, or `None` if it did not exist there.
fn old_manifest(rev: &str, path: &str) -> Option<toml::Table> {
    let out = Command::new("git")
        .args(["show", &format!("{rev}:{path}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()?.parse().ok()
}

/// Path of a crate in the sparse registry index.
fn index_path(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    match n.len() {
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

fn latest_stable(krate: &str) -> Result<Option<Version>> {
    let url = format!("{INDEX_URL}/{}", index_path(krate));
    let body = ureq::get(&url)
        .header("User-Agent", "dark-factory-xtask")
        .call()
        .with_context(|| format!("GET {url}"))?
        .body_mut()
        .read_to_string()?;
    Ok(latest_stable_from_index(&body))
}

#[derive(serde::Deserialize)]
struct IndexLine {
    vers: String,
    yanked: bool,
}

/// Newest non-yanked, non-prerelease version in a sparse-index response (one JSON object per line).
fn latest_stable_from_index(body: &str) -> Option<Version> {
    body.lines()
        .filter_map(|l| serde_json::from_str::<IndexLine>(l).ok())
        .filter(|l| !l.yanked)
        .filter_map(|l| Version::parse(&l.vers).ok())
        .filter(|v| v.pre.is_empty())
        .max()
}

fn is_outdated(requirement: &str, latest: &Version) -> Result<bool> {
    let req = VersionReq::parse(requirement)
        .with_context(|| format!("invalid version requirement \"{requirement}\""))?;
    Ok(!req.matches(latest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(manifest: &str) -> BTreeMap<String, Dep> {
        declared_deps(&manifest.parse().unwrap(), &["dependencies"])
    }

    #[test]
    fn index_paths_follow_the_registry_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("syn"), "3/s/syn");
        assert_eq!(index_path("tonic"), "to/ni/tonic");
        assert_eq!(index_path("Serde-JSON"), "se/rd/serde-json");
    }

    #[test]
    fn latest_ignores_yanked_and_prerelease_versions() {
        let body = r#"{"vers":"0.14.5","yanked":false}
{"vers":"0.14.6","yanked":true}
{"vers":"0.15.0-rc.1","yanked":false}
{"vers":"0.14.2","yanked":false}"#;
        assert_eq!(latest_stable_from_index(body), Some(Version::new(0, 14, 5)));
        assert_eq!(latest_stable_from_index(""), None);
    }

    #[test]
    fn requirement_is_outdated_only_when_the_latest_release_is_excluded() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(is_outdated("0.12", &v("0.14.6")).unwrap());
        assert!(is_outdated("0.7", &v("0.8.0")).unwrap());
        assert!(is_outdated("1", &v("2.0.1")).unwrap());
        assert!(!is_outdated("1", &v("1.9.0")).unwrap());
        assert!(!is_outdated("0.14", &v("0.14.6")).unwrap());
        assert!(!is_outdated("^3.4.0", &v("3.9.1")).unwrap());
        assert!(is_outdated("not a version", &v("1.0.0")).is_err());
    }

    #[test]
    fn only_registry_dependencies_are_collected() {
        let d = deps(
            r#"
[dependencies]
plain = "1"
detailed = { version = "0.4", features = ["x"] }
renamed = { package = "real-name", version = "2" }
local = { path = "../local" }
shared = { workspace = true }
vcs = { git = "https://example.test/x.git" }
no_version = { features = ["x"] }
"#,
        );
        assert_eq!(
            d.keys().map(String::as_str).collect::<Vec<_>>(),
            ["detailed", "plain", "renamed"]
        );
        assert_eq!(d["renamed"].krate, "real-name");
        assert_eq!(d["detailed"].requirement, "0.4");
    }

    #[test]
    fn ignore_list_reads_workspace_metadata() {
        let root: toml::Table = r#"
[workspace.metadata.outdated.ignore]
old-crate = "blocked by upstream"
"#
        .parse()
        .unwrap();
        assert_eq!(ignore_list(&root)["old-crate"], "blocked by upstream");
        assert!(ignore_list(&toml::Table::new()).is_empty());
    }

    #[test]
    fn changed_specs_are_detected_by_comparing_values() {
        let old = deps("[dependencies]\na = \"1\"\nb = \"2\"\n");
        let new = deps("[dependencies]\na = \"1\"\nb = \"3\"\nc = \"1\"\n");
        let changed: Vec<_> = new
            .iter()
            .filter(|(n, d)| old.get(*n) != Some(d))
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(changed, ["b", "c"]);
    }
}
