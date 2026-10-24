use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use df_proto::gateway::v1::{FlowEvent, FlowFilter, Phase, Verdict};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use super::*;
use crate::policy::HostRule;
use crate::sni::tests::client_hello;

const PUBLIC_IP: &str = "203.0.113.10";

pub(crate) struct StubResolver(HashMap<String, Vec<IpAddr>>);

impl StubResolver {
    pub(crate) fn of(names: &[(&str, &str)]) -> Self {
        Self(
            names
                .iter()
                .map(|(name, ip)| ((*name).to_string(), vec![ip.parse().unwrap()]))
                .collect(),
        )
    }
}

#[async_trait]
impl Resolver for StubResolver {
    async fn resolve(&self, host: &str) -> std::io::Result<Vec<IpAddr>> {
        self.0
            .get(host)
            .cloned()
            .ok_or_else(|| std::io::ErrorKind::NotFound.into())
    }
}

/// Connects every destination to one local server.
pub(crate) struct RedirectDialer {
    pub(crate) to: SocketAddr,
}

#[async_trait]
impl Dialer for RedirectDialer {
    async fn dial(&self, _: SocketAddr) -> std::io::Result<TcpStream> {
        TcpStream::connect(self.to).await
    }
}

struct Harness {
    proxy: SocketAddr,
    policy: watch::Sender<Option<Arc<Policy>>>,
    flows: Arc<FlowLog>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

fn rule(pattern: &str, port: u16) -> HostRule {
    HostRule {
        pattern: pattern.into(),
        port,
        scope: "project".into(),
        entry_id: "e".into(),
    }
}

fn policy_of(source: &str, hosts: Vec<HostRule>) -> Option<Arc<Policy>> {
    Some(Arc::new(Policy::new(
        1,
        [(
            source.parse().unwrap(),
            Sandbox {
                run_id: "run-1".into(),
                task_key: "DF-1".into(),
                project_key: "DF".into(),
                persona_id: "persona".into(),
                hosts,
            },
        )],
    )))
}

impl Harness {
    /// A proxy whose names resolve as `names` say and whose connections all end at `upstream`.
    async fn start(
        policy: Option<Arc<Policy>>,
        names: &[(&str, &str)],
        upstream: SocketAddr,
    ) -> Result<Self> {
        Self::start_with(policy, names, upstream, Arc::default()).await
    }

