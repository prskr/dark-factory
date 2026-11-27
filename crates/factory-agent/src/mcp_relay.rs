//! `factory-agent mcp-relay`: a transport bridge between a harness's stdio MCP client and the
//! gateway's MCP endpoint (iter4-mcp-gateway design D8).
//!
//! Each JSON-RPC line from stdin becomes one `POST` to the endpoint, each answer one compact line
//! on stdout. The relay is no MCP server: it reads only what the transport needs (the session
//! header, the protocol version of `initialize`, the request id for errors). It connects directly
//! (never through the egress proxy), trusts only the instance CA, sends no credential and opens no
//! listening socket.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{ACCEPT, CONTENT_TYPE, HOST};
use hyper::{Request, Uri};
use hyper_util::rt::TokioIo;
use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_rustls::TlsConnector;

/// Time to connect and complete the TLS handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Time one request may take end to end; the gateway's own call limit is 120 s.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
/// Time requests in flight get to finish after stdin ended.
const EOF_GRACE: Duration = Duration::from_secs(2);
/// JSON-RPC: server error, the code of every error the relay answers with.
const RELAY_ERROR: i64 = -32000;

/// The endpoint and the CA the relay trusts.
pub struct Relay {
    host: String,
    port: u16,
    target: String,
    authority: String,
    tls: TlsConnector,
    session: Mutex<Session>,
}

/// What the endpoint told the relay in the answer to `initialize`.
#[derive(Debug, Default)]
struct Session {
    id: Option<String>,
    protocol: Option<String>,
}

impl Relay {
    /// Reads the CA at `ca_path` and parses `url` (`https://host[:port]/path`).
    pub fn new(url: &str, ca_path: &Path) -> anyhow::Result<Self> {
        let pem = std::fs::read(ca_path)
            .with_context(|| format!("cannot read the CA file {}", ca_path.display()))?;
        let mut roots = RootCertStore::empty();
        let mut added = 0;
        for cert in CertificateDer::pem_slice_iter(&pem) {
            let cert = cert.with_context(|| format!("invalid CA file {}", ca_path.display()))?;
            roots.add(cert).context("invalid CA certificate")?;
            added += 1;
        }
        anyhow::ensure!(
            added > 0,
            "no certificate in the CA file {}",
            ca_path.display()
        );
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let uri: Uri = url
            .parse()
            .with_context(|| format!("invalid endpoint URL {url}"))?;
        anyhow::ensure!(
            uri.scheme_str() == Some("https"),
            "the endpoint URL must be https"
        );
        let host = uri
            .host()
            .context("the endpoint URL has no host")?
            .trim_matches(['[', ']'])
            .to_owned();
        let port = uri.port_u16().unwrap_or(443);
        Ok(Self {
            host,
            port,
            target: uri
                .path_and_query()
                .map_or_else(|| "/".into(), ToString::to_string),
            authority: uri.authority().map(ToString::to_string).unwrap_or_default(),
            tls: TlsConnector::from(Arc::new(config)),
            session: Mutex::new(Session::default()),
        })
    }

    /// Forwards the lines of `input` and writes the answers to `output` until `input` ends.
    /// Requests run concurrently; one writer keeps the lines apart.
    pub async fn run<R, W>(self: Arc<Self>, input: R, output: W) -> anyhow::Result<()>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (lines, mut written) = mpsc::unbounded_channel::<String>();
        let writer = tokio::spawn(async move {
            let mut output = output;
            while let Some(line) = written.recv().await {
                let mut line = line.into_bytes();
                line.push(b'\n');
                if output.write_all(&line).await.is_err() || output.flush().await.is_err() {
                    return;
                }
            }
        });
        let mut tasks = JoinSet::new();
        let mut input = input.lines();
        while let Some(line) = input.next_line().await.context("cannot read stdin")? {
            if line.trim().is_empty() {
                continue;
            }
            let (relay, lines) = (self.clone(), lines.clone());
            tasks.spawn(async move {
                if let Some(answer) = relay.forward(&line).await {
                    // The writer ended only if stdout is gone; nothing is left to tell.
                    lines.send(answer).ok();
                }
            });
        }
        // The harness is gone: calls in flight have no reader any more. Short ones, such as a
        // notification being delivered, get a moment to finish; the rest ends.
        let _finished = tokio::time::timeout(EOF_GRACE, async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        tasks.abort_all();
        drop(lines);
        writer.await.ok();
        Ok(())
    }

