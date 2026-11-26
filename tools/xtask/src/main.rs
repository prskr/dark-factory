//! Repository tasks (`cargo xtask <task>`, alias in `.cargo/config.toml`).
//!
//! - `update-tools [--check]` generates `multitool.lock.json` from the latest upstream releases
//!   (GitHub release assets, the npm registry for tools that are published only as npm tarballs,
//!   see `npm_tool.rs`). `--check` fails if the committed lock differs from what upstream would
//!   produce now. Set `GITHUB_TOKEN` to avoid API rate limits.
//! - `update-opencode-checksums` sets the checksums of the `OpenCode` archives in `MODULE.bazel` for
//!   `OPENCODE_VERSION` (see `update_opencode_checksums.rs`).
//! - `next-version` prints the next release version `YYYY.M.N` (see `next_version.rs`).
//! - `check-crates` fails when a workspace member escapes the lint, build or test setup (see
//!   `check_crates.rs`).
//! - `check-file-length [<file>...]` fails for source files over the line limit (see
//!   `check_file_length.rs`).
//! - `check-test-names [<file>...]` fails for tests named after review rounds or findings (see
//!   `check_test_names.rs`).
//! - `outdated [--since <rev>]` fails if a declared dependency requirement excludes the latest
//!   crates.io release (see `outdated.rs`).
//! - `update-github-spec [--commit <sha> | --check]` writes the extract of GitHub's REST API
//!   description the GitHub plugin is tested against, from the commit recorded in the extract or
//!   `--commit`. `--check` fails if the committed extract is stale (see `update_github_spec.rs`).

mod check_crates;
mod check_file_length;
mod check_test_names;
mod common;
mod next_version;
mod npm_tool;
mod outdated;
mod update_github_spec;
mod update_opencode_checksums;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use npm_tool::{NPM_TOOLS, npm_binaries};
use serde::{Deserialize, Serialize};
use update_github_spec::Mode;

const LOCK_PATH: &str = "multitool.lock.json";
/// The most an npm tarball of a pinned tool may be (a platform package of `esbuild` is about
/// 5 MB); ureq's default read limit is 10 MB.
const MAX_TARBALL_BYTES: u64 = 64 * 1024 * 1024;
const SCHEMA: &str =
    "https://raw.githubusercontent.com/theoremlp/rules_multitool/main/lockfile.schema.json";

#[derive(Clone, Copy)]
struct Platform {
    os: &'static str,
    cpu: &'static str,
}

/// Intel macOS is not supported: upstream tools stopped shipping builds for it (cargo-deny 0.21).
const PLATFORMS: [Platform; 3] = [
    Platform {
        os: "linux",
        cpu: "x86_64",
    },
    Platform {
        os: "linux",
        cpu: "arm64",
    },
    Platform {
        os: "macos",
        cpu: "arm64",
    },
];

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Binary {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    url: String,
    sha256: String,
    os: &'static str,
    cpu: &'static str,
}

/// Maps `(version without leading "v", platform)` to `(asset name, path of the binary inside the archive)`.
/// An empty path means the asset is the bare binary (`kind: file`).
type AssetFn = fn(&str, Platform) -> (String, String);

struct GithubTool {
    name: &'static str,
    repo: &'static str,
    asset: AssetFn,
}

/// Tools pinned for the Linux platforms only (no macOS build upstream; used in Linux image builds).
const LINUX_ONLY: [&str; 1] = ["patchelf"];

