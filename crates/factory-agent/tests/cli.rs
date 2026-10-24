//! Process-level checks of the command line, which need the real environment.

use std::process::Command;

/// Cargo sets `CARGO_BIN_EXE_*` at compile time; Bazel passes the path in `DF_TEST_BIN`.
fn binary() -> String {
    option_env!("CARGO_BIN_EXE_factory-agent")
        .map(str::to_string)
        .or_else(|| std::env::var("DF_TEST_BIN").ok())
        .expect("binary path (CARGO_BIN_EXE_factory-agent or DF_TEST_BIN)")
}

#[test]
fn unknown_log_format_in_env_exits_non_zero_naming_the_variable() {
    let out = Command::new(binary())
        .arg("run")
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
