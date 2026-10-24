//! Log format and level selection (`DF_LOG_FORMAT`, `RUST_LOG`) shared by all binaries.

use tracing::Subscriber;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Level used when `RUST_LOG` is unset, empty or invalid.
const DEFAULT_LEVEL: &str = "info";

/// An extra layer on the base registry, e.g. the OpenTelemetry span exporter.
pub type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

/// Output format of log events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
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

/// Installs the global subscriber, writing to stdout. `extra` sees every span and event that
/// passes `RUST_LOG`.
pub fn init_tracing(format: LogFormat, extra: Option<BoxedLayer>) {
    let raw = std::env::var("RUST_LOG").ok();
    // Fails only when a subscriber is already installed, which is fine.
    let _ = tracing::dispatcher::set_global_default(build(
        format,
        raw.as_deref(),
        std::io::stdout,
        extra,
    ));
}

/// Builds the dispatcher for the `RUST_LOG` value `raw`. An invalid value falls back to `info`
/// and is reported as a WARN through the new dispatcher.
pub(crate) fn build<W>(
    format: LogFormat,
    raw: Option<&str>,
    writer: W,
    extra: Option<BoxedLayer>,
) -> tracing::Dispatch
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let (filter, invalid) = resolve_filter(raw);
    let dispatch = tracing::Dispatch::new(subscriber(format, filter, writer, extra));
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

pub(crate) fn subscriber<W>(
    format: LogFormat,
    filter: EnvFilter,
    writer: W,
    extra: Option<BoxedLayer>,
) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let fmt = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(writer);
    let base = tracing_subscriber::registry().with(extra).with(filter);
    match format {
        LogFormat::Text => Box::new(base.with(fmt)),
        LogFormat::Json => Box::new(base.with(fmt.json())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{buffer, capture};

    #[test]
    fn format_names_are_the_env_values() {
        assert_eq!(LogFormat::Text.as_str(), "text");
        assert_eq!(LogFormat::Json.as_str(), "json");
    }

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
        assert!(resolve_filter(Some("info,dark_factory=debug")).1.is_none());
        assert_eq!(
            resolve_filter(Some("dark_factory=loud")).1.as_deref(),
            Some("dark_factory=loud")
        );
    }

    #[test]
    fn invalid_level_logs_at_info_and_warns_naming_rust_log() -> anyhow::Result<()> {
        let (out, writer) = buffer();
        let dispatch = build(LogFormat::Json, Some("dark_factory=loud"), writer, None);
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
        let (out, writer) = buffer();
        let dispatch = build(LogFormat::Json, None, writer, None);
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