const TOOLS: [GithubTool; 18] = [
    GithubTool {
        name: "patchelf",
        repo: "NixOS/patchelf",
        asset: |v, p| {
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            (format!("patchelf-{v}-{cpu}.tar.gz"), "bin/patchelf".into())
        },
    },
    GithubTool {
        name: "shellcheck",
        repo: "koalaman/shellcheck",
        asset: |v, p| {
            let os = p.os.replace("macos", "darwin");
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            (
                format!("shellcheck-v{v}.{os}.{cpu}.tar.gz"),
                format!("shellcheck-v{v}/shellcheck"),
            )
        },
    },
    GithubTool {
        name: "lefthook",
        repo: "evilmartians/lefthook",
        asset: |v, p| {
            let os = if p.os == "macos" { "MacOS" } else { "Linux" };
            (format!("lefthook_{v}_{os}_{}", p.cpu), String::new())
        },
    },
    GithubTool {
        name: "hadolint",
        repo: "hadolint/hadolint",
        asset: |_, p| (format!("hadolint-{}-{}", p.os, p.cpu), String::new()),
    },
    GithubTool {
        name: "typos",
        repo: "crate-ci/typos",
        asset: |v, p| {
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            let target = match p.os {
                "macos" => format!("{cpu}-apple-darwin"),
                _ => format!("{cpu}-unknown-linux-musl"),
            };
            (format!("typos-v{v}-{target}.tar.gz"), "typos".into())
        },
    },
    GithubTool {
        name: "cargo-machete",
        repo: "bnjbvr/cargo-machete",
        asset: |v, p| {
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            let target = if p.os == "macos" {
                format!("{cpu}-apple-darwin")
            } else {
                format!("{cpu}-unknown-linux-musl")
            };
            let stem = format!("cargo-machete-v{v}-{target}");
            (format!("{stem}.tar.gz"), format!("{stem}/cargo-machete"))
        },
    },
    GithubTool {
        name: "kube-linter",
        repo: "stackrox/kube-linter",
        asset: |_, p| {
            let os = p.os.replace("macos", "darwin");
            let suffix = if p.cpu == "arm64" { "_arm64" } else { "" };
            (format!("kube-linter-{os}{suffix}"), String::new())
        },
    },
    GithubTool {
        name: "kind",
        repo: "kubernetes-sigs/kind",
        asset: |_, p| {
            let os = p.os.replace("macos", "darwin");
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            (format!("kind-{os}-{arch}"), String::new())
        },
    },
    GithubTool {
        name: "kubeconform",
        repo: "yannh/kubeconform",
        asset: |_, p| {
            let os = p.os.replace("macos", "darwin");
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            (
                format!("kubeconform-{os}-{arch}.tar.gz"),
                "kubeconform".into(),
            )
        },
    },
    GithubTool {
        name: "kingfisher",
        repo: "mongodb/kingfisher",
        asset: |_, p| {
            let os = p.os.replace("macos", "darwin");
            let cpu = if p.cpu == "x86_64" { "x64" } else { p.cpu };
            (format!("kingfisher-{os}-{cpu}.tgz"), "kingfisher".into())
        },
    },
    GithubTool {
        name: "helm-unittest",
        repo: "helm-unittest/helm-unittest",
        // Standalone `untt` binary; works without the helm plugin machinery.
        asset: |v, p| {
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            (
                format!("helm-unittest-{}-{arch}-{v}.tgz", p.os),
                format!("untt-{}-{arch}", p.os),
            )
        },
    },
    GithubTool {
        name: "yq",
        repo: "mikefarah/yq",
        asset: |_, p| {
            let os = p.os.replace("macos", "darwin");
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            (format!("yq_{os}_{arch}.tar.gz"), format!("yq_{os}_{arch}"))
        },
    },
    GithubTool {
        name: "cargo-deny",
        repo: "EmbarkStudios/cargo-deny",
        asset: |v, p| {
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            let triple = match p.os {
                "macos" => format!("{cpu}-apple-darwin"),
                _ => format!("{cpu}-unknown-linux-musl"),
            };
            (
                format!("cargo-deny-{v}-{triple}.tar.gz"),
                format!("cargo-deny-{v}-{triple}/cargo-deny"),
            )
        },
    },
    GithubTool {
        name: "cargo-nextest",
        repo: "nextest-rs/nextest",
        // The tag is `cargo-nextest-<version>`. Static musl builds on Linux, one universal build on
        // macOS; the archive holds the bare `cargo-nextest` binary.
        asset: |v, p| {
            let cpu = if p.cpu == "arm64" { "aarch64" } else { p.cpu };
            let target = match p.os {
                "macos" => "universal-apple-darwin".to_owned(),
                _ => format!("{cpu}-unknown-linux-musl"),
            };
            (
                format!("cargo-nextest-{v}-{target}.tar.gz"),
                "cargo-nextest".into(),
            )
        },
    },
    GithubTool {
        name: "jq",
        repo: "jqlang/jq",
        asset: |_, p| {
            let os = p.os;
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            (format!("jq-{os}-{arch}"), String::new())
        },
    },
    GithubTool {
        name: "tailwindcss",
        repo: "tailwindlabs/tailwindcss",
        asset: |_, p| {
            let cpu = if p.cpu == "x86_64" { "x64" } else { p.cpu };
            // glibc build: the musl one needs a musl loader that glibc hosts (CI runners, dev VMs) lack
            (format!("tailwindcss-{}-{cpu}", p.os), String::new())
        },
    },
    GithubTool {
        name: "swc",
        repo: "swc-project/swc",
        // Standalone CLI, bare executables; static musl builds on Linux.
        asset: |_, p| {
            let (os, cpu) = match (p.os, p.cpu) {
                ("linux", "x86_64") => ("linux", "x64-musl"),
                ("linux", _) => ("linux", "arm64-musl"),
                (_, "x86_64") => ("darwin", "x64"),
                _ => ("darwin", "arm64"),
            };
            (format!("swc-{os}-{cpu}"), String::new())
        },
    },
    GithubTool {
        name: "hugo",
        repo: "gohugoio/hugo",
        // Standard build (0.170 dropped the extended one; Hextra needs no Sass). macOS ships only a
        // universal installer package (kind `pkg`, expanded by `pkgutil`); the binary is
        // `Payload/hugo` inside it.
        asset: |v, p| match (p.os, p.cpu) {
            ("macos", _) => (
                format!("hugo_{v}_darwin-universal.pkg"),
                "Payload/hugo".into(),
            ),
            (_, cpu) => {
                let arch = if cpu == "arm64" { "arm64" } else { "amd64" };
                (format!("hugo_{v}_linux-{arch}.tar.gz"), "hugo".into())
            }
        },
    },
];