    /// Like [`Harness::start`], recording metrics into `metrics`.
    async fn start_with(
        policy: Option<Arc<Policy>>,
        names: &[(&str, &str)],
        upstream: SocketAddr,
        metrics: Arc<crate::metrics::Metrics>,
    ) -> Result<Self> {
        let (policy_tx, policy_rx) = watch::channel(policy);
        let flows = Arc::new(FlowLog::new(100));
        let proxy = Arc::new(Proxy::new(
            "gw-0",
            policy_rx,
            Arc::new(StubResolver::of(names)),
            Arc::new(RedirectDialer { to: upstream }),
            BlockedRanges::new([])?,
            flows.clone(),
            metrics,
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(proxy.serve(listener, async {
            let _ = stopped.await;
        }));
        Ok(Self {
            proxy: addr,
            policy: policy_tx,
            flows,
            stop: Some(stop),
            task,
        })
    }

    /// Every event recorded so far.
    fn events(&self) -> Vec<FlowEvent> {
        self.flows.subscribe(&FlowFilter::default()).0
    }

    /// Stops the proxy and waits until `serve` has returned.
    async fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        (&mut self.task).await.unwrap();
    }

    /// Event number `index` among those with `phase` (any phase if `None`), waiting for it.
    async fn nth_event(&self, phase: Option<Phase>, index: usize) -> FlowEvent {
        let wanted = |event: &FlowEvent| phase.is_none_or(|phase| event.phase() == phase);
        let (events, mut live) = self.flows.subscribe(&FlowFilter::default());
        let mut events: Vec<_> = events.into_iter().filter(wanted).collect();
        while events.len() <= index {
            let event = tokio::time::timeout(Duration::from_secs(5), live.recv())
                .await
                .expect("no event within 5 s")
                .expect("the flow log closed");
            if wanted(&event) {
                events.push(event);
            }
        }
        events.swap_remove(index)
    }

    async fn event(&self, index: usize) -> FlowEvent {
        self.nth_event(None, index).await
    }

    async fn opened_event(&self, index: usize) -> FlowEvent {
        self.nth_event(Some(Phase::Opened), index).await
    }

    async fn closed_event(&self, index: usize) -> FlowEvent {
        self.nth_event(Some(Phase::Closed), index).await
    }
}

/// Sends `head` to the proxy and returns the connection with the status line and headers read.
pub(crate) async fn request(proxy: SocketAddr, head: &str) -> Result<(TcpStream, String)> {
    let mut stream = TcpStream::connect(proxy).await?;
    stream.write_all(head.as_bytes()).await?;
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        response.push(stream.read_u8().await?);
    }
    Ok((stream, String::from_utf8(response)?))
}

pub(crate) fn connect_head(host: &str, port: u16) -> String {
    format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n")
}

/// A TLS server echoing what it reads, with a certificate for `name`; returns its address and
/// a client config that trusts it.
async fn tls_echo_server(name: &str) -> Result<(SocketAddr, TlsConnector)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec![name.to_string()])?;
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key.into())?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone())?;
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let acceptor = TlsAcceptor::from(Arc::new(server));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buf = [0; 64];
                while let Ok(n @ 1..) = tls.read(&mut buf).await {
                    if tls.write_all(&buf[..n]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Ok((addr, TlsConnector::from(Arc::new(client))))
}

#[tokio::test]
async fn allowed_host_gets_a_tunnel_and_an_allowed_event() -> Result<()> {
    let (upstream, tls) = tls_echo_server("api.anthropic.com").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream,
    )
    .await?;

    let (stream, response) =
        request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let mut tls = tls.connect("api.anthropic.com".try_into()?, stream).await?;
    tls.write_all(b"ping").await?;
    let mut echoed = [0; 4];
    tls.read_exact(&mut echoed).await?;
    assert_eq!(&echoed, b"ping");
    drop(tls);

    let event = harness.closed_event(0).await;
    assert_eq!(event.verdict(), Verdict::Allowed);
    assert_eq!(
        (
            event.method.as_str(),
            event.host.as_str(),
            event.port,
            event.sni.as_str()
        ),
        ("CONNECT", "api.anthropic.com", 443, "api.anthropic.com")
    );
    assert_eq!(
        (
            event.run_id.as_str(),
            event.task_key.as_str(),
            event.project_key.as_str()
        ),
        ("run-1", "DF-1", "DF")
    );
    assert_eq!(event.matched_rule, "api.anthropic.com");
    assert_eq!(event.gateway_pod, "gw-0");
    assert!(event.bytes_out > 4 && event.bytes_in > 4, "{event:?}");
    Ok(())
}

