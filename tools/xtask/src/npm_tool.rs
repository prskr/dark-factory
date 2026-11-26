//! Tools that are published only as npm tarballs (`esbuild`: one package per platform, the binary
//! in `package/bin/`).
//!
//! The pin of such a tool comes from the npm registry instead of GitHub release assets: the latest
//! version of the first platform's package, then the metadata of that version for every platform
//! (`dist.tarball`), and the sha256 of the downloaded tarball (the registry publishes only sha1 and
//! sha512 `integrity`, which `rules_multitool` does not take). Used by `update-tools`.

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;

use crate::update_github_spec::sha256_hex;
use crate::{Binary, PLATFORMS, Platform};

const REGISTRY: &str = "https://registry.npmjs.org";

/// A tool whose binary is in the npm package of its platform.
pub(crate) struct NpmTool {
    pub(crate) name: &'static str,
    /// The package with the binary for a platform.
    pub(crate) package: fn(Platform) -> String,
    /// Path of the binary inside the tarball.
    pub(crate) file: &'static str,
}

pub(crate) const NPM_TOOLS: [NpmTool; 1] = [NpmTool {
    name: "esbuild",
    package: |p| {
        let os = p.os.replace("macos", "darwin");
        let cpu = if p.cpu == "x86_64" { "x64" } else { p.cpu };
        format!("@esbuild/{os}-{cpu}")
    },
    file: "package/bin/esbuild",
}];

/// The part of the registry's metadata of one package version that the pin needs.
#[derive(Debug, Deserialize)]
pub(crate) struct Package {
    version: String,
    dist: Dist,
}

#[derive(Debug, Deserialize)]
struct Dist {
    tarball: String,
}

/// The pins of `tool` for every platform. `metadata` reads the registry document at a URL,
/// `download` the bytes of a tarball.
pub(crate) fn npm_binaries(
    tool: &NpmTool,
    mut metadata: impl FnMut(&str) -> Result<Package>,
    mut download: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<Vec<Binary>> {
    let first = PLATFORMS
        .first()
        .ok_or_else(|| anyhow!("{}: no platform", tool.name))?;
    let latest = metadata(&format!("{REGISTRY}/{}/latest", (tool.package)(*first)))?.version;
    PLATFORMS
        .iter()
        .map(|&p| {
            let package = (tool.package)(p);
            let found = metadata(&format!("{REGISTRY}/{package}/{latest}"))?;
            if found.version != latest {
                bail!(
                    "{}: {package} answered version {}, not {latest}",
                    tool.name,
                    found.version
                );
            }
            let sha256 = sha256_hex(&download(&found.dist.tarball)?);
            Ok(Binary {
                kind: "archive",
                file: Some(tool.file.into()),
                url: found.dist.tarball,
                sha256,
                os: p.os,
                cpu: p.cpu,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str, name: &str) -> Package {
        Package {
            version: version.into(),
            dist: Dist {
                tarball: format!(
                    "{REGISTRY}/{name}/-/{}-{version}.tgz",
                    name.replace('/', "-")
                ),
            },
        }
    }

    /// A registry that knows `latest` for every package and serves the bytes `body`.
    fn pins(tool: &NpmTool, latest: &str) -> Result<Vec<Binary>> {
        npm_binaries(
            tool,
            |url| {
                let path = url.strip_prefix(&format!("{REGISTRY}/")).unwrap();
                let (name, _) = path.rsplit_once('/').unwrap();
                Ok(package(latest, name))
            },
            |_| Ok(b"abc".to_vec()),
        )
    }

    #[test]
    fn esbuild_is_pinned_from_the_package_of_each_platform() {
        let tool = &NPM_TOOLS[0];
        assert_eq!(tool.name, "esbuild");

        let bins = pins(tool, "0.28.2").unwrap();

        let got: Vec<_> = bins.iter().map(|b| (b.os, b.cpu, b.url.as_str())).collect();
        assert_eq!(
            got,
            [
                (
                    "linux",
                    "x86_64",
                    "https://registry.npmjs.org/@esbuild/linux-x64/-/@esbuild-linux-x64-0.28.2.tgz"
                ),
                (
                    "linux",
                    "arm64",
                    "https://registry.npmjs.org/@esbuild/linux-arm64/-/@esbuild-linux-arm64-0.28.2.tgz"
                ),
                (
                    "macos",
                    "arm64",
                    "https://registry.npmjs.org/@esbuild/darwin-arm64/-/@esbuild-darwin-arm64-0.28.2.tgz"
                ),
            ]
        );
        assert!(
            bins.iter()
                .all(|b| b.kind == "archive" && b.file.as_deref() == Some("package/bin/esbuild"))
        );
    }

    #[test]
    fn the_sha256_is_that_of_the_downloaded_tarball() {
        let bins = pins(&NPM_TOOLS[0], "0.28.2").unwrap();

        assert!(bins.iter().all(
            |b| b.sha256 == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ));
    }

    #[test]
    fn every_platform_takes_the_version_of_the_first_one() {
        let mut asked = Vec::new();
        npm_binaries(
            &NPM_TOOLS[0],
            |url| {
                asked.push(url.to_owned());
                Ok(package("0.28.2", "@esbuild/x"))
            },
            |_| Ok(Vec::new()),
        )
        .unwrap();

        assert_eq!(
            asked,
            [
                "https://registry.npmjs.org/@esbuild/linux-x64/latest",
                "https://registry.npmjs.org/@esbuild/linux-x64/0.28.2",
                "https://registry.npmjs.org/@esbuild/linux-arm64/0.28.2",
                "https://registry.npmjs.org/@esbuild/darwin-arm64/0.28.2",
            ]
        );
    }

    #[test]
    fn a_package_that_answers_another_version_fails() {
        let err = npm_binaries(
            &NPM_TOOLS[0],
            |url| {
                Ok(package(
                    if url.ends_with("/latest") {
                        "0.28.2"
                    } else {
                        "0.28.1"
                    },
                    "@esbuild/x",
                ))
            },
            |_| Ok(Vec::new()),
        )
        .unwrap_err();

        assert!(err.to_string().contains("not 0.28.2"), "{err}");
    }

    #[test]
    fn a_failing_download_fails_the_pin() {
        let err = npm_binaries(
            &NPM_TOOLS[0],
            |_| Ok(package("0.28.2", "@esbuild/x")),
            |url| bail!("GET {url} failed"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("failed"), "{err}");
    }
}