fn github_binaries(tool: &GithubTool, release: &Release) -> Result<Vec<Binary>> {
    // The tag is `v1.2.3`, or `<tool>-1.2.3` where one repository releases several tools.
    let tag = &release.tag_name;
    let version = tag
        .strip_prefix(tool.name)
        .map_or(tag.as_str(), |rest| rest.trim_start_matches('-'))
        .trim_start_matches('v');
    PLATFORMS
        .iter()
        .filter(|p| p.os == "linux" || !LINUX_ONLY.contains(&tool.name))
        .map(|&p| {
            let (name, file) = (tool.asset)(version, p);
            let asset = release
                .assets
                .iter()
                .find(|a| a.name == name)
                .ok_or_else(|| anyhow!("{}: no asset {name}", tool.name))?;
            let sha256 = asset
                .digest
                .as_deref()
                .and_then(|d| d.strip_prefix("sha256:"))
                .ok_or_else(|| anyhow!("{}: missing digest for {name}", tool.name))?;
            Ok(Binary {
                kind: if file.is_empty() {
                    "file"
                } else if Path::new(&name).extension().is_some_and(|e| e == "pkg") {
                    "pkg"
                } else {
                    "archive"
                },
                file: Some(file).filter(|f| !f.is_empty()),
                url: asset.browser_download_url.clone(),
                sha256: sha256.into(),
                os: p.os,
                cpu: p.cpu,
            })
        })
        .collect()
}

/// helm is hosted on get.helm.sh and publishes `<archive>.sha256` sidecar files.
fn helm_binaries(
    version: &str,
    mut fetch_sha: impl FnMut(&str) -> Result<String>,
) -> Result<Vec<Binary>> {
    let version = format!("v{}", version.trim_start_matches('v'));
    PLATFORMS
        .iter()
        .map(|&p| {
            let os = p.os.replace("macos", "darwin");
            let arch = if p.cpu == "arm64" { "arm64" } else { "amd64" };
            let url = format!("https://get.helm.sh/helm-{version}-{os}-{arch}.tar.gz");
            let sha256 = parse_sha_file(&fetch_sha(&format!("{url}.sha256"))?)?;
            Ok(Binary {
                kind: "archive",
                file: Some(format!("{os}-{arch}/helm")),
                url,
                sha256,
                os: p.os,
                cpu: p.cpu,
            })
        })
        .collect()
}