#[tokio::test]
async fn wildcard_entry_allows_a_subdomain() -> Result<()> {
    let (upstream, tls) = tls_echo_server("static.crates.io").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("*.crates.io", 443)]),
        &[("static.crates.io", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (stream, response) = request(harness.proxy, &connect_head("static.crates.io", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    tls.connect("static.crates.io".try_into()?, stream).await?;
    assert_eq!(harness.closed_event(0).await.matched_rule, "*.crates.io");
    Ok(())
}

#[tokio::test]
async fn host_without_entry_gets_403_and_a_denied_event() -> Result<()> {
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("example.com", PUBLIC_IP)],
        "127.0.0.1:1".parse()?,
    )
    .await?;
    let (_, response) = request(harness.proxy, &connect_head("example.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let event = harness.closed_event(0).await;
    assert_eq!(event.verdict(), Verdict::Denied);
    assert_eq!(event.reason, "not-allowlisted");
    assert_eq!(event.project_key, "DF");
    assert_eq!(event.phase(), Phase::Closed);
    assert_eq!(
        harness.events().len(),
        1,
        "a denied request records one event"
    );
    Ok(())
}

#[tokio::test]
async fn unknown_pod_gets_403_unknown_source() -> Result<()> {
    let harness = Harness::start(
        policy_of("10.1.2.3", vec![rule("api.anthropic.com", 443)]),
        &[],
        "127.0.0.1:1".parse()?,
    )
    .await?;
    let (_, response) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "unknown-source");
    assert!(event.run_id.is_empty());
    Ok(())
}

#[tokio::test]
async fn without_a_snapshot_connect_gets_503_and_nothing_is_forwarded() -> Result<()> {
    let harness = Harness::start(None, &[], "127.0.0.1:1".parse()?).await?;
    let (_, response) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert_eq!(harness.closed_event(0).await.reason, "no-snapshot");

    let (_, response) = request(
        harness.proxy,
        "GET http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n",
    )
    .await?;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    Ok(())
}

#[tokio::test]
async fn ip_literal_connect_gets_403() -> Result<()> {
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("1.1.1.1", 443)]),
        &[],
        "127.0.0.1:1".parse()?,
    )
    .await?;
    for target in ["1.1.1.1", "[2606:4700::1111]"] {
        let (_, response) = request(harness.proxy, &connect_head(target, 443)).await?;
        assert!(response.starts_with("HTTP/1.1 403"), "{target}: {response}");
    }
    assert_eq!(harness.closed_event(0).await.reason, "ip-literal");
    Ok(())
}

#[tokio::test]
async fn allowed_name_that_resolves_internally_is_refused() -> Result<()> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("internal.example.com", 443)]),
        &[("internal.example.com", "10.0.0.5")],
        upstream.local_addr()?,
    )
    .await?;
    let (_, response) = request(harness.proxy, &connect_head("internal.example.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "private-destination");
    assert_eq!(event.verdict(), Verdict::Denied);
    Ok(())
}

#[tokio::test]
async fn a_name_with_one_private_address_among_public_ones_is_refused() -> Result<()> {
    let (policy_tx, policy_rx) =
        watch::channel(policy_of("127.0.0.1", vec![rule("mixed.example.com", 443)]));
    let resolver = StubResolver(HashMap::from([(
        "mixed.example.com".to_string(),
        vec![PUBLIC_IP.parse()?, "192.168.1.1".parse()?],
    )]));
    let proxy = Proxy::new(
        "gw-0",
        policy_rx,
        Arc::new(resolver),
        Arc::new(RedirectDialer {
            to: "127.0.0.1:1".parse()?,
        }),
        BlockedRanges::new([])?,
        Arc::new(FlowLog::new(10)),
        Arc::default(),
    );
    let refusal = proxy.resolve("mixed.example.com").await.err().unwrap();
    assert_eq!(refusal.reason, "private-destination");
    drop(policy_tx);
    Ok(())
}

#[tokio::test]
async fn domain_fronting_is_closed_before_the_destination_sees_a_byte() -> Result<()> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream.local_addr()?,
    )
    .await?;
    let (mut stream, response) =
        request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    stream.write_all(&client_hello("evil.example.com")).await?;

    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).await?;
    assert!(rest.is_empty(), "the tunnel was closed without a reply");
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "sni-mismatch");
    assert_eq!(event.sni, "evil.example.com");
    assert_eq!(event.verdict(), Verdict::Denied);
    let connected = tokio::time::timeout(Duration::from_millis(100), upstream.accept()).await;
    assert!(connected.is_err(), "the destination was contacted");
    Ok(())
}

