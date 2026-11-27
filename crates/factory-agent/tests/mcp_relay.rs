//! `factory-agent mcp-relay` against a local TLS server that plays the gateway's MCP endpoint
//! (iter4-mcp-gateway design D8): the real binary, started like the harness starts it.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "helper functions of an integration test"
)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response};
use hyper_util::rt::TokioIo;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

/// Cargo sets `CARGO_BIN_EXE_*` at compile time; Bazel passes the path in `DF_TEST_BIN`.
fn binary() -> PathBuf {
    let path = option_env!("CARGO_BIN_EXE_factory-agent")
        .map(PathBuf::from)
        .or_else(|| std::env::var("DF_TEST_BIN").ok().map(PathBuf::from))
        .expect("binary path (CARGO_BIN_EXE_factory-agent or DF_TEST_BIN)");
    std::path::absolute(path).expect("absolute binary path")
}

/// A private CA and a server certificate for `localhost`.
struct Pki {
    ca: String,
    cert: String,
    key: String,
}

fn pki() -> Pki {
    let mut ca = CertificateParams::new(Vec::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let mut server = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let cert = server.signed_by(&key, &issuer).unwrap();
    Pki {
        ca: ca_cert.pem(),
        cert: cert.pem(),
        key: key.serialize_pem(),
    }
}

/// What the gateway received.
#[derive(Debug, Clone)]
struct Seen {
    headers: HeaderMap,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

/// How the gateway answers.
#[derive(Clone)]
enum Reply {
    Json(Value),
    /// A JSON body that is pretty-printed over several lines.
    Pretty(Value),
    JsonWith(Vec<(&'static str, String)>, Value),
    Status(u16),
    /// The answer is held back until the gate opens.
    Gated(Arc<Notify>, Box<Reply>),
}

type Handler = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

struct Gateway {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Gateway {
    async fn start(pki: &Pki, handler: Handler) -> Self {
        let certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(pki.cert.as_bytes())
                .map(Result::unwrap)
                .collect();
        let key = PrivateKeyDer::from_pem_slice(pki.key.as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();
        let task = tokio::spawn(async move {
            // The connections belong to this task: aborting it ends them too.
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _peer)) = accepted else { break };
                        let (acceptor, handler, seen) =
                            (acceptor.clone(), handler.clone(), seen_in.clone());
                        connections.spawn(serve_connection(stream, acceptor, handler, seen));
                    }
                    Some(_finished) = connections.join_next() => {}
                }
            }
        });
        Self { addr, seen, task }
    }

    fn url(&self) -> String {
        format!("https://localhost:{}/mcp", self.addr.port())
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Serves one connection of the relay: the TLS handshake, then HTTP/1.1 requests.
async fn serve_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    handler: Handler,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    let Ok(stream) = acceptor.accept(stream).await else {
        return;
    };
    let service = service_fn(move |request: Request<Incoming>| {
        let (handler, seen) = (handler.clone(), seen.clone());
        async move { Ok::<_, Infallible>(answer(&handler, &seen, request).await) }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

async fn answer(
    handler: &Handler,
    seen: &Mutex<Vec<Seen>>,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let (parts, body) = request.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    let record = Seen {
        headers: parts.headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let reply = handler(&record);
    seen.lock().unwrap().push(record);
    render(reply).await
}

async fn render(reply: Reply) -> Response<Full<Bytes>> {
    match reply {
        Reply::Json(value) => respond(200, &[], value.to_string()),
        Reply::Pretty(value) => respond(200, &[], serde_json::to_string_pretty(&value).unwrap()),
        Reply::JsonWith(headers, value) => respond(200, &headers, value.to_string()),
        Reply::Status(status) => respond(status, &[], String::new()),
        Reply::Gated(gate, inner) => {
            gate.notified().await;
            Box::pin(render(*inner)).await
        }
    }
}

fn respond(status: u16, headers: &[(&'static str, String)], body: String) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    for (name, value) in headers {
        response.headers_mut().insert(*name, value.parse().unwrap());
    }
    response
}

/// The relay process with its pipes.
struct RelayProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
}

impl RelayProcess {
    fn start(url: &str, ca: &Path, env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(binary());
        command
            .args(["mcp-relay", "--url", url, "--ca"])
            .arg(ca)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            stdin,
            stdout,
        }
    }

    async fn send(&mut self, message: &Value) {
        self.send_raw(&message.to_string()).await;
    }

    async fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
        stdin.flush().await.unwrap();
    }

    /// The next line of stdout; fails the test if none comes within 10 s.
    async fn line(&mut self) -> String {
        tokio::time::timeout(Duration::from_secs(10), self.stdout.next_line())
            .await
            .expect("no line on stdout")
            .unwrap()
            .expect("stdout closed")
    }

    async fn answer(&mut self) -> Value {
        serde_json::from_str(&self.line().await).unwrap()
    }

    /// Closes stdin and waits for the exit; returns the status and stderr.
    async fn finish(mut self) -> (std::process::ExitStatus, String, Vec<String>) {
        drop(self.stdin.take());
        let mut stderr = String::new();
        let mut err = self.child.stderr.take().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .expect("the relay did not exit")
            .unwrap();
        err.read_to_string(&mut stderr).await.unwrap();
        let mut rest = Vec::new();
        while let Ok(Some(line)) = self.stdout.next_line().await {
            rest.push(line);
        }
        (status, stderr, rest)
    }
}

fn ca_file(dir: &tempfile::TempDir, pki: &Pki) -> PathBuf {
    let path = dir.path().join("ca.crt");
    std::fs::write(&path, &pki.ca).unwrap();
    path
}

fn ok_handler() -> Handler {
    Arc::new(|seen: &Seen| {
        let message: Value = serde_json::from_str(&seen.body).unwrap_or(Value::Null);
        match message.get("id") {
            Some(id) => Reply::Json(
                json!({"jsonrpc": "2.0", "id": id, "result": {"echo": message["method"]}}),
            ),
            None => Reply::Status(202),
        }
    })
}

#[tokio::test]
async fn messages_are_forwarded_with_the_same_content_and_answers_come_back_as_one_compact_line() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(
        &pki,
        Arc::new(|seen: &Seen| {
            let id = serde_json::from_str::<Value>(&seen.body).unwrap()["id"].clone();
            Reply::Pretty(json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [{"name": "wiki__search"}]}}))
        }),
    )
    .await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);
    let request =
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {"cursor": "x"}});

    relay.send(&request).await;
    let line = relay.line().await;

    assert!(!line.contains('\n') && !line.contains("  "), "{line}");
    let answer: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        answer,
        json!({"jsonrpc": "2.0", "id": 3, "result": {"tools": [{"name": "wiki__search"}]}})
    );
    let seen = gateway.requests();
    assert_eq!(
        serde_json::from_str::<Value>(&seen[0].body).unwrap(),
        request
    );
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert_eq!(
        seen[0].header("authorization"),
        None,
        "the relay sends no credential"
    );
}

