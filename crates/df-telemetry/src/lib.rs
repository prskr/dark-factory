//! Logging and telemetry setup shared by the binaries.
//!
//! - [`logging`] (always): `DF_LOG_FORMAT` and `RUST_LOG` handling and the global subscriber.
//! - [`testing`] (`test-util`): capture of log output in tests.
//! - [`http`] (`http`): the `http.request` span middleware for axum listeners.
//! - [`otel`] (`otel`): OpenTelemetry metrics and traces from the `OTEL_*` variables.
//! - `clap`: `clap::ValueEnum` for [`logging::LogFormat`].
//!
//! Only the control plane and the gateway enable `http` and `otel`: the sandbox runtime enables
//! `clap` (and `test-util` in its tests) and so links neither OpenTelemetry nor axum.

#[cfg(feature = "http")]
pub mod http;
pub mod logging;
#[cfg(feature = "otel")]
pub mod otel;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;

/// The release version the binary was built with (`DF_VERSION` at build time), or `dev`.
///
/// Bazel sets it from `--//build:version`; a plain `cargo build` reports `dev` unless
/// `DF_VERSION=… cargo build` is used.
pub const VERSION: &str = version_or_dev(option_env!("DF_VERSION"));

/// `dev` for a missing or empty build-time version.
const fn version_or_dev(value: Option<&'static str>) -> &'static str {
    match value {
        Some(v) if !v.is_empty() => v,
        _ => "dev",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_empty_version_is_dev() {
        assert_eq!(version_or_dev(None), "dev");
        assert_eq!(version_or_dev(Some("")), "dev");
        assert_eq!(version_or_dev(Some("2026.11.0")), "2026.11.0");
    }
}