#[tokio::test]
async fn a_tunnel_without_sni_is_closed() -> Result<()> {
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        "127.0.0.1:1".parse()?,
    )
    .await?;
    let (mut stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    stream.write_all(b"not tls at all, but long enough").await?;
    // The proxy closes the connection, with a reset because it left data unread.
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest).await;
    assert!(rest.is_empty());
    assert_eq!(harness.closed_event(0).await.reason, "sni-mismatch");

    // A ClientHello without server name is no better.
    let (mut stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    stream.write_all(&client_hello("192.0.2.1")).await?;
    let _ = stream.read_to_end(&mut rest).await;
    assert_eq!(harness.closed_event(1).await.sni, "");
    Ok(())
}

/// A plain HTTP origin that answers every request with `200 hello`, remembering the request head.
async fn http_origin() -> Result<(SocketAddr, tokio::sync::mpsc::UnboundedReceiver<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let Ok(byte) = stream.read_u8().await else {
                        return;
                    };
                    head.push(byte);
                }
                let _ = tx.send(String::from_utf8_lossy(&head).into_owned());
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                    )
                    .await;
            });
        }
    });
    Ok((addr, rx))
}

#[tokio::test]
async fn plain_http_is_forwarded_for_an_explicit_port_80_entry() -> Result<()> {
    let (origin, mut heads) = http_origin().await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (mut stream, response) = request(
        harness.proxy,
        "GET http://pypi.org/simple/?a=1 HTTP/1.1\r\nHost: pypi.org\r\nProxy-Connection: keep-alive\r\n\r\n",
    )
    .await?;
    let mut body = String::new();
    stream.read_to_string(&mut body).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(body, "hello");

    let head = heads.recv().await.unwrap().to_lowercase();
    assert!(head.starts_with("get /simple/?a=1 http/1.1"), "{head}");
    assert!(head.contains("host: pypi.org\r\n"), "{head}");
    assert!(!head.contains("proxy-connection"), "{head}");
    let event = harness.closed_event(0).await;
    assert_eq!((event.method.as_str(), event.http_status), ("GET", 200));
    assert_eq!(event.verdict(), Verdict::Allowed);
    Ok(())
}

#[tokio::test]
async fn plain_http_without_port_80_entry_gets_403() -> Result<()> {
    let (origin, _heads) = http_origin().await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("deb.debian.org", 443)]),
        &[("deb.debian.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (_, response) = request(
        harness.proxy,
        "GET http://deb.debian.org/ HTTP/1.1\r\nHost: deb.debian.org\r\n\r\n",
    )
    .await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert_eq!(harness.closed_event(0).await.reason, "not-allowlisted");
    Ok(())
}