#[tokio::test]
async fn the_session_id_and_protocol_version_of_initialize_are_sent_with_the_next_requests() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(
        &pki,
        Arc::new(|seen: &Seen| {
            let message: Value = serde_json::from_str(&seen.body).unwrap();
            if message["method"] == "initialize" {
                Reply::JsonWith(
                    vec![("mcp-session-id", "df-run-abc".into())],
                    json!({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": "2025-11-25"}}),
                )
            } else {
                Reply::Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": {}}))
            }
        }),
    )
    .await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    relay
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}))
        .await;
    relay.answer().await;
    relay
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    relay.answer().await;

    let seen = gateway.requests();
    assert_eq!(seen[0].header("mcp-session-id"), None);
    assert_eq!(seen[1].header("mcp-session-id"), Some("df-run-abc"));
    assert_eq!(seen[1].header("mcp-protocol-version"), Some("2025-11-25"));
}

#[tokio::test]
async fn requests_go_directly_to_the_gateway_even_with_a_proxy_in_the_environment() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(&pki, ok_handler()).await;
    // Nothing listens on port 1: a request through this proxy could not succeed.
    let proxy = [
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("https_proxy", "http://127.0.0.1:1"),
        ("ALL_PROXY", "http://127.0.0.1:1"),
    ];
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &proxy);

    relay
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .await;

    assert_eq!(relay.answer().await["result"]["echo"], "ping");
}

#[tokio::test]
async fn only_the_instance_ca_is_trusted() {
    let gateway_pki = pki();
    let other_ca = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(&gateway_pki, ok_handler()).await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &other_ca), &[]);

    relay
        .send(&json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}))
        .await;

    let answer = relay.answer().await;
    assert_eq!(answer["id"], 9);
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("gateway unreachable"),
        "{answer}"
    );
    assert!(gateway.requests().is_empty());
}

