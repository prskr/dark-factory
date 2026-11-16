//! The Playwright version check over fixture text: match, mismatch and missing values.

use super::{Source, Versions, check};

const PACKAGE_JSON: &str =
    r#"{ "name": "playwright-server", "dependencies": { "playwright-core": "1.65.0" } }"#;
const MODULE_BAZEL: &str = r#"
pull(
    name = "other",
    tag = "v9.9.9-noble",
)
pull(
    name = "playwright_base",
    digest = "sha256:abc",
    tag = "v1.65.0-noble",
)
"#;
const BUILD_RS: &str = "const OTHER: &str = \"x\";\nconst PLAYWRIGHT_VERSION: &str = \"1.65.0\";\n";
const DRIVER_URLS: &str = "const NODE_VERSION: &str = \"26.13.0\";\n";

fn run(
    package_json: &str,
    module_bazel: &str,
    build_rs: &str,
    driver_urls: &str,
) -> anyhow::Result<Versions> {
    check(
        &Source {
            file: "package.json",
            text: package_json,
        },
        &Source {
            file: "MODULE.bazel",
            text: module_bazel,
        },
        &Source {
            file: "build.rs",
            text: build_rs,
        },
        &Source {
            file: "driver_urls.rs",
            text: driver_urls,
        },
    )
}

#[test]
fn matching_versions_give_the_playwright_and_node_version() {
    let versions = run(PACKAGE_JSON, MODULE_BAZEL, BUILD_RS, DRIVER_URLS).unwrap();
    assert_eq!(
        versions,
        Versions {
            playwright: "1.65.0".into(),
            node: "26.13.0".into()
        }
    );
}

#[test]
fn a_mismatch_names_the_three_versions_and_files() {
    let error = run(
        PACKAGE_JSON,
        MODULE_BAZEL,
        &BUILD_RS.replace("1.65.0", "1.64.0"),
        DRIVER_URLS,
    )
    .unwrap_err()
    .to_string();
    for part in [
        "package.json: '1.65.0'",
        "MODULE.bazel (playwright_base): '1.65.0'",
        "build.rs: '1.64.0'",
    ] {
        assert!(error.contains(part), "{error}");
    }
}

#[test]
fn the_base_image_tag_is_read_only_from_the_playwright_base_pull() {
    let module_bazel = r#"
pull(
    name = "playwright_base",
    tag = "v1.64.0-jammy",
)
pull(
    name = "later",
    tag = "v1.65.0-noble",
)
"#;
    let error = run(PACKAGE_JSON, module_bazel, BUILD_RS, DRIVER_URLS)
        .unwrap_err()
        .to_string();
    assert!(error.contains("playwright_base"), "{error}");
}

#[test]
fn a_missing_version_names_the_file() {
    let cases = [
        (
            run("{}", MODULE_BAZEL, BUILD_RS, DRIVER_URLS),
            "package.json",
        ),
        (
            run(PACKAGE_JSON, "pull(name = \"x\")", BUILD_RS, DRIVER_URLS),
            "MODULE.bazel",
        ),
        (run(PACKAGE_JSON, MODULE_BAZEL, "", DRIVER_URLS), "build.rs"),
        (
            run(PACKAGE_JSON, MODULE_BAZEL, BUILD_RS, ""),
            "driver_urls.rs",
        ),
    ];
    for (result, file) in cases {
        let error = result.unwrap_err().to_string();
        assert!(error.starts_with("no ") && error.ends_with(file), "{error}");
    }
}
