//! `update-opencode-checksums`: sets the `sha256` of the `OpenCode` release archives in `MODULE.bazel`
//! to the digests GitHub reports for the release that `OPENCODE_VERSION` names. Renovate runs it
//! after bumping the version (`postUpgradeTasks` in `renovate.json`).

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use crate::Release;

const MODULE_PATH: &str = "MODULE.bazel";
const REPO: &str = "anomalyco/opencode";
/// `http_archive` name in `MODULE.bazel` and the release asset it downloads.
const ARCHIVES: [(&str, &str); 2] = [
    (
        "opencode_linux_amd64_musl",
        "opencode-linux-x64-musl.tar.gz",
    ),
    (
        "opencode_linux_arm64_musl",
        "opencode-linux-arm64-musl.tar.gz",
    ),
];

pub(crate) fn run(root: &Path) -> Result<()> {
    let path = root.join(MODULE_PATH);
    let module = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let release = crate::release_by_tag(REPO, &format!("v{}", version(&module)?))?;
    std::fs::write(&path, update(&module, &release)?).with_context(|| path.display().to_string())
}

fn version(module: &str) -> Result<&str> {
    module
        .lines()
        .find_map(|l| l.strip_prefix("OPENCODE_VERSION = \"")?.strip_suffix('"'))
        .ok_or_else(|| anyhow!("no OPENCODE_VERSION in {MODULE_PATH}"))
}

/// Replaces the `sha256` line inside the `http_archive` block of each archive.
fn update(module: &str, release: &Release) -> Result<String> {
    let mut lines: Vec<String> = module.lines().map(str::to_owned).collect();
    for (name, asset) in ARCHIVES {
        let sha256 = digest(release, asset)?;
        let start = lines
            .iter()
            .position(|l| l.trim() == format!("name = \"{name}\","))
            .ok_or_else(|| anyhow!("no http_archive {name} in {MODULE_PATH}"))?;
        let line = lines
            .iter_mut()
            .skip(start)
            .take_while(|l| l.trim() != ")")
            .find(|l| l.trim_start().starts_with("sha256 = "))
            .ok_or_else(|| anyhow!("no sha256 in http_archive {name} in {MODULE_PATH}"))?;
        let indent = line.len() - line.trim_start().len();
        *line = format!("{:indent$}sha256 = \"{sha256}\",", "");
    }
    let mut out = lines.join("\n");
    if module.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn digest<'a>(release: &'a Release, asset: &str) -> Result<&'a str> {
    let digest = release
        .assets
        .iter()
        .find(|a| a.name == asset)
        .ok_or_else(|| anyhow!("{REPO} {}: no asset {asset}", release.tag_name))?
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .ok_or_else(|| anyhow!("{REPO} {}: no sha256 digest for {asset}", release.tag_name))?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(
            "{REPO} {}: not a sha256 for {asset}: {digest:?}",
            release.tag_name
        );
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Asset;

    const MODULE: &str = r#"OPENCODE_VERSION = "2.4.1"

http_archive(
    name = "opencode_linux_amd64_musl",
    build_file_content = OPENCODE_BUILD,
    sha256 = "old-amd64",
    url = "https://example.test/opencode-linux-x64-musl.tar.gz",
)

http_archive(
    name = "opencode_linux_arm64_musl",
    build_file_content = OPENCODE_BUILD,
    sha256 = "old-arm64",
    url = "https://example.test/opencode-linux-arm64-musl.tar.gz",
)

http_archive(
    name = "other",
    sha256 = "untouched",
)
"#;

    fn release(assets: &[(&str, Option<&str>)]) -> Release {
        Release {
            tag_name: "v2.4.1".into(),
            assets: assets
                .iter()
                .map(|(name, digest)| Asset {
                    name: (*name).into(),
                    browser_download_url: format!("https://example.test/{name}"),
                    digest: digest.map(Into::into),
                })
                .collect(),
        }
    }

    #[test]
    fn version_is_read_from_the_module() {
        assert_eq!(version(MODULE).unwrap(), "2.4.1");
        assert!(version("").is_err());
    }

    #[test]
    fn checksums_of_both_archives_are_replaced() {
        let amd64 = "a".repeat(64);
        let arm64 = "b".repeat(64);
        let release = release(&[
            (
                "opencode-linux-x64-musl.tar.gz",
                Some(&format!("sha256:{amd64}")),
            ),
            (
                "opencode-linux-arm64-musl.tar.gz",
                Some(&format!("sha256:{arm64}")),
            ),
        ]);
        let want = MODULE
            .replace("old-amd64", &amd64)
            .replace("old-arm64", &arm64);
        assert_eq!(update(MODULE, &release).unwrap(), want);
    }

    #[test]
    fn missing_asset_or_digest_fails() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let only_x64 = release(&[("opencode-linux-x64-musl.tar.gz", Some(&digest))]);
        assert!(update(MODULE, &only_x64).is_err());
        let no_digest = release(&[
            ("opencode-linux-x64-musl.tar.gz", None),
            ("opencode-linux-arm64-musl.tar.gz", Some(&digest)),
        ]);
        assert!(update(MODULE, &no_digest).is_err());
    }

    #[test]
    fn missing_archive_in_the_module_fails() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let release = release(&[
            ("opencode-linux-x64-musl.tar.gz", Some(&digest)),
            ("opencode-linux-arm64-musl.tar.gz", Some(&digest)),
        ]);
        assert!(update("OPENCODE_VERSION = \"2.4.1\"\n", &release).is_err());
    }
}
