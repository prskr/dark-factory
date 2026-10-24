//! OpenTelemetry setup of the control plane and the gateway (`OTEL_*` variables).
//!
//! Library crates record through `opentelemetry::global`; this module owns the SDK: it selects
//! the exporters, registers the meter provider and offers the span layer for the subscriber
//! (`specs/telemetry/spec.md`, design D1-D3, D6).

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{Key, KeyValue};
use opentelemetry_otlp::{MetricExporter, SpanExporter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::resource::EnvResourceDetector;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use prometheus::{Encoder as _, Registry, TextEncoder};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::filter_fn;

use crate::logging::BoxedLayer;

const DEFAULT_PROMETHEUS_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const DEFAULT_PROMETHEUS_PORT: u16 = 9464;
/// Name of the tracer (instrumentation scope) of all spans.
const TRACER_NAME: &str = "dark-factory";
/// Targets whose events must not become spans: exporting them would produce more of them.
const EXPORT_TARGETS: [&str; 6] = ["opentelemetry", "tonic", "hyper", "h2", "tower", "reqwest"];
/// Variables that select the OTLP transport; only gRPC is built in.
const PROTOCOL_VARS: [&str; 3] = [
    "OTEL_EXPORTER_OTLP_PROTOCOL",
    "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
    "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
];

/// Why the telemetry could not be set up.
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// An `OTEL_*` variable has a value that is not supported.
    #[error(
        "invalid {var} value `{value}`{}",
        .reason.as_ref().map(|r| format!(": {r}")).unwrap_or_default()
    )]
    InvalidVariable {
        var: &'static str,
        value: String,
        /// What is expected instead, if the value does not say it all.
        reason: Option<String>,
    },
    #[error("failed to bind the metrics endpoint on {addr}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("failed to build the {what}")]
    Exporter {
        what: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl TelemetryError {
    fn invalid(var: &'static str, value: &str, reason: Option<&str>) -> Self {
        Self::InvalidVariable {
            var,
            value: value.to_string(),
            reason: reason.map(str::to_string),
        }
    }

    fn exporter(
        what: &'static str,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Exporter {
            what,
            source: source.into(),
        }
    }
}

/// The process whose telemetry is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    ControlPlane,
    Gateway,
}

impl Component {
    /// Default `service.name`.
    fn service_name(self) -> &'static str {
        match self {
            Component::ControlPlane => "dark-factory-control-plane",
            Component::Gateway => "dark-factory-gateway",
        }
    }
}

/// Where metrics go (`OTEL_METRICS_EXPORTER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsExporter {
    Prometheus,
    Otlp,
}

/// Trace sampling (`OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    parent_based: bool,
    kind: SamplingKind,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            parent_based: true,
            kind: SamplingKind::AlwaysOn,
        }
    }
}

