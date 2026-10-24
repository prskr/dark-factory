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
