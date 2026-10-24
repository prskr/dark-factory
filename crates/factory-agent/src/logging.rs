//! Log format and level selection (`DF_LOG_FORMAT`, `RUST_LOG`).
//!
//! The same setup lives in `dark-factory` (`crates/dark-factory/src/logging.rs`); the sandbox
//! binary does not depend on `df-core`, so the ~30 lines are duplicated on purpose.

use clap::ValueEnum;
use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Level used when `RUST_LOG` is unset, empty or invalid.
const DEFAULT_LEVEL: &str = "info";

/// Output format of log events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    /// Human-readable lines.
    Text,
    /// One JSON object per line.
    Json,
}

/// Installs the global subscriber, writing to stdout.
pub fn init_tracing(format: LogFormat) {
    let raw = std::env::var("RUST_LOG").ok();
    // Fails only when a subscriber is already installed, which is fine.
    let _ = tracing::dispatcher::set_global_default(build(format, raw.as_deref(), std::io::stdout));
}

/// Builds the dispatcher for the `RUST_LOG` value `raw`. An invalid value falls back to `info`
/// and is reported as a WARN through the new dispatcher.
fn build<W>(format: LogFormat, raw: Option<&str>, writer: W) -> tracing::Dispatch
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let (filter, invalid) = resolve_filter(raw);
    let dispatch = tracing::Dispatch::new(subscriber(format, filter, writer));
    if let Some(value) = invalid {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::warn!(RUST_LOG = %value, "invalid RUST_LOG, falling back to info");
        });
    }
    dispatch
}

/// Resolves the filter; the second value is the rejected `RUST_LOG` if it was invalid.
fn resolve_filter(raw: Option<&str>) -> (EnvFilter, Option<String>) {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None => (EnvFilter::new(DEFAULT_LEVEL), None),
        Some(v) => match EnvFilter::try_new(v) {
            Ok(f) => (f, None),
            Err(_) => (EnvFilter::new(DEFAULT_LEVEL), Some(v.to_string())),
        },
    }
}

fn subscriber<W>(
    format: LogFormat,
    filter: EnvFilter,
    writer: W,
) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(writer);
    match format {
        LogFormat::Text => Box::new(builder.finish()),
        LogFormat::Json => Box::new(builder.json().finish()),
    }
}

/// Captures log output in tests.
#[cfg(test)]
pub mod testing {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Log output captured by [`capture`].
    #[derive(Clone, Default)]
    pub struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("capture lock")).into_owned()
        }
    }

    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture lock").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A writer factory appending to a fresh [`Captured`] buffer.
    pub fn buffer() -> (
        Captured,
        impl for<'a> MakeWriter<'a> + Send + Sync + 'static,
    ) {
        let captured = Captured::default();
        let buf = Arc::clone(&captured.0);
        (captured, move || Sink(Arc::clone(&buf)))
    }

    /// Installs a thread-local subscriber (`filter` in `RUST_LOG` syntax) writing JSON lines to the
    /// returned buffer until the guard is dropped.
    pub fn capture(filter: &str) -> (Captured, tracing::subscriber::DefaultGuard) {
        let captured = Captured::default();
        let buf = Arc::clone(&captured.0);
        let sub = subscriber(LogFormat::Json, EnvFilter::new(filter), move || {
            Sink(Arc::clone(&buf))
        });
        // With a single registered dispatcher, tracing computes a callsite's interest from the
        // registering thread's default only. A parallel test without a subscriber would then
        // disable the callsite for every capture. A second (silent, global) dispatcher makes
        // tracing consult all of them.
        static SILENT_GLOBAL: std::sync::Once = std::sync::Once::new();
        SILENT_GLOBAL.call_once(|| {
            let silent = tracing_subscriber::fmt()
                .with_max_level(tracing::level_filters::LevelFilter::OFF)
                .with_writer(std::io::sink)
                .finish();
            // Ignored on purpose: a global subscriber may already exist, which serves as well.
            let _ = tracing::subscriber::set_global_default(silent);
        });
        (captured, tracing::subscriber::set_default(sub))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::capture;
    use super::*;

    #[test]
    fn json_event_has_timestamp_level_and_message() -> anyhow::Result<()> {
        let (out, _guard) = capture("info");
        tracing::info!(run_id = "r1", "hello");
        let line = out.text();
        let v: serde_json::Value = serde_json::from_str(line.trim())?;
        assert!(v["timestamp"].is_string());
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["fields"]["message"], "hello");
        assert_eq!(v["fields"]["run_id"], "r1");
        Ok(())
    }

    #[test]
    fn invalid_or_empty_level_falls_back_to_info() {
        assert!(resolve_filter(None).1.is_none());
        assert!(resolve_filter(Some("  ")).1.is_none());
        assert!(resolve_filter(Some("info,factory_agent=debug")).1.is_none());
        assert_eq!(
            resolve_filter(Some("factory_agent=loud")).1.as_deref(),
            Some("factory_agent=loud")
        );
    }

    #[test]
    fn invalid_level_logs_at_info_and_warns_naming_rust_log() -> anyhow::Result<()> {
        let (out, writer) = super::testing::buffer();
        let dispatch = build(LogFormat::Json, Some("factory_agent=loud"), writer);
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("visible");
            tracing::debug!("hidden");
        });
        let text = out.text();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0]["level"], "WARN");
        assert!(lines[0]["fields"].get("RUST_LOG").is_some());
        assert_eq!(lines[1]["fields"]["message"], "visible");
        Ok(())
    }

    #[test]
    fn default_level_writes_info_but_not_debug() {
        let (out, writer) = super::testing::buffer();
        let dispatch = build(LogFormat::Json, None, writer);
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("visible");
            tracing::debug!("hidden");
        });
        let text = out.text();
        assert!(
            text.contains("visible") && !text.contains("hidden"),
            "{text}"
        );
    }
}