#[tokio::test]
async fn requests_that_are_not_proxy_requests_get_400() -> Result<()> {
    let harness = Harness::start(None, &[], "127.0.0.1:1".parse()?).await?;
    let (_, response) = request(harness.proxy, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").await?;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    Ok(())
}

#[tokio::test]
async fn removing_a_host_closes_the_open_tunnel_and_denies_the_next_connect() -> Result<()> {
    let (upstream, tls) = tls_echo_server("registry.npmjs.org").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("registry.npmjs.org", 443)]),
        &[("registry.npmjs.org", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (stream, _) = request(harness.proxy, &connect_head("registry.npmjs.org", 443)).await?;
    let mut tls = tls
        .connect("registry.npmjs.org".try_into()?, stream)
        .await?;
    tls.write_all(b"ping").await?;
    tls.read_exact(&mut [0; 4]).await?;

    harness.policy.send_replace(policy_of("127.0.0.1", vec![]));

    // The proxy ends the tunnel: reading yields EOF or an error, never data.
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut rest)).await?;
    assert!(rest.is_empty());
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "removed-from-allowlist");
    assert_eq!(event.verdict(), Verdict::Denied);
    // A tunnel closed from outside still reports what it carried.
    assert!(event.bytes_out > 4 && event.bytes_in > 4, "{event:?}");

    let (_, response) = request(harness.proxy, &connect_head("registry.npmjs.org", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    Ok(())
}

#[tokio::test]
async fn an_added_host_works_on_the_next_connection() -> Result<()> {
    let (upstream, tls) = tls_echo_server("pypi.org").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![]),
        &[("pypi.org", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (_, response) = request(harness.proxy, &connect_head("pypi.org", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");

    harness
        .policy
        .send_replace(policy_of("127.0.0.1", vec![rule("pypi.org", 443)]));
    let (stream, response) = request(harness.proxy, &connect_head("pypi.org", 443)).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    tls.connect("pypi.org".try_into()?, stream).await?;
    Ok(())
}

#[tokio::test]
async fn a_tunnel_of_a_sandbox_that_left_the_registry_is_closed_with_that_reason() -> Result<()> {
    let (upstream, tls) = tls_echo_server("api.anthropic.com").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    let mut tls = tls.connect("api.anthropic.com".try_into()?, stream).await?;
    tls.write_all(b"ping").await?;
    tls.read_exact(&mut [0; 4]).await?;

    // The run ended: its pod is no longer in the registry.
    harness.policy.send_replace(policy_of("10.9.9.9", vec![]));
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut rest)).await?;
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "unknown-source");
    assert_eq!(event.verdict(), Verdict::Denied);
    Ok(())
}

#[tokio::test]
async fn plain_http_from_an_unknown_source_gets_403() -> Result<()> {
    let (origin, _heads) = http_origin().await?;
    let harness = Harness::start(
        policy_of("10.1.2.3", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (_, response) = request(
        harness.proxy,
        "GET http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n",
    )
    .await?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert_eq!(harness.closed_event(0).await.reason, "unknown-source");
    Ok(())
}

#[tokio::test]
async fn plain_http_events_count_the_bytes_of_both_bodies() -> Result<()> {
    let (origin, _heads) = http_origin().await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (mut stream, response) = request(
        harness.proxy,
        "POST http://pypi.org/upload HTTP/1.1\r\nHost: pypi.org\r\nContent-Length: 3\r\n\r\nabc",
    )
    .await?;
    let mut body = String::new();
    stream.read_to_string(&mut body).await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(body, "hello");
    let event = harness.closed_event(0).await;
    assert_eq!((event.bytes_out, event.bytes_in), (3, 5), "{event:?}");
    assert!(event.duration.is_some());
    Ok(())
}

/// An origin that accepts connections and never answers.
async fn silent_origin() -> Result<(SocketAddr, TcpListener)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    Ok((listener.local_addr()?, listener))
}

#[tokio::test(start_paused = true)]
async fn an_upstream_that_never_answers_ends_with_502_and_upstream_timeout() -> Result<()> {
    let (origin, listener) = silent_origin().await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (_, response) = request(
        harness.proxy,
        "POST http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\nContent-Length: 3\r\n\r\nabc",
    )
    .await?;
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "upstream-timeout");
    assert_eq!(
        event.bytes_out, 3,
        "what was sent before the timeout is counted"
    );

    // The proxy closed its connection to the upstream: the socket does not leak.
    let (mut upstream, _) = listener.accept().await?;
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), upstream.read_to_end(&mut received)).await??;
    assert!(
        received.ends_with(b"abc"),
        "the request reached the upstream"
    );
    Ok(())
}

#[tokio::test]
async fn stopping_the_proxy_closes_open_tunnels_and_records_them() -> Result<()> {
    let (upstream, tls) = tls_echo_server("api.anthropic.com").await?;
    let mut harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    let mut tls = tls.connect("api.anthropic.com".try_into()?, stream).await?;
    tls.write_all(b"ping").await?;
    tls.read_exact(&mut [0; 4]).await?;

    harness.shutdown().await;
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "gateway-shutdown");
    assert_eq!(event.verdict(), Verdict::Allowed, "a rollout is no denial");
    assert!(event.bytes_out > 4 && event.bytes_in > 4, "{event:?}");
    Ok(())
}