impl Sampling {
    fn sampler(self) -> Sampler {
        let root = match self.kind {
            SamplingKind::AlwaysOn => Sampler::AlwaysOn,
            SamplingKind::AlwaysOff => Sampler::AlwaysOff,
            SamplingKind::Ratio(ratio) => Sampler::TraceIdRatioBased(ratio),
        };
        if self.parent_based {
            Sampler::ParentBased(Box::new(root))
        } else {
            root
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SamplingKind {
    AlwaysOn,
    AlwaysOff,
    Ratio(f64),
}

/// Telemetry configuration from the environment.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub metrics: Vec<MetricsExporter>,
    /// Spans are exported over OTLP.
    pub traces: bool,
    pub sampling: Sampling,
    pub prometheus: SocketAddr,
    /// `OTEL_SERVICE_NAME`, when set.
    service_name: Option<String>,
    component_name: &'static str,
}

impl Settings {
    /// Reads the settings through `lookup` (an environment lookup, faked in tests).
    pub fn from_lookup(
        component: Component,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let host = match lookup("OTEL_EXPORTER_PROMETHEUS_HOST") {
            Some(v) if !v.trim().is_empty() => {
                v.trim().parse().map_err(|e: std::net::AddrParseError| {
                    TelemetryError::invalid(
                        "OTEL_EXPORTER_PROMETHEUS_HOST",
                        &v,
                        Some(&e.to_string()),
                    )
                })?
            }
            _ => DEFAULT_PROMETHEUS_HOST,
        };
        let port = match lookup("OTEL_EXPORTER_PROMETHEUS_PORT") {
            Some(v) if !v.trim().is_empty() => {
                v.trim().parse().map_err(|e: std::num::ParseIntError| {
                    TelemetryError::invalid(
                        "OTEL_EXPORTER_PROMETHEUS_PORT",
                        &v,
                        Some(&e.to_string()),
                    )
                })?
            }
            _ => DEFAULT_PROMETHEUS_PORT,
        };
        for var in PROTOCOL_VARS {
            check_protocol(var, lookup(var).as_deref())?;
        }
        Ok(Self {
            metrics: parse_metrics_exporters(lookup("OTEL_METRICS_EXPORTER").as_deref())?,
            traces: parse_traces_exporter(lookup("OTEL_TRACES_EXPORTER").as_deref())?,
            sampling: parse_sampling(
                lookup("OTEL_TRACES_SAMPLER").as_deref(),
                lookup("OTEL_TRACES_SAMPLER_ARG").as_deref(),
            )?,
            prometheus: SocketAddr::new(host, port),
            service_name: lookup("OTEL_SERVICE_NAME").filter(|v| !v.trim().is_empty()),
            component_name: component.service_name(),
        })
    }
}

/// The installed telemetry. Shut it down with [`Telemetry::shutdown`] to flush OTLP exports.
#[derive(Debug)]
pub struct Telemetry {
    meters: Option<SdkMeterProvider>,
    tracer: Option<SdkTracerProvider>,
    server: Option<MetricsServer>,
}

impl Telemetry {
    /// Telemetry of `component` configured from the process environment. Registers the global
    /// meter provider; the span layer is handed out by [`Telemetry::trace_layer`].
    pub async fn start(component: Component) -> Result<Self> {
        let settings = Settings::from_lookup(component, |key| std::env::var(key).ok())?;
        let telemetry = Self::start_with(
            &settings,
            resource(&settings, &instance_id(), &env_resource()),
        )
        .await?;
        if let Some(meters) = &telemetry.meters {
            opentelemetry::global::set_meter_provider(meters.clone());
        }
        Ok(telemetry)
    }

    async fn start_with(settings: &Settings, resource: Resource) -> Result<Self> {
        let mut telemetry = Telemetry {
            meters: None,
            tracer: None,
            server: None,
        };
        if !settings.metrics.is_empty() {
            let mut meters = SdkMeterProvider::builder().with_resource(resource.clone());
            if settings.metrics.contains(&MetricsExporter::Prometheus) {
                let registry = Registry::new();
                // Resource attributes only reach `target_info` by default; every series carries
                // the instance, so that pods can be told apart after relabeling.
                let reader = opentelemetry_prometheus::exporter()
                    .with_registry(registry.clone())
                    .with_resource_selector(HashSet::from([Key::new("service.instance.id")]))
                    .build()
                    .map_err(|e| TelemetryError::exporter("Prometheus exporter", e))?;
                meters = meters.with_reader(reader);
                telemetry.server = Some(serve_metrics(settings.prometheus, registry).await?);
            }
            if settings.metrics.contains(&MetricsExporter::Otlp) {
                let exporter = MetricExporter::builder()
                    .with_tonic()
                    .build()
                    .map_err(|e| TelemetryError::exporter("OTLP metric exporter", e))?;
                meters = meters.with_reader(PeriodicReader::builder(exporter).build());
            }
            telemetry.meters = Some(meters.build());
        }
        if settings.traces {
            let exporter = SpanExporter::builder()
                .with_tonic()
                .build()
                .map_err(|e| TelemetryError::exporter("OTLP span exporter", e))?;
            telemetry.tracer = Some(tracer_provider(exporter, resource, settings.sampling));
        }
        Ok(telemetry)
    }

    /// The address `/metrics` is served on, if it is.
    pub fn metrics_addr(&self) -> Option<SocketAddr> {
        self.server.as_ref().map(|s| s.addr)
    }

    /// The layer that exports spans; `None` unless traces are enabled.
    pub fn trace_layer(&self) -> Option<BoxedLayer> {
        self.tracer.as_ref().map(trace_layer)
    }

    /// Flushes pending exports and stops the exporters, waiting at most `grace` (a collector
    /// that does not answer must not delay the shutdown).
    pub async fn shutdown(mut self, grace: Duration) {
        let meters = self.meters.take();
        let tracer = self.tracer.take();
        if meters.is_none() && tracer.is_none() {
            return;
        }
        let (done, flushed) = tokio::sync::oneshot::channel();
        // A plain thread: a flush stuck on an unreachable collector must not keep the runtime
        // (and with it the process) alive after the grace period.
        let spawned = std::thread::Builder::new()
            .name("telemetry-flush".into())
            .spawn(move || {
                if let Some(Err(err)) = meters.map(|m| m.shutdown()) {
                    tracing::warn!(error = %err, "failed to flush metrics");
                }
                if let Some(Err(err)) = tracer.map(|t| t.shutdown()) {
                    tracing::warn!(error = %err, "failed to flush spans");
                }
                // The receiver is gone after the grace period; nothing left to tell.
                let _ = done.send(());
            });
        if let Err(err) = spawned {
            tracing::warn!(error = %err, "failed to start the telemetry flush");
            return;
        }
        if tokio::time::timeout(grace, flushed).await.is_err() {
            tracing::warn!("telemetry flush did not finish within the grace period");
        }
    }
}

/// The span layer for `provider`. Events of the exporters' own stack are left out.
pub fn trace_layer(provider: &SdkTracerProvider) -> BoxedLayer {
    tracing_opentelemetry::layer()
        .with_tracer(provider.tracer(TRACER_NAME))
        .with_filter(filter_fn(|meta| {
            let target = meta.target();
            !EXPORT_TARGETS.iter().any(|t| target.starts_with(t))
        }))
        .boxed()
}

fn tracer_provider(
    exporter: impl opentelemetry_sdk::trace::SpanExporter + 'static,
    resource: Resource,
    sampling: Sampling,
) -> SdkTracerProvider {
    SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(sampling.sampler())
        .with_batch_exporter(exporter)
        .build()
}

type Result<T, E = TelemetryError> = std::result::Result<T, E>;

/// The `/metrics` listener; stopped when dropped.
#[derive(Debug)]
struct MetricsServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for MetricsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Parses `OTEL_METRICS_EXPORTER`: a comma-separated list; unset or empty means `prometheus`.
fn parse_metrics_exporters(raw: Option<&str>) -> Result<Vec<MetricsExporter>> {
    let raw = raw.map(str::trim).filter(|v| !v.is_empty());
    let Some(raw) = raw else {
        return Ok(vec![MetricsExporter::Prometheus]);
    };
    let mut exporters = Vec::new();
    for name in raw.split(',').map(str::trim) {
        let exporter = match name {
            "prometheus" => MetricsExporter::Prometheus,
            "otlp" => MetricsExporter::Otlp,
            "none" => continue,
            other => {
                return Err(TelemetryError::invalid(
                    "OTEL_METRICS_EXPORTER",
                    other,
                    None,
                ));
            }
        };
        if !exporters.contains(&exporter) {
            exporters.push(exporter);
        }
    }
    Ok(exporters)
}

/// Parses `OTEL_TRACES_EXPORTER`; `true` if traces are exported (only `otlp` does).
fn parse_traces_exporter(raw: Option<&str>) -> Result<bool> {
    let raw = raw.map(str::trim).filter(|v| !v.is_empty());
    let Some(raw) = raw else {
        return Ok(false);
    };
    let mut enabled = false;
    for name in raw.split(',').map(str::trim) {
        match name {
            "otlp" => enabled = true,
            "none" => {}
            other => return Err(TelemetryError::invalid("OTEL_TRACES_EXPORTER", other, None)),
        }
    }
    Ok(enabled)
}

/// Parses `OTEL_TRACES_SAMPLER` and its argument; unset means `parentbased_always_on`.
fn parse_sampling(name: Option<&str>, arg: Option<&str>) -> Result<Sampling> {
    let Some(name) = name.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(Sampling::default());
    };
    let (parent_based, base) = match name.strip_prefix("parentbased_") {
        Some(base) => (true, base),
        None => (false, name),
    };
    let kind = match base {
        "always_on" => SamplingKind::AlwaysOn,
        "always_off" => SamplingKind::AlwaysOff,
        "traceidratio" => {
            let ratio = match arg.map(str::trim).filter(|v| !v.is_empty()) {
                Some(v) => v
                    .parse::<f64>()
                    .ok()
                    .filter(|r| (0.0..=1.0).contains(r))
                    .ok_or_else(|| {
                        TelemetryError::invalid(
                            "OTEL_TRACES_SAMPLER_ARG",
                            v,
                            Some("a ratio from 0 to 1"),
                        )
                    })?,
                None => 1.0,
            };
            SamplingKind::Ratio(ratio)
        }
        _ => return Err(TelemetryError::invalid("OTEL_TRACES_SAMPLER", name, None)),
    };
    Ok(Sampling { parent_based, kind })
}

/// Only gRPC is built in: any other protocol would be silently ignored, so it is refused.
fn check_protocol(var: &'static str, raw: Option<&str>) -> Result<()> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None | Some("grpc") => Ok(()),
        Some(other) => Err(TelemetryError::invalid(
            var,
            other,
            Some("only `grpc` is supported"),
        )),
    }
}