/// Accepts `<hash>` or `<hash>  <filename>`.
fn parse_sha_file(body: &str) -> Result<String> {
    let hash = body.split_whitespace().next().unwrap_or_default();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("not a sha256: {body:?}");
    }
    Ok(hash.to_ascii_lowercase())
}

fn render(tools: BTreeMap<&str, Vec<Binary>>) -> Result<String> {
    let mut doc = serde_json::Map::new();
    doc.insert("$schema".into(), SCHEMA.into());
    for (name, binaries) in tools {
        doc.insert(name.into(), serde_json::json!({ "binaries": binaries }));
    }
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
}

/// An agent that trusts the platform's root certificates (on Linux the system bundle, honoring
/// `SSL_CERT_FILE`) instead of ureq's built-in Mozilla roots, so it works behind a
/// TLS-intercepting proxy whose CA is installed on the machine.
fn http_agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .build()
        .new_agent()
}

fn get(url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let mut req = http_agent()
        .get(url)
        .header("User-Agent", "dark-factory-xtask");
    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && url.starts_with("https://api.github.com/")
    {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    req.call().with_context(|| format!("GET {url}"))
}

fn release_by_tag(repo: &str, tag: &str) -> Result<Release> {
    Ok(get(&format!(
        "https://api.github.com/repos/{repo}/releases/tags/{tag}"
    ))?
    .body_mut()
    .read_json()?)
}

fn latest_release(repo: &str) -> Result<Release> {
    Ok(get(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))?
    .body_mut()
    .read_json()?)
}

fn generate() -> Result<String> {
    let mut tools = BTreeMap::new();
    for tool in &TOOLS {
        tools.insert(
            tool.name,
            github_binaries(tool, &latest_release(tool.repo)?)?,
        );
    }
    let helm = latest_release("helm/helm")?;
    tools.insert(
        "helm",
        helm_binaries(&helm.tag_name, |url| {
            Ok(get(url)?.body_mut().read_to_string()?)
        })?,
    );
    for tool in &NPM_TOOLS {
        tools.insert(
            tool.name,
            npm_binaries(
                tool,
                |url| Ok(get(url)?.body_mut().read_json()?),
                |url| {
                    Ok(get(url)?
                        .body_mut()
                        .with_config()
                        .limit(MAX_TARBALL_BYTES)
                        .read_to_vec()?)
                },
            )?,
        );
    }
    render(tools)
}

