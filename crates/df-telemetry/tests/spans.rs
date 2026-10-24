//! Spans of the telemetry setup (`specs/telemetry/spec.md`, "Span coverage"): the layer and the
//! `http.request` middleware.
//!
//! A test binary of its own, with a global subscriber: tracing caches which callsites are enabled
//! per process, and tests running in parallel threads make that cache race with a scoped one.

use std::sync::OnceLock;

use axum::Router;
use axum::routing::get;
use df_telemetry::{http, otel};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tracing_subscriber::layer::SubscriberExt as _;

/// The one subscriber of the process, exporting into the returned exporter.
fn exporter() -> &'static InMemorySpanExporter {
    static EXPORTER: OnceLock<InMemorySpanExporter> = OnceLock::new();
    EXPORTER.get_or_init(|| {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry().with(Some(otel::trace_layer(&provider)));
        tracing::subscriber::set_global_default(subscriber)
            .expect("first subscriber of the process");
        exporter
    })
}

fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

#[test]
fn spans_reach_the_exporter_without_the_exporters_own_stack() {
    let exporter = exporter();
    tracing::info_span!("agent.persist_batch", messages = 3).in_scope(|| {});
    tracing::info_span!(target: "hyper::proto", "internal-stack-span").in_scope(|| {});
    let names: Vec<_> = exporter
        .get_finished_spans()
        .unwrap_or_default()
        .iter()
        .map(|s| s.name.to_string())
        .collect();
    assert!(names.contains(&"agent.persist_batch".to_string()));
    assert!(!names.contains(&"internal-stack-span".to_string()));
}

#[tokio::test]
async fn http_requests_are_spans_with_method_route_and_status() -> anyhow::Result<()> {
    let exporter = exporter();
    let router = Router::new()
        .route("/tasks/{id}", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(http::request_span));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(axum::serve(listener, router).into_future());

    let mut stream = TcpStream::connect(addr).await?;
    stream
        .write_all(b"GET /tasks/42?token=secret HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    server.abort();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let spans = exporter.get_finished_spans()?;
    let span = spans
        .iter()
        .find(|s| {
            s.name == "http.request" && attr(s, "http.route").as_deref() == Some("/tasks/{id}")
        })
        .ok_or_else(|| anyhow::anyhow!("no http.request span: {spans:?}"))?;
    assert_eq!(attr(span, "http.request.method").as_deref(), Some("GET"));
    assert_eq!(
        attr(span, "http.response.status_code").as_deref(),
        Some("200")
    );
    assert!(!format!("{span:?}").contains("secret"));
    Ok(())
}