    /// Posts one message. The line to write for it: the gateway's answer, or an error for the
    /// request id; `None` for a notification and for answers without a body.
    async fn forward(&self, line: &str) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                log(&format!("ignoring a line that is not JSON: {error}"));
                return None;
            }
        };
        let id = message
            .get("id")
            .filter(|_| message.get("method").is_some())
            .cloned();
        let is_initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
        match tokio::time::timeout(REQUEST_TIMEOUT, self.post(line, is_initialize)).await {
            Ok(Ok(answer)) => answer.filter(|_| id.is_some()),
            Ok(Err(error)) => self.failed(id, &error),
            Err(_elapsed) => self.failed(id, "gateway did not answer in time"),
        }
    }

    /// The error line for request `id`; a notification only logs.
    fn failed(&self, id: Option<Value>, cause: &str) -> Option<String> {
        match id {
            Some(id) => Some(
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": RELAY_ERROR, "message": cause } }).to_string(),
            ),
            None => {
                log(&format!("notification not delivered: {cause}"));
                None
            }
        }
    }

    /// `Ok(None)` for an answer without body (`202`).
    async fn post(&self, line: &str, is_initialize: bool) -> Result<Option<String>, String> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, self.connect())
            .await
            .map_err(|_elapsed| "gateway unreachable: connect timed out".to_owned())??;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| format!("gateway unreachable: {error}"))?;
        let driver = tokio::spawn(async move {
            connection.await.ok();
        });
        let mut request = Request::builder()
            .method("POST")
            .uri(&self.target)
            .header(HOST, &self.authority)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream");
        {
            let session = self
                .session
                .lock()
                .map_err(|_poisoned| "relay state unavailable".to_owned())?;
            if let Some(id) = &session.id {
                request = request.header("mcp-session-id", id);
            }
            if let Some(protocol) = &session.protocol {
                request = request.header("mcp-protocol-version", protocol);
            }
        }
        let request = request
            .body(Full::new(Bytes::from(line.to_owned())))
            .map_err(|error| format!("invalid request: {error}"))?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|error| format!("gateway unreachable: {error}"))?;
        let (parts, body) = response.into_parts();
        let body = body
            .collect()
            .await
            .map_err(|error| format!("gateway answer interrupted: {error}"))?
            .to_bytes();
        driver.abort();
        if !parts.status.is_success() {
            return Err(format!("gateway answered HTTP {}", parts.status.as_u16()));
        }
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        let answer: Value = serde_json::from_slice(&body)
            .map_err(|_error| "gateway answered with something that is not JSON".to_owned())?;
        if is_initialize {
            self.remember(&parts.headers, &answer);
        }
        Ok(Some(answer.to_string()))
    }

    /// Keeps the session id and the protocol version of an `initialize` answer for the next
    /// requests.
    fn remember(&self, headers: &hyper::HeaderMap, answer: &Value) {
        let Ok(mut session) = self.session.lock() else {
            return;
        };
        session.id = headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        session.protocol = answer
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }

    async fn connect(&self) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
        let tcp = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|error| format!("gateway unreachable: {error}"))?;
        let name = ServerName::try_from(self.host.clone())
            .map_err(|error| format!("invalid gateway name: {error}"))?;
        self.tls
            .connect(name, tcp)
            .await
            .map_err(|error| format!("gateway unreachable: {error}"))
    }
}

#[expect(clippy::print_stderr, reason = "the relay reports problems on stderr")]
fn log(message: &str) {
    eprintln!("mcp-relay: {message}");
}

/// Runs the relay on stdin and stdout.
pub async fn run(url: &str, ca_path: &Path) -> anyhow::Result<()> {
    let relay = Arc::new(Relay::new(url, ca_path)?);
    relay
        .run(
            tokio::io::BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        )
        .await
}