#[tokio::test]
async fn stopping_the_proxy_answers_a_waiting_http_request_and_records_it() -> Result<()> {
    let (origin, listener) = silent_origin().await?;
    let mut harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let mut client = TcpStream::connect(harness.proxy).await?;
    client
        .write_all(b"GET http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n")
        .await?;
    // The origin accepted: the proxy is waiting for response headers.
    let (_upstream, _) = listener.accept().await?;

    harness.shutdown().await;
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_string(&mut response)).await??;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "gateway-shutdown");
    assert_eq!(event.verdict(), Verdict::Allowed);
    assert_eq!(harness.events().len(), 1, "exactly one event per request");
    Ok(())
}

#[tokio::test]
async fn stopping_the_proxy_cuts_off_a_streaming_response_and_records_why() -> Result<()> {
    let (origin, listener) = silent_origin().await?;
    let mut harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let mut client = TcpStream::connect(harness.proxy).await?;
    client
        .write_all(b"GET http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n")
        .await?;
    // The origin starts a body of 10 bytes and stops after 3.
    let (mut upstream, _) = listener.accept().await?;
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(upstream.read_u8().await?);
    }
    upstream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")
        .await?;
    // The client has the start of the body, so the proxy is streaming it.
    let mut received = Vec::new();
    while !received.ends_with(b"abc") {
        received.push(tokio::time::timeout(Duration::from_secs(5), client.read_u8()).await??);
    }

    harness.shutdown().await;
    let event = harness.closed_event(0).await;
    assert_eq!(event.reason, "gateway-shutdown");
    assert_eq!(event.verdict(), Verdict::Allowed);
    assert_eq!((event.http_status, event.bytes_in), (200, 3));
    // Opened when the headers arrived, closed when the body was cut off.
    let phases: Vec<_> = harness.events().iter().map(FlowEvent::phase).collect();
    assert_eq!(phases, [Phase::Opened, Phase::Closed]);
    Ok(())
}

#[tokio::test]
async fn a_tunnel_is_visible_as_opened_while_it_is_open_and_closes_with_its_bytes() -> Result<()> {
    let (upstream, tls) = tls_echo_server("api.anthropic.com").await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream,
    )
    .await?;
    let (stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    let mut tls = tls.connect("api.anthropic.com".try_into()?, stream).await?;
    tls.write_all(b"ping").await?;
    tls.read_exact(&mut [0; 4]).await?;

    // Still open: only the opened event exists.
    let opened = harness.event(0).await;
    assert_eq!(opened.phase(), Phase::Opened);
    assert_eq!(opened.verdict(), Verdict::Allowed);
    assert_eq!(opened.sni, "api.anthropic.com");
    assert_eq!(opened.matched_rule, "api.anthropic.com");
    assert_eq!(harness.events().len(), 1);

    drop(tls);
    let closed = harness.closed_event(0).await;
    assert_eq!(closed.phase(), Phase::Closed);
    assert_ne!(opened.flow_id, 0);
    assert_eq!(closed.flow_id, opened.flow_id, "one flow id per request");
    assert!(closed.bytes_out > 4 && closed.bytes_in > 4);
    assert_eq!(harness.events().len(), 2);
    let at = |event: &FlowEvent| event.time.map(|t| (t.seconds, t.nanos));
    assert!(
        at(&closed) >= at(&opened),
        "closing is recorded after opening"
    );
    Ok(())
}

#[tokio::test]
async fn a_plain_http_exchange_is_opened_when_the_headers_arrive() -> Result<()> {
    let (origin, _heads) = http_origin().await?;
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![rule("pypi.org", 80)]),
        &[("pypi.org", PUBLIC_IP)],
        origin,
    )
    .await?;
    let (mut stream, _) = request(
        harness.proxy,
        "GET http://pypi.org/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n",
    )
    .await?;
    stream.read_to_string(&mut String::new()).await?;
    let opened = harness.opened_event(0).await;
    assert_eq!(
        (opened.http_status, opened.verdict()),
        (200, Verdict::Allowed)
    );
    let closed = harness.closed_event(0).await;
    assert_eq!(closed.bytes_in, 5);
    Ok(())
}

