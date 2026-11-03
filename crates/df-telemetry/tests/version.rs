//! `VERSION` is the build-time `DF_VERSION`, `dev` without one.
//!
//! Under Bazel `DF_VERSION_ENV_FILE` points to the env file built from `--//build:version`;
//! `DF_EXPECTED_VERSION` (set by `with_version_test`, together with
//! `DF_REQUIRE_EXPECTED_VERSION`) is the fixed version the build must report.
//! Under cargo none of them is set; the library and this test are compiled in the same
//! environment, so the test reads the build-time `DF_VERSION` the same way.

#[test]
fn version_is_the_stamped_version_or_dev() -> anyhow::Result<()> {
    let fixed = std::env::var("DF_EXPECTED_VERSION");
    assert!(
        fixed.is_ok() || std::env::var("DF_REQUIRE_EXPECTED_VERSION").is_err(),
        "run through with_version_test but DF_EXPECTED_VERSION is not set"
    );
    if let Ok(expected) = fixed {
        assert_eq!(df_telemetry::VERSION, expected);
        return Ok(());
    }
    let expected = match std::env::var("DF_VERSION_ENV_FILE") {
        Ok(file) => std::fs::read_to_string(file)?
            .trim()
            .strip_prefix("DF_VERSION=")
            .map(str::to_owned)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "dev".into()),
        Err(_) => option_env!("DF_VERSION")
            .filter(|v| !v.is_empty())
            .unwrap_or("dev")
            .into(),
    };
    assert_eq!(df_telemetry::VERSION, expected);
    Ok(())
}
