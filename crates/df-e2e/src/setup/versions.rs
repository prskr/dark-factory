//! `setup playwright-versions`: the browser server and the test driver must run the same
//! Playwright version: `playwright-core` in `deploy/images/playwright-server/package.json`, the
//! base image tag in `MODULE.bazel` and the driver of the locked `playwright-rs`. Renovate groups
//! all three (`renovate.json`); the check catches a `playwright-rs` release that lags.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

const PACKAGE_JSON: &str = "deploy/images/playwright-server/package.json";
const MODULE_BAZEL: &str = "MODULE.bazel";

/// What the driver install of the runner needs.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Versions {
    pub playwright: String,
    pub node: String,
}

/// A file and its content, so that errors name where a version came from.
pub struct Source<'a> {
    pub file: &'a str,
    pub text: &'a str,
}

/// Checks the Playwright versions of the four sources against each other and returns them with
/// the Node version of the driver.
pub fn check(
    package_json: &Source<'_>,
    module_bazel: &Source<'_>,
    build_rs: &Source<'_>,
    driver_urls: &Source<'_>,
) -> Result<Versions> {
    let server = server_version(package_json)?;
    let image = image_version(module_bazel)?;
    let playwright = rust_const(build_rs, "PLAYWRIGHT_VERSION")?;
    let node = rust_const(driver_urls, "NODE_VERSION")?;
    if server != image || server != playwright {
        bail!(
            "Playwright versions differ: playwright-core in {}: '{server}', base image tag in {} (playwright_base): '{image}', playwright-rs driver in {}: '{playwright}'",
            package_json.file,
            module_bazel.file,
            build_rs.file,
        );
    }
    Ok(Versions { playwright, node })
}

/// Reads the sources below `root` (the repository root; the locked `playwright-rs` through
/// `cargo metadata`) and checks them.
pub fn read(root: &Path) -> Result<Versions> {
    let package_json = read_file(&root.join(PACKAGE_JSON))?;
    let module_bazel = read_file(&root.join(MODULE_BAZEL))?;
    let crate_dir = playwright_rs_dir(root)?;
    let build_rs_path = crate_dir.join("build.rs");
    let driver_urls_path = crate_dir.join("src/build_support/driver_urls.rs");
    let build_rs = read_file(&build_rs_path)?;
    let driver_urls = read_file(&driver_urls_path)?;
    check(
        &Source {
            file: PACKAGE_JSON,
            text: &package_json,
        },
        &Source {
            file: MODULE_BAZEL,
            text: &module_bazel,
        },
        &Source {
            file: &build_rs_path.to_string_lossy(),
            text: &build_rs,
        },
        &Source {
            file: &driver_urls_path.to_string_lossy(),
            text: &driver_urls,
        },
    )
}

fn server_version(source: &Source<'_>) -> Result<String> {
    let json: Value =
        serde_json::from_str(source.text).with_context(|| format!("parse {}", source.file))?;
    json.pointer("/dependencies/playwright-core")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .with_context(|| format!("no playwright-core version in {}", source.file))
}

/// The tag of the `playwright_base` image (`v1.65.0-noble`) without prefix and suffix. `[^)]`
/// keeps the search inside that `pull(…)` block.
fn image_version(source: &Source<'_>) -> Result<String> {
    let re = Regex::new(r#"name = "playwright_base"[^)]*?tag = "v([^"]+)-noble""#)
        .expect("a constant pattern");
    re.captures(source.text)
        .and_then(|captures| captures.get(1))
        .map(|tag| tag.as_str().to_owned())
        .with_context(|| format!("no tag of playwright_base in {}", source.file))
}

/// The value of `const <name>: &str = "…";` in Rust source.
fn rust_const(source: &Source<'_>, name: &str) -> Result<String> {
    let re = Regex::new(&format!(r#"(?m)^const {name}: &str = "([^"]+)";"#))
        .expect("a pattern of a constant name");
    re.captures(source.text)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
        .with_context(|| format!("no {name} in {}", source.file))
}

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

/// Source directory of the locked `playwright-rs` crate.
fn playwright_rs_dir(root: &Path) -> Result<std::path::PathBuf> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_unset| "cargo".to_owned()))
        .args(["metadata", "--quiet", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .context("run cargo metadata")?;
    if !output.status.success() {
        bail!("cargo metadata failed: {}", output.status);
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).context("parse the output of cargo metadata")?;
    let manifest = metadata
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|package| package.get("name").and_then(Value::as_str) == Some("playwright-rs"))
        .and_then(|package| package.get("manifest_path"))
        .and_then(Value::as_str)
        .context("playwright-rs is not in the output of cargo metadata")?;
    Path::new(manifest)
        .parent()
        .map(Path::to_path_buf)
        .context("the manifest of playwright-rs has no directory")
}

#[cfg(test)]
mod tests;