/// The attributes of `OTEL_RESOURCE_ATTRIBUTES`.
fn env_resource() -> Resource {
    Resource::builder_empty()
        .with_detector(Box::new(EnvResourceDetector::new()))
        .build()
}

/// Resource precedence, lowest first: the defaults (component `service.name`, `service.version`,
/// `service.instance.id` = `instance_id`), the `from_env` attributes (`OTEL_RESOURCE_ATTRIBUTES`),
/// `OTEL_SERVICE_NAME` for `service.name`.
fn resource(settings: &Settings, instance_id: &str, from_env: &Resource) -> Resource {
    let copy = |r: &Resource| {
        r.iter()
            .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
            .collect::<Vec<_>>()
    };
    let builder = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", settings.component_name),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("service.instance.id", instance_id.to_string()),
        ])
        .with_attributes(copy(from_env));
    match &settings.service_name {
        Some(name) => builder.with_service_name(name.clone()),
        None => builder,
    }
    .build()
}

/// The pod name (Kubernetes sets `HOSTNAME` to it), else the host name.
fn instance_id() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Serves `GET /metrics` from `registry` on `addr`.
async fn serve_metrics(addr: SocketAddr, registry: Registry) -> Result<MetricsServer> {
    let bind = |source| TelemetryError::Bind { addr, source };
    let listener = TcpListener::bind(addr).await.map_err(bind)?;
    let addr = listener.local_addr().map_err(bind)?;
    // No `http.request` span on purpose: a scrape every few seconds would drown the real ones.
    let router = Router::new()
        .route("/metrics", get(scrape))
        .with_state(registry);
    let task = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, router).await {
            tracing::error!(error = %err, "metrics listener failed");
        }
    });
    Ok(MetricsServer { addr, task })
}

