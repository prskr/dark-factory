//! The spans of the gateway pod: `gateway.connect` and the health endpoint requests
//! (`specs/telemetry/spec.md`, "Span coverage").
//!
//! A test binary of its own: tracing caches which callsites are enabled per process, and tests
//! running in parallel threads make that cache race with a scoped subscriber, so one global
//! subscriber serves all tests.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::OnceLock;

use async_trait::async_trait;
use df_gateway::flow::FlowLog;
use df_gateway::policy::{HostRule, Policy, Sandbox};
use df_gateway::proxy::{Proxy, TcpDialer};
use df_gateway::resolve::{BlockedRanges, Resolver};
use df_proto::gateway::v1::{FlowFilter, Phase};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tracing_subscriber::layer::SubscriberExt as _;

struct NoNames;

#[async_trait]
impl Resolver for NoNames {
    async fn resolve(&self, _: &str) -> std::io::Result<Vec<std::net::IpAddr>> {
        Err(std::io::ErrorKind::NotFound.into())
    }
}

/// The one subscriber of the process: a global one is not subject to the callsite cache race.
fn exporter() -> &'static InMemorySpanExporter {
    static EXPORTER: OnceLock<InMemorySpanExporter> = OnceLock::new();
    EXPORTER.get_or_init(|| {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("test"));
        // Fails only if a subscriber exists, which no test of this binary installs.
        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer)).ok();
        exporter
    })
}

fn attr(span: &opentelemetry_sdk::trace::SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

struct Started {
    addr: SocketAddr,
    flows: Arc<FlowLog>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn start_proxy() -> anyhow::Result<Started> {
    let sandbox = Sandbox {
        run_id: "run-1".into(),
        task_key: "DF-1".into(),
        run_number: 1,
        project_key: "DF".into(),
        persona_id: "persona".into(),
        hosts: vec![HostRule {
            pattern: "api.anthropic.com".into(),
            port: 443,
            scope: "project".into(),
            entry_id: "e".into(),
        }],
    };
    let policy = Policy::new(1, [("127.0.0.1".parse()?, sandbox)]);
    let (_policy_tx, policy_rx) = tokio::sync::watch::channel(Some(Arc::new(policy)));
    let flows = Arc::new(FlowLog::new(10));
    let proxy = Arc::new(Proxy::new(
        "gw-0",
        policy_rx,
        Arc::new(NoNames),
        Arc::new(TcpDialer),
        BlockedRanges::new([])?,
        flows.clone(),
        Arc::default(),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(proxy.serve(listener, async {
        // Both outcomes (signal, sender dropped) mean: stop.
        stopped.await.ok();
    }));
    Ok(Started {
        addr,
        flows,
        stop,
        task,
    })
}

#[tokio::test]
async fn a_denied_connect_is_a_span_with_the_decision_and_without_the_host() -> anyhow::Result<()> {
    let exporter = exporter();
    let proxy = start_proxy().await?;
    let (_, mut events) = proxy.flows.subscribe(&FlowFilter::default());

    let mut stream = TcpStream::connect(proxy.addr).await?;
    stream
        .write_all(b"CONNECT denied.example:443 HTTP/1.1\r\nHost: denied.example:443\r\n\r\n")
        .await?;
    let mut head = vec![0; 12];
    stream.read_exact(&mut head).await?;
    assert_eq!(&head, b"HTTP/1.1 403", "{}", String::from_utf8_lossy(&head));
    while events.recv().await?.phase != i32::from(Phase::Closed) {}
    // Stopping the proxy ends the request handlers, which closes their spans.
    drop(proxy.stop);
    proxy.task.await?;

    let spans = exporter.get_finished_spans()?;
    let span = spans
        .iter()
        .find(|s| s.name == "gateway.connect" && attr(s, "decision").as_deref() == Some("denied"))
        .ok_or_else(|| anyhow::anyhow!("no gateway.connect span: {spans:?}"))?;
    assert_eq!(attr(span, "host"), None);
    Ok(())
}

#[tokio::test]
async fn the_health_endpoint_requests_are_spans_with_route_and_status() -> anyhow::Result<()> {
    let exporter = exporter();
    let (_policy_tx, policy_rx) = tokio::sync::watch::channel(None);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server =
        tokio::spawn(axum::serve(listener, df_gateway::health::router(policy_rx)).into_future());

    let mut stream = TcpStream::connect(addr).await?;
    stream
        .write_all(b"GET /readyz?x=secret HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    server.abort();

    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    let spans = exporter.get_finished_spans()?;
    let span = spans
        .iter()
        .find(|s| s.name == "http.request" && attr(s, "http.route").as_deref() == Some("/readyz"))
        .ok_or_else(|| anyhow::anyhow!("no http.request span: {spans:?}"))?;
    assert_eq!(attr(span, "http.request.method").as_deref(), Some("GET"));
    assert_eq!(
        attr(span, "http.response.status_code").as_deref(),
        Some("503")
    );
    assert!(!format!("{span:?}").contains("secret"));
    Ok(())
}