#[tokio::test]
async fn every_request_gets_its_own_flow_id_denied_ones_too() -> Result<()> {
    let harness = Harness::start(
        policy_of("127.0.0.1", vec![]),
        &[("a.example.com", PUBLIC_IP)],
        "127.0.0.1:1".parse()?,
    )
    .await?;
    for _ in 0..2 {
        let (_, response) = request(harness.proxy, &connect_head("a.example.com", 443)).await?;
        assert!(response.starts_with("HTTP/1.1 403"));
    }
    let first = harness.closed_event(0).await;
    let second = harness.closed_event(1).await;
    assert!(first.flow_id != second.flow_id);
    // Seeded with the time, so a restarted pod does not reuse the ids of its previous run.
    assert!(first.flow_id > 1_700_000_000_000_000, "{}", first.flow_id);
    Ok(())
}

fn probed() -> (
    df_agent_channel::metrics::probe::Probe,
    Arc<crate::metrics::Metrics>,
) {
    let probe = df_agent_channel::metrics::probe::Probe::new();
    let metrics = Arc::new(crate::metrics::Metrics::new(&probe.meter()));
    (probe, metrics)
}

#[tokio::test]
async fn a_denied_connect_is_counted_with_its_reason_and_no_host() -> Result<()> {
    let (probe, metrics) = probed();
    let harness = Harness::start_with(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("example.com", PUBLIC_IP)],
        "127.0.0.1:1".parse()?,
        metrics,
    )
    .await?;

    request(harness.proxy, &connect_head("example.com", 443)).await?;
    harness.closed_event(0).await;

    let denied = [("decision", "denied"), ("reason", "not-allowlisted")];
    assert_eq!(probe.total("df.gateway.connections", &denied), 1.0);
    assert_eq!(
        probe.total("df.gateway.connections", &[("decision", "allowed")]),
        0.0
    );
    assert!(probe.values("df.gateway.connections", "host").is_empty());
    assert_eq!(probe.total("df.gateway.active_connections", &[]), 0.0);
    Ok(())
}

#[tokio::test]
async fn an_allowed_tunnel_is_counted_active_while_open_and_reports_its_bytes() -> Result<()> {
    let (probe, metrics) = probed();
    let (upstream, tls) = tls_echo_server("api.anthropic.com").await?;
    let harness = Harness::start_with(
        policy_of("127.0.0.1", vec![rule("api.anthropic.com", 443)]),
        &[("api.anthropic.com", PUBLIC_IP)],
        upstream,
        metrics,
    )
    .await?;

    let (stream, _) = request(harness.proxy, &connect_head("api.anthropic.com", 443)).await?;
    let mut tls = tls.connect("api.anthropic.com".try_into()?, stream).await?;
    tls.write_all(b"ping").await?;
    let mut echoed = [0; 4];
    tls.read_exact(&mut echoed).await?;
    assert_eq!(probe.total("df.gateway.active_connections", &[]), 1.0);
    drop(tls);
    harness.closed_event(0).await;

    let allowed = [("decision", "allowed"), ("reason", "none")];
    assert_eq!(probe.total("df.gateway.connections", &allowed), 1.0);
    assert_eq!(probe.total("df.gateway.active_connections", &[]), 0.0);
    assert!(probe.total("df.gateway.bytes", &[("direction", "upstream")]) > 4.0);
    assert!(probe.total("df.gateway.bytes", &[("direction", "downstream")]) > 4.0);
    assert!(probe.values("df.gateway.bytes", "host").is_empty());
    Ok(())
}