#[tokio::test]
async fn an_unreachable_gateway_is_answered_per_request_and_the_relay_keeps_running() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://localhost:{}/mcp",
        closed.local_addr().unwrap().port()
    );
    drop(closed);
    let mut relay = RelayProcess::start(&url, &ca_file(&dir, &pki), &[]);

    for id in [1, 2] {
        relay
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}))
            .await;
        let answer = relay.answer().await;
        assert_eq!(answer["id"], id);
        assert!(
            answer["error"]["message"]
                .as_str()
                .unwrap()
                .contains("gateway unreachable"),
            "{answer}"
        );
    }
}

#[tokio::test]
async fn an_http_error_status_for_a_request_becomes_an_error_for_its_id() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(&pki, Arc::new(|_: &Seen| Reply::Status(403))).await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    relay
        .send(&json!({"jsonrpc": "2.0", "id": "c-1", "method": "tools/call", "params": {}}))
        .await;

    let answer = relay.answer().await;
    assert_eq!(answer["id"], "c-1");
    assert!(
        answer["error"]["message"].as_str().unwrap().contains("403"),
        "{answer}"
    );
}

#[tokio::test]
async fn notifications_write_nothing_to_stdout_and_a_refusal_is_logged_on_stderr() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(
        &pki,
        Arc::new(|seen: &Seen| {
            if seen.body.contains("notifications/cancelled") {
                Reply::Status(400)
            } else {
                ok_handler()(seen)
            }
        }),
    )
    .await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    relay
        .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    relay.send(&json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}})).await;
    relay
        .send(&json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}))
        .await;

    // The first line on stdout is the answer to the request: the notifications wrote none.
    assert_eq!(relay.answer().await["id"], 5);
    let (status, stderr, rest) = relay.finish().await;
    assert!(status.success());
    assert!(rest.is_empty(), "{rest:?}");
    assert!(stderr.contains("400"), "{stderr}");
}

#[tokio::test]
async fn a_slow_call_does_not_delay_a_ping() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Notify::new());
    let held = gate.clone();
    let gateway = Gateway::start(
        &pki,
        Arc::new(move |seen: &Seen| {
            let message: Value = serde_json::from_str(&seen.body).unwrap();
            let reply = Reply::Json(
                json!({"jsonrpc": "2.0", "id": message["id"], "result": {"m": message["method"]}}),
            );
            if message["method"] == "tools/call" {
                Reply::Gated(held.clone(), Box::new(reply))
            } else {
                reply
            }
        }),
    )
    .await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    relay
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {}}))
        .await;
    relay
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;

    assert_eq!(relay.answer().await["id"], 2, "the ping is answered first");
    gate.notify_waiters();
    // `notify_waiters` reaches only waiters that already wait; the call may still be on its way.
    let first = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            gate.notify_waiters();
            if let Ok(line) =
                tokio::time::timeout(Duration::from_millis(100), relay.stdout.next_line()).await
            {
                break line.unwrap().unwrap();
            }
        }
    })
    .await
    .expect("the held call was not answered after the gate opened");
    assert_eq!(serde_json::from_str::<Value>(&first).unwrap()["id"], 1);
}

#[tokio::test]
async fn the_relay_exits_with_status_0_when_the_harness_closes_stdin() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(&pki, ok_handler()).await;
    let relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    let (status, _stderr, _rest) = relay.finish().await;

    assert!(status.success());
}

#[tokio::test]
async fn an_unreadable_ca_file_ends_the_relay_with_an_error_naming_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-ca.crt");
    let relay = RelayProcess::start("https://localhost:1/mcp", &missing, &[]);

    let (status, stderr, _rest) = relay.finish().await;

    assert!(!status.success());
    assert!(stderr.contains("missing-ca.crt"), "{stderr}");
}

#[tokio::test]
async fn a_line_that_is_not_json_is_ignored() {
    let pki = pki();
    let dir = tempfile::tempdir().unwrap();
    let gateway = Gateway::start(&pki, ok_handler()).await;
    let mut relay = RelayProcess::start(&gateway.url(), &ca_file(&dir, &pki), &[]);

    relay.send_raw("this is not json").await;
    relay
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .await;

    assert_eq!(relay.answer().await["id"], 1);
    assert_eq!(gateway.requests().len(), 1);
}