async fn scrape(State(registry): State<Registry>) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let mut body = Vec::new();
    match encoder.encode(&registry.gather(), &mut body) {
        Ok(()) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, encoder.format_type().to_string())],
            body,
        ),
        Err(err) => {
            tracing::error!(error = %err, "failed to encode metrics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain".to_string())],
                Vec::new(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context as _, Result};
    use opentelemetry::metrics::MeterProvider as _;
    use opentelemetry::trace::Tracer as _;
    use opentelemetry_sdk::error::OTelSdkResult;
    use opentelemetry_sdk::metrics::InMemoryMetricExporter;
    use opentelemetry_sdk::trace::SpanData;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::*;

    fn settings(env: &[(&str, &str)]) -> std::result::Result<Settings, TelemetryError> {
        Settings::from_lookup(Component::Gateway, |key| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        })
    }

    #[test]
    fn defaults_scrape_on_9464_without_traces() -> Result<()> {
        let s = settings(&[])?;
        assert_eq!(s.metrics, [MetricsExporter::Prometheus]);
        assert!(!s.traces);
        assert_eq!(s.sampling, Sampling::default());
        assert_eq!(s.prometheus, "0.0.0.0:9464".parse()?);
        assert_eq!(s.service_name, None);
        Ok(())
    }

    #[test]
    fn prometheus_address_and_service_name_come_from_the_environment() -> Result<()> {
        let s = settings(&[
            ("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1"),
            ("OTEL_EXPORTER_PROMETHEUS_PORT", "9999"),
            ("OTEL_SERVICE_NAME", "custom"),
        ])?;
        assert_eq!(s.prometheus, "127.0.0.1:9999".parse()?);
        assert_eq!(s.service_name.as_deref(), Some("custom"));
        let err = settings(&[("OTEL_EXPORTER_PROMETHEUS_PORT", "x")]).unwrap_err();
        assert!(format!("{err:#}").contains("OTEL_EXPORTER_PROMETHEUS_PORT"));
        Ok(())
    }

    #[test]
    fn sampler_variables_are_honored() -> Result<()> {
        let s = settings(&[
            ("OTEL_TRACES_EXPORTER", "otlp"),
            ("OTEL_TRACES_SAMPLER", "parentbased_traceidratio"),
            ("OTEL_TRACES_SAMPLER_ARG", "0.1"),
        ])?;
        assert!(s.traces);
        assert_eq!(
            s.sampling,
            Sampling {
                parent_based: true,
                kind: SamplingKind::Ratio(0.1)
            }
        );
        assert_eq!(
            settings(&[("OTEL_TRACES_SAMPLER", "always_off")])?.sampling,
            Sampling {
                parent_based: false,
                kind: SamplingKind::AlwaysOff
            }
        );
        for bad in [
            &[("OTEL_TRACES_SAMPLER", "sometimes")][..],
            &[
                ("OTEL_TRACES_SAMPLER", "traceidratio"),
                ("OTEL_TRACES_SAMPLER_ARG", "2"),
            ],
        ] {
            let err = format!("{:#}", settings(bad).unwrap_err());
            assert!(err.contains("OTEL_TRACES_SAMPLER"), "{err}");
        }
        Ok(())
    }

    #[test]
    fn sampling_decides_which_spans_are_exported() {
        let exported = |sampling| {
            let exporter = Recording::default();
            let provider = tracer_provider(
                exporter.clone(),
                Resource::builder_empty().build(),
                sampling,
            );
            provider.tracer("t").start("span");
            provider.shutdown().expect("shutdown");
            exporter.names().len()
        };
        assert_eq!(exported(Sampling::default()), 1);
        let off = Sampling {
            parent_based: false,
            kind: SamplingKind::AlwaysOff,
        };
        assert_eq!(exported(off), 0);
    }

    #[test]
    fn the_cause_is_rendered_once_through_the_error_chain() {
        let cause = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        let bind = TelemetryError::Bind {
            addr: ([127, 0, 0, 1], 9464).into(),
            source: cause,
        };
        let exporter = TelemetryError::exporter("OTLP span exporter", "no such endpoint");
        for (err, cause) in [(bind, "address in use"), (exporter, "no such endpoint")] {
            let rendered = format!("{:#}", anyhow::Error::from(err));
            assert_eq!(rendered.matches(cause).count(), 1, "{rendered}");
        }
    }

    #[test]
    fn only_the_grpc_protocol_is_accepted() -> Result<()> {
        settings(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")])?;
        for var in PROTOCOL_VARS {
            let err = format!("{:#}", settings(&[(var, "http/protobuf")]).unwrap_err());
            assert!(err.contains(var) && err.contains("http/protobuf"), "{err}");
        }
        Ok(())
    }

    fn attrs(settings: &Settings, from_env: &[(&'static str, &'static str)]) -> Resource {
        let from_env = Resource::builder_empty()
            .with_attributes(from_env.iter().map(|(k, v)| KeyValue::new(*k, *v)))
            .build();
        resource(settings, "pod-1", &from_env)
    }

    fn get(r: &Resource, k: &str) -> Option<String> {
        r.get(&opentelemetry::Key::new(k.to_string()))
            .map(|v| v.to_string())
    }

    #[test]
    fn resource_carries_service_attributes() -> Result<()> {
        let r = attrs(&settings(&[])?, &[]);
        assert_eq!(
            get(&r, "service.name").as_deref(),
            Some("dark-factory-gateway")
        );
        assert_eq!(get(&r, "service.instance.id").as_deref(), Some("pod-1"));
        assert_eq!(
            get(&r, "service.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        Ok(())
    }

    #[test]
    fn resource_attributes_override_the_defaults() -> Result<()> {
        let r = attrs(
            &settings(&[])?,
            &[("service.name", "custom"), ("service.instance.id", "pod-7")],
        );
        assert_eq!(get(&r, "service.name").as_deref(), Some("custom"));
        assert_eq!(get(&r, "service.instance.id").as_deref(), Some("pod-7"));
        Ok(())
    }

    #[test]
    fn otel_service_name_beats_resource_attributes() -> Result<()> {
        let r = attrs(
            &settings(&[("OTEL_SERVICE_NAME", "named")])?,
            &[("service.name", "custom")],
        );
        assert_eq!(get(&r, "service.name").as_deref(), Some("named"));
        Ok(())
    }

    async fn http_get(addr: SocketAddr, path: &str) -> Result<String> {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut out = String::new();
        stream.read_to_string(&mut out).await?;
        Ok(out)
    }

    #[tokio::test]
    async fn metrics_endpoint_serves_recorded_counters() -> Result<()> {
        let mut s = settings(&[("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1")])?;
        s.prometheus.set_port(0);
        let telemetry = Telemetry::start_with(
            &s,
            resource(&s, "test-pod", &Resource::builder_empty().build()),
        )
        .await?;
        let addr = telemetry.metrics_addr().context("no metrics server")?;
        let meters = telemetry.meters.clone().context("no meter provider")?;
        meters
            .meter("test")
            .u64_counter("df.test.events")
            .build()
            .add(3, &[]);

        let response = http_get(addr, "/metrics").await?;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("df_test_events_total"), "{response}");
        assert!(
            response.contains("service_name=\"dark-factory-gateway\""),
            "{response}"
        );
        let series = response
            .lines()
            .find(|l| l.starts_with("df_test_events_total"))
            .context("no series line")?;
        assert!(
            series.contains("service_instance_id=\"test-pod\""),
            "{series}"
        );
        assert!(http_get(addr, "/other").await?.starts_with("HTTP/1.1 404"));
        Ok(())
    }

    #[tokio::test]
    async fn disabled_metrics_open_no_port_and_install_no_layer() -> Result<()> {
        let s = settings(&[("OTEL_METRICS_EXPORTER", "none")])?;
        let telemetry =
            Telemetry::start_with(&s, resource(&s, "test", &Resource::builder_empty().build()))
                .await?;
        assert!(telemetry.server.is_none() && telemetry.meters.is_none());
        assert!(telemetry.trace_layer().is_none());
        Ok(())
    }

    /// Prometheus pull and OTLP push together; the collector (the default endpoint) is down.
    #[tokio::test(flavor = "multi_thread")]
    async fn pull_and_push_run_together_and_an_unreachable_collector_is_survived() -> Result<()> {
        let mut s = settings(&[
            ("OTEL_METRICS_EXPORTER", "prometheus,otlp"),
            ("OTEL_EXPORTER_PROMETHEUS_HOST", "127.0.0.1"),
        ])?;
        s.prometheus.set_port(0);
        let telemetry =
            Telemetry::start_with(&s, resource(&s, "test", &Resource::builder_empty().build()))
                .await?;
        let addr = telemetry.metrics_addr().context("no metrics server")?;
        let meters = telemetry.meters.clone().context("no meter provider")?;
        meters
            .meter("test")
            .u64_counter("df.test.both")
            .build()
            .add(1, &[]);

        assert!(
            http_get(addr, "/metrics")
                .await?
                .contains("df_test_both_total")
        );
        // The push reader is attached: its export fails against the missing collector (and is
        // logged by the SDK at ERROR), which neither panics nor stops the pull side.
        let flush = tokio::task::spawn_blocking({
            let meters = meters.clone();
            move || meters.force_flush()
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(20), flush)
                .await??
                .is_err()
        );
        assert!(
            http_get(addr, "/metrics")
                .await?
                .contains("df_test_both_total")
        );
        tokio::time::timeout(
            Duration::from_secs(20),
            telemetry.shutdown(Duration::from_secs(5)),
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_delivers_a_final_metrics_export() -> Result<()> {
        let exporter = InMemoryMetricExporter::default();
        // An interval that never elapses: only the flush on shutdown can deliver the counter.
        let reader = PeriodicReader::builder(exporter.clone())
            .with_interval(Duration::from_secs(3600))
            .build();
        let meters = SdkMeterProvider::builder().with_reader(reader).build();
        meters
            .meter("test")
            .u64_counter("df.test.final")
            .build()
            .add(7, &[]);
        assert!(exporter.get_finished_metrics()?.is_empty());

        let telemetry = Telemetry {
            meters: Some(meters),
            tracer: None,
            server: None,
        };
        telemetry.shutdown(Duration::from_secs(5)).await;

        let exported = exporter.get_finished_metrics()?;
        let names: Vec<_> = exported
            .iter()
            .flat_map(|rm| rm.scope_metrics())
            .flat_map(|sm| sm.metrics())
            .map(|m| m.name().to_string())
            .collect();
        assert_eq!(names, ["df.test.final"]);
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_flushes_pending_spans() {
        let exporter = Recording::default();
        let provider = tracer_provider(
            exporter.clone(),
            Resource::builder_empty().build(),
            Sampling::default(),
        );
        provider.tracer("t").start("pending");
        let telemetry = Telemetry {
            meters: None,
            tracer: Some(provider),
            server: None,
        };
        telemetry.shutdown(Duration::from_secs(5)).await;
        assert_eq!(exporter.names(), ["pending"]);
    }

    /// An exporter that keeps the names of the spans it got (the in-memory one forgets them on
    /// shutdown, which is what these tests look at).
    #[derive(Debug, Clone, Default)]
    struct Recording(Arc<Mutex<Vec<String>>>);

    impl Recording {
        fn names(&self) -> Vec<String> {
            self.0.lock().map(|g| g.clone()).unwrap_or_default()
        }
    }

    impl opentelemetry_sdk::trace::SpanExporter for Recording {
        async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
            if let Ok(mut names) = self.0.lock() {
                names.extend(batch.iter().map(|s| s.name.to_string()));
            }
            Ok(())
        }
    }

    /// An exporter that never finishes.
    #[derive(Debug)]
    struct Hanging;

    impl opentelemetry_sdk::trace::SpanExporter for Hanging {
        async fn export(&self, _batch: Vec<SpanData>) -> OTelSdkResult {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn a_hanging_exporter_does_not_exceed_the_grace_period() {
        let provider = tracer_provider(
            Hanging,
            Resource::builder_empty().build(),
            Sampling::default(),
        );
        provider.tracer("t").start("stuck");
        let telemetry = Telemetry {
            meters: None,
            tracer: Some(provider),
            server: None,
        };
        let started = std::time::Instant::now();
        telemetry.shutdown(Duration::from_millis(200)).await;
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn shutdown_returns_within_the_grace_period() -> Result<()> {
        let s = settings(&[("OTEL_METRICS_EXPORTER", "none")])?;
        let telemetry =
            Telemetry::start_with(&s, resource(&s, "test", &Resource::builder_empty().build()))
                .await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            telemetry.shutdown(Duration::from_secs(1)),
        )
        .await?;
        Ok(())
    }

    #[test]
    fn metrics_default_to_prometheus() -> Result<()> {
        assert_eq!(
            parse_metrics_exporters(None)?,
            [MetricsExporter::Prometheus]
        );
        assert_eq!(
            parse_metrics_exporters(Some(" "))?,
            [MetricsExporter::Prometheus]
        );
        Ok(())
    }

    #[test]
    fn metrics_accept_lists() -> Result<()> {
        assert_eq!(
            parse_metrics_exporters(Some("prometheus, otlp"))?,
            [MetricsExporter::Prometheus, MetricsExporter::Otlp]
        );
        assert_eq!(
            parse_metrics_exporters(Some("otlp,otlp"))?,
            [MetricsExporter::Otlp]
        );
        Ok(())
    }

    #[test]
    fn metrics_none_disables() -> Result<()> {
        assert!(parse_metrics_exporters(Some("none"))?.is_empty());
        Ok(())
    }

    #[test]
    fn metrics_unknown_value_names_the_variable() {
        let err = parse_metrics_exporters(Some("statsd"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("OTEL_METRICS_EXPORTER") && err.contains("statsd"),
            "{err}"
        );
    }

    #[test]
    fn traces_are_off_unless_otlp() -> Result<()> {
        assert!(!parse_traces_exporter(None)?);
        assert!(!parse_traces_exporter(Some("none"))?);
        assert!(parse_traces_exporter(Some("otlp"))?);
        let err = parse_traces_exporter(Some("jaeger"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("OTEL_TRACES_EXPORTER"), "{err}");
        Ok(())
    }
}
