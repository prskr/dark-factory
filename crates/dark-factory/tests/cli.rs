//! Process-level checks of the command line, which need the real environment.

use std::process::Command;

/// Cargo sets `CARGO_BIN_EXE_*` at compile time; Bazel passes the path in `DF_TEST_BIN`.
fn binary() -> String {
    option_env!("CARGO_BIN_EXE_dark-factory")
        .map(str::to_string)
        .or_else(|| std::env::var("DF_TEST_BIN").ok())
        .expect("binary path (CARGO_BIN_EXE_dark-factory or DF_TEST_BIN)")
}

#[test]
fn unknown_log_format_in_env_exits_non_zero_naming_the_variable() {
    // A config path that cannot exist, so a parse regression fails fast instead of serving.
    let dir = tempfile::tempdir().expect("tempdir");
    let out = Command::new(binary())
        .arg("--config")
        .arg(dir.path().join("missing.yaml"))
        .arg("serve")
        .env_clear()
        .env("DF_LOG_FORMAT", "logfmt")
        .output()
        .expect("run binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("DF_LOG_FORMAT") && stderr.contains("text") && stderr.contains("json"),
        "{stderr}"
    );
}
