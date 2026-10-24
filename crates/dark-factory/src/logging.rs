//! Log format and level selection (`DF_LOG_FORMAT`, `RUST_LOG`).
//!
//! The same setup lives in `factory-agent` (`crates/factory-agent/src/logging.rs`); the sandbox
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

impl LogFormat {
    /// The `DF_LOG_FORMAT` value of this format.
    pub fn as_str(self) -> &'static str {
        match self {
            LogFormat::Text => "text",
            LogFormat::Json => "json",
        }
    }
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

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

    #[test]
    fn json_event_has_timestamp_level_and_message() -> anyhow::Result<()> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buf);
        let sub = subscriber(LogFormat::Json, EnvFilter::new("info"), move || {
            Sink(Arc::clone(&writer))
        });
        tracing::subscriber::with_default(sub, || tracing::info!(run_id = "r1", "hello"));
        let line = String::from_utf8(buf.lock().expect("capture lock").clone())?;
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
        assert!(resolve_filter(Some("info,dark_factory=debug")).1.is_none());
        assert_eq!(
            resolve_filter(Some("dark_factory=loud")).1.as_deref(),
            Some("dark_factory=loud")
        );
    }

    #[test]
    fn invalid_level_logs_at_info_and_warns_naming_rust_log() -> anyhow::Result<()> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buf);
        let dispatch = build(LogFormat::Json, Some("dark_factory=loud"), move || {
            Sink(Arc::clone(&writer))
        });
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!("visible");
            tracing::debug!("hidden");
        });
        let text = String::from_utf8(buf.lock().expect("capture lock").clone())?;
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
}