/// Fails when the committed lock `committed` differs from the freshly `generated` one.
fn check_lock(committed: &str, generated: &str) -> Result<()> {
    if committed != generated {
        bail!("{LOCK_PATH} is stale; run `cargo xtask update-tools`");
    }
    Ok(())
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["update-tools"] => std::fs::write(LOCK_PATH, generate()?)?,
        ["update-tools", "--check"] => {
            check_lock(&std::fs::read_to_string(LOCK_PATH)?, &generate()?)?;
        }
        ["outdated"] => outdated::run(None)?,
        ["outdated", "--since", rev] => outdated::run(Some(rev))?,
        ["update-opencode-checksums"] => update_opencode_checksums::run(Path::new("."))?,
        ["next-version"] => next_version::run()?,
        ["check-crates"] => check_crates::run(Path::new("."))?,
        ["check-file-length", files @ ..] => check_file_length::run(Path::new("."), &owned(files))?,
        ["check-test-names", files @ ..] => check_test_names::run(Path::new("."), &owned(files))?,
        ["update-github-spec"] => update_github_spec::run(Path::new("."), Mode::Update)?,
        ["update-github-spec", "--check"] => update_github_spec::run(Path::new("."), Mode::Check)?,
        ["update-github-spec", "--commit", commit] => {
            update_github_spec::run(Path::new("."), Mode::Commit(commit))?;
        }
        _ => bail!(
            "usage: cargo xtask update-tools [--check] | update-opencode-checksums | outdated [--since <rev>] | check-crates | next-version\n       | check-file-length [<file>...]\n       | check-test-names [<file>...]\n       | update-github-spec [--commit <sha> | --check]"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, names: &[&str], digest: Option<&str>) -> Release {
        Release {
            tag_name: tag.into(),
            assets: names
                .iter()
                .map(|n| Asset {
                    name: (*n).into(),
                    browser_download_url: format!("https://example.test/{n}"),
                    digest: digest.map(Into::into),
                })
                .collect(),
        }
    }

    fn tool(name: &str) -> &'static GithubTool {
        TOOLS.iter().find(|t| t.name == name).unwrap()
    }

    const ALL_KINGFISHER: [&str; 4] = [
        "kingfisher-darwin-arm64.tgz",
        "kingfisher-darwin-x64.tgz",
        "kingfisher-linux-arm64.tgz",
        "kingfisher-linux-x64.tgz",
    ];

    #[test]
    fn http_agent_trusts_the_platform_root_certificates() {
        let agent = http_agent();
        assert!(matches!(
            agent.config().tls_config().root_certs(),
            ureq::tls::RootCerts::PlatformVerifier
        ));
    }

    #[test]
    fn patchelf_is_pinned_for_linux_only() {
        let names = [
            "patchelf-0.19.3-x86_64.tar.gz",
            "patchelf-0.19.3-aarch64.tar.gz",
        ];
        let r = release("0.19.3", &names, Some("sha256:abc"));
        let bins = github_binaries(tool("patchelf"), &r).unwrap();
        assert_eq!(bins.len(), 2);
        assert!(bins.iter().all(|b| b.os == "linux"
            && b.kind == "archive"
            && b.file.as_deref() == Some("bin/patchelf")));
    }

    #[test]
    fn tools_are_pinned_for_linux_and_apple_silicon_only() {
        let platforms: Vec<_> = PLATFORMS.iter().map(|p| (p.os, p.cpu)).collect();
        assert_eq!(
            platforms,
            [("linux", "x86_64"), ("linux", "arm64"), ("macos", "arm64")]
        );
    }

    #[test]
    fn github_maps_every_platform() {
        let r = release("v2.13.0", &ALL_KINGFISHER, Some("sha256:abc"));
        let bins = github_binaries(tool("kingfisher"), &r).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(bins.iter().any(|b| b.os == "macos"
            && b.cpu == "arm64"
            && b.url.ends_with("kingfisher-darwin-arm64.tgz")
            && b.sha256 == "abc"));
    }

    #[test]
    fn github_fails_without_digest() {
        let r = release("v2.13.0", &ALL_KINGFISHER, None);
        assert!(github_binaries(tool("kingfisher"), &r).is_err());
    }

    #[test]
    fn github_fails_on_missing_asset() {
        let r = release("v2.13.0", &ALL_KINGFISHER[..3], Some("sha256:abc"));
        assert!(github_binaries(tool("kingfisher"), &r).is_err());
    }

    #[test]
    fn archive_paths_embed_version() {
        let (name, file) = (tool("cargo-deny").asset)("0.20.2", PLATFORMS[2]);
        assert_eq!(name, "cargo-deny-0.20.2-aarch64-apple-darwin.tar.gz");
        assert_eq!(file, "cargo-deny-0.20.2-aarch64-apple-darwin/cargo-deny");
        let (_, file) = (tool("shellcheck").asset)("0.11.0", PLATFORMS[0]);
        assert_eq!(file, "shellcheck-v0.11.0/shellcheck");
    }

    #[test]
    fn nextest_version_comes_from_its_prefixed_tag() {
        let names = [
            "cargo-nextest-0.9.152-x86_64-unknown-linux-musl.tar.gz",
            "cargo-nextest-0.9.152-aarch64-unknown-linux-musl.tar.gz",
            "cargo-nextest-0.9.152-universal-apple-darwin.tar.gz",
        ];
        let r = release("cargo-nextest-0.9.152", &names, Some("sha256:abc"));
        let bins = github_binaries(tool("cargo-nextest"), &r).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(
            bins.iter()
                .all(|b| b.file.as_deref() == Some("cargo-nextest"))
        );
        assert!(bins.iter().any(|b| {
            b.os == "macos"
                && b.url
                    .ends_with("cargo-nextest-0.9.152-universal-apple-darwin.tar.gz")
        }));
    }

    #[test]
    fn helm_unittest_names() {
        let (name, file) = (tool("helm-unittest").asset)("1.2.1", PLATFORMS[0]);
        assert_eq!(name, "helm-unittest-linux-amd64-1.2.1.tgz");
        assert_eq!(file, "untt-linux-amd64");
    }

    #[test]
    fn kubeconform_and_yq_names() {
        let (name, file) = (tool("kubeconform").asset)("0.8.0", PLATFORMS[1]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("kubeconform-linux-arm64.tar.gz", "kubeconform")
        );
        let (name, file) = (tool("yq").asset)("4.54.1", PLATFORMS[2]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("yq_darwin_arm64.tar.gz", "yq_darwin_arm64")
        );
    }

    #[test]
    fn kube_linter_is_a_bare_file() {
        let names = [
            "kube-linter-darwin",
            "kube-linter-darwin_arm64",
            "kube-linter-linux",
            "kube-linter-linux_arm64",
        ];
        let r = release("v0.8.3", &names, Some("sha256:abc"));
        let bins = github_binaries(tool("kube-linter"), &r).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(bins.iter().all(|b| b.kind == "file" && b.file.is_none()));
        assert!(
            bins.iter().any(|b| b.os == "linux"
                && b.cpu == "x86_64"
                && b.url.ends_with("kube-linter-linux"))
        );
    }

    #[test]
    fn lefthook_asset_names() {
        let (name, file) = (tool("lefthook").asset)("2.2.2", PLATFORMS[2]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("lefthook_2.2.2_MacOS_arm64", "")
        );
        let (name, _) = (tool("lefthook").asset)("2.2.2", PLATFORMS[0]);
        assert_eq!(name, "lefthook_2.2.2_Linux_x86_64");
    }

    #[test]
    fn hadolint_typos_machete_names() {
        let (name, file) = (tool("hadolint").asset)("2.15.1", PLATFORMS[2]);
        assert_eq!((name.as_str(), file.as_str()), ("hadolint-macos-arm64", ""));
        let (name, file) = (tool("typos").asset)("1.51.1", PLATFORMS[0]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("typos-v1.51.1-x86_64-unknown-linux-musl.tar.gz", "typos")
        );
        let (name, file) = (tool("cargo-machete").asset)("0.10.1", PLATFORMS[1]);
        assert_eq!(
            name,
            "cargo-machete-v0.10.1-aarch64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            file,
            "cargo-machete-v0.10.1-aarch64-unknown-linux-musl/cargo-machete"
        );
    }

    #[test]
    fn helm_uses_sidecar_hash_and_adds_v_prefix() {
        let hash = "a".repeat(64);
        let body = format!("{hash}  helm.tar.gz\n");
        let bins = helm_binaries("4.3.1", |_| Ok(body.clone())).unwrap();
        assert_eq!(bins.len(), 3);
        let b = &bins[1];
        assert_eq!(b.url, "https://get.helm.sh/helm-v4.3.1-linux-arm64.tar.gz");
        assert_eq!(b.file.as_deref(), Some("linux-arm64/helm"));
        assert_eq!(b.sha256, hash);
    }

    #[test]
    fn check_fails_for_a_lock_that_differs_from_upstream() {
        assert!(check_lock("{}\n", "{}\n").is_ok());
        let stale = check_lock("{}\n", "{\"esbuild\":{}}\n").unwrap_err();
        assert!(
            stale.to_string().contains("cargo xtask update-tools"),
            "{stale}"
        );
    }

    #[test]
    fn render_lists_an_npm_tool_next_to_the_github_ones() {
        let mut t = BTreeMap::new();
        t.insert("swc", vec![]);
        t.insert(NPM_TOOLS[0].name, vec![]);
        t.insert("hugo", vec![]);
        let out = render(t).unwrap();
        assert!(out.find("\"esbuild\"").unwrap() < out.find("\"hugo\"").unwrap());
        assert!(out.find("\"hugo\"").unwrap() < out.find("\"swc\"").unwrap());
    }

    #[test]
    fn sha_file_rejects_garbage() {
        assert!(parse_sha_file("<html>").is_err());
        assert!(parse_sha_file(&"g".repeat(64)).is_err());
    }

    #[test]
    fn render_is_sorted_and_has_schema() {
        let mut t = BTreeMap::new();
        t.insert("zz", vec![]);
        t.insert("aa", vec![]);
        let out = render(t).unwrap();
        assert!(out.contains("\"$schema\""));
        assert!(out.find("\"aa\"").unwrap() < out.find("\"zz\"").unwrap());
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn jq_asset_names() {
        let names: Vec<_> = PLATFORMS
            .iter()
            .map(|&p| (tool("jq").asset)("1.8.2", p).0)
            .collect();
        assert_eq!(
            names,
            ["jq-linux-amd64", "jq-linux-arm64", "jq-macos-arm64"]
        );
    }

    #[test]
    fn tailwindcss_asset_names() {
        let (name, file) = (tool("tailwindcss").asset)("4.3.3", PLATFORMS[0]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("tailwindcss-linux-x64", "")
        );
        let (name, file) = (tool("tailwindcss").asset)("4.3.3", PLATFORMS[1]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("tailwindcss-linux-arm64", "")
        );
        let (name, file) = (tool("tailwindcss").asset)("4.3.3", PLATFORMS[2]);
        assert_eq!(
            (name.as_str(), file.as_str()),
            ("tailwindcss-macos-arm64", "")
        );
    }

    #[test]
    fn swc_asset_names_pick_the_musl_build_on_linux() {
        let names: Vec<_> = PLATFORMS
            .iter()
            .map(|&p| (tool("swc").asset)("1.16.22", p))
            .collect();
        let expected = [
            "swc-linux-x64-musl",
            "swc-linux-arm64-musl",
            "swc-darwin-arm64",
        ];
        for (got, want) in names.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1.as_str()), (want, ""));
        }
    }

    #[test]
    fn hugo_asset_names_use_the_standard_build() {
        let names: Vec<_> = PLATFORMS
            .iter()
            .map(|&p| (tool("hugo").asset)("0.170.0", p))
            .collect();
        let linux_x86 = ("hugo_0.170.0_linux-amd64.tar.gz", "hugo");
        let linux_arm = ("hugo_0.170.0_linux-arm64.tar.gz", "hugo");
        let macos = ("hugo_0.170.0_darwin-universal.pkg", "Payload/hugo");
        let expected = [linux_x86, linux_arm, macos];
        for (got, want) in names.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1.as_str()), want);
        }
    }

    #[test]
    fn hugo_macos_is_a_pkg_and_linux_an_archive() {
        let names = [
            "hugo_0.170.0_linux-amd64.tar.gz",
            "hugo_0.170.0_linux-arm64.tar.gz",
            "hugo_0.170.0_darwin-universal.pkg",
        ];
        let r = release("v0.170.0", &names, Some("sha256:abc"));
        let bins = github_binaries(tool("hugo"), &r).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(
            bins.iter()
                .all(|b| b.kind == if b.os == "macos" { "pkg" } else { "archive" })
        );
        assert!(bins.iter().any(|b| b.os == "macos"
            && b.cpu == "arm64"
            && b.file.as_deref() == Some("Payload/hugo")));
    }

    #[test]
    fn tailwindcss_is_a_bare_file() {
        let names = [
            "tailwindcss-linux-x64",
            "tailwindcss-linux-arm64",
            "tailwindcss-macos-x64",
            "tailwindcss-macos-arm64",
        ];
        let r = release("v4.3.3", &names, Some("sha256:abc"));
        let bins = github_binaries(tool("tailwindcss"), &r).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(bins.iter().all(|b| b.kind == "file" && b.file.is_none()));
    }
}
