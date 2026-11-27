//! Test support: a fake upstream MCP server over plain HTTP that keeps what it received.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::resolve::Resolver;

/// What the fake upstream received.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub method: Method,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Seen {
    /// The JSON-RPC message of the body, `Null` if there is none.
    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    /// The JSON-RPC method, if the body has one.
    pub(crate) fn rpc_method(&self) -> Option<String> {
        self.json()
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

/// How the fake upstream answers one request.
#[derive(Clone)]
pub(crate) enum Answer {
    /// `200` with a JSON body.
    Json(Value),
    /// `200` with a JSON body and extra headers.
    JsonWith(Vec<(&'static str, String)>, Value),
    /// `200` as server-sent events, one event per `data` entry (raw `data:` payloads).
    Sse(Vec<String>),
    /// Like [`Answer::Sse`] with extra headers.
    SseWith(Vec<(&'static str, String)>, Vec<String>),
    /// `202` without a body.
    Accepted,
    /// An empty answer with this status.
    Status(u16),
    /// A redirect to `Location`.
    Redirect(&'static str),
    /// A body of any content type.
    Raw(&'static str, String),
    /// No answer at all.
    Stall,
    /// The answer after a delay.
    After(Duration, Box<Answer>),
    /// The answer once the test calls `notify_one` on the gate.
    Gated(Arc<tokio::sync::Notify>, Box<Answer>),
}

pub(crate) type Handler = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

pub(crate) struct FakeMcp {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: JoinHandle<()>,
}

impl FakeMcp {
    /// A server that follows the MCP handshake and offers `tools`.
    pub(crate) async fn start(tools: Vec<Value>) -> Self {
        Self::start_with(Arc::new(Standard::new(tools)).handler()).await
    }

    pub(crate) async fn start_with(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(handler));
        let (seen_in, handler_in) = (seen.clone(), handler.clone());
        let task = tokio::spawn(async move {
            while let Ok((stream, _peer)) = listener.accept().await {
                tokio::spawn(serve(stream, seen_in.clone(), handler_in.clone()));
            }
        });
        Self { addr, seen, task }
    }

    pub(crate) fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    pub(crate) fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// The JSON-RPC methods received so far (`DELETE` as `DELETE`).
    pub(crate) fn methods(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|seen| {
                if seen.method == Method::DELETE {
                    "DELETE".to_owned()
                } else {
                    seen.rpc_method().unwrap_or_else(|| "response".into())
                }
            })
            .collect()
    }
}

impl Drop for FakeMcp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    stream: tokio::net::TcpStream,
    seen: Arc<Mutex<Vec<Seen>>>,
    handler: Arc<Mutex<Handler>>,
) {
    let service = service_fn(move |request: Request<Incoming>| {
        let (seen, handler) = (seen.clone(), handler.clone());
        async move { Ok::<_, Infallible>(respond(&seen, &handler, request).await) }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

type Reply = Response<http_body_util::combinators::UnsyncBoxBody<Bytes, Infallible>>;

async fn respond(
    seen: &Mutex<Vec<Seen>>,
    handler: &Mutex<Handler>,
    request: Request<Incoming>,
) -> Reply {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map_or_default(http_body_util::Collected::to_bytes);
    let record = Seen {
        method: parts.method,
        path_and_query: parts
            .uri
            .path_and_query()
            .map_or_default(ToString::to_string),
        headers: parts.headers,
        body,
    };
    let handler = handler.lock().unwrap().clone();
    let answer = handler(&record);
    seen.lock().unwrap().push(record);
    render(answer).await
}

async fn render(answer: Answer) -> Reply {
    match answer {
        Answer::Json(value) => build(200, &[], "application/json", value.to_string()),
        Answer::JsonWith(headers, value) => {
            build(200, &headers, "application/json", value.to_string())
        }
        Answer::Sse(events) => sse(&[], &events),
        Answer::SseWith(headers, events) => sse(&headers, &events),
        Answer::Accepted => build(202, &[], "application/json", String::new()),
        Answer::Status(status) => build(status, &[], "application/json", String::new()),
        Answer::Redirect(location) => build(
            307,
            &[("location", location.to_owned())],
            "text/plain",
            String::new(),
        ),
        Answer::Raw(content_type, body) => build(200, &[], content_type, body),
        Answer::Stall => std::future::pending().await,
        Answer::Gated(gate, inner) => {
            gate.notified().await;
            Box::pin(render(*inner)).await
        }
        Answer::After(delay, inner) => {
            tokio::time::sleep(delay).await;
            Box::pin(render(*inner)).await
        }
    }
}

fn sse(headers: &[(&'static str, String)], events: &[String]) -> Reply {
    let body = events.iter().fold(String::new(), |mut body, data| {
        write!(body, "event: message\ndata: {data}\n\n").unwrap();
        body
    });
    build(200, headers, "text/event-stream", body)
}

fn build(
    status: u16,
    headers: &[(&'static str, String)],
    content_type: &str,
    body: String,
) -> Reply {
    let mut response = Response::new(Full::new(Bytes::from(body)).boxed_unsync());
    *response.status_mut() = StatusCode::from_u16(status).unwrap();
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_str(content_type).unwrap());
    for (name, value) in headers {
        response.headers_mut().insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    response
}

/// An upstream that enforces the handshake: requests before `notifications/initialized` get 400,
/// an unknown session 404.
pub(crate) struct Standard {
    tools: Mutex<Vec<Value>>,
    stateless: bool,
    sse: bool,
    page_size: usize,
    sessions: Mutex<HashSet<String>>,
    ready: Mutex<HashSet<String>>,
    counter: AtomicUsize,
}

impl Standard {
    pub(crate) fn new(tools: Vec<Value>) -> Self {
        Self {
            tools: Mutex::new(tools),
            stateless: false,
            sse: false,
            page_size: usize::MAX,
            sessions: Mutex::default(),
            ready: Mutex::default(),
            counter: AtomicUsize::new(0),
        }
    }

    /// No `Mcp-Session-Id`, like Context7.
    #[must_use]
    pub(crate) fn stateless(mut self) -> Self {
        self.stateless = true;
        self
    }

    /// Answers requests as server-sent events.
    #[must_use]
    pub(crate) fn sse(mut self) -> Self {
        self.sse = true;
        self
    }

    #[must_use]
    pub(crate) fn pages_of(mut self, size: usize) -> Self {
        self.page_size = size;
        self
    }

    pub(crate) fn handler(self: Arc<Self>) -> Handler {
        Arc::new(move |seen| self.answer(seen))
    }

    /// Forgets every session, so the next request of one gets 404.
    pub(crate) fn expire_sessions(&self) {
        self.sessions.lock().unwrap().clear();
        self.ready.lock().unwrap().clear();
    }

    fn reply(&self, value: Value) -> Answer {
        if self.sse {
            Answer::Sse(vec![value.to_string()])
        } else {
            Answer::Json(value)
        }
    }

    fn answer(&self, seen: &Seen) -> Answer {
        if seen.method == Method::DELETE {
            if let Some(id) = seen.header("mcp-session-id") {
                self.sessions.lock().unwrap().remove(id);
            }
            return Answer::Status(204);
        }
        let message = seen.json();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        if method == "initialize" {
            return self.initialize(&id);
        }
        let session = seen.header("mcp-session-id").unwrap_or_default().to_owned();
        if !self.stateless && !self.sessions.lock().unwrap().contains(&session) {
            return Answer::Status(if session.is_empty() { 400 } else { 404 });
        }
        if method == "notifications/initialized" {
            self.ready.lock().unwrap().insert(session);
            return Answer::Accepted;
        }
        if !self.ready.lock().unwrap().contains(&session) {
            return Answer::Raw("text/plain", "not initialized".into());
        }
        match method {
            "tools/list" => self.list(&id, &message),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                let arguments = message["params"]["arguments"].clone();
                self.reply(json!({"jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": format!("called {name} {arguments}")}]}}))
            }
            _ if message.get("id").is_none() => Answer::Accepted,
            _ => self.reply(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "Method not found"}})),
        }
    }

    fn initialize(&self, id: &Value) -> Answer {
        let result = json!({"jsonrpc": "2.0", "id": id, "result": {
            "protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
            "serverInfo": {"name": "fake", "version": "1"}}});
        if self.stateless {
            self.ready.lock().unwrap().insert(String::new());
            return self.reply(result);
        }
        let session = format!("sess-{}", self.counter.fetch_add(1, Ordering::Relaxed) + 1);
        self.sessions.lock().unwrap().insert(session.clone());
        if self.sse {
            return Answer::SseWith(vec![("mcp-session-id", session)], vec![result.to_string()]);
        }
        Answer::JsonWith(vec![("mcp-session-id", session)], result)
    }

    fn list(&self, id: &Value, message: &Value) -> Answer {
        let tools = self.tools.lock().unwrap().clone();
        let start: usize = message["params"]["cursor"]
            .as_str()
            .and_then(|cursor| cursor.parse().ok())
            .unwrap_or(0);
        let end = start.saturating_add(self.page_size).min(tools.len());
        let mut result = json!({"tools": tools.get(start..end).unwrap_or_default()});
        if end < tools.len() {
            result["nextCursor"] = json!(end.to_string());
        }
        self.reply(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

/// A resolver with fixed answers.
pub(crate) struct StaticResolver(pub HashMap<String, Vec<IpAddr>>);

#[async_trait]
impl Resolver for StaticResolver {
    async fn resolve(&self, host: &str) -> std::io::Result<Vec<IpAddr>> {
        self.0
            .get(host)
            .cloned()
            .ok_or_else(|| std::io::Error::other("no such host"))
    }
}

/// A tool as an upstream lists it.
pub(crate) fn tool(name: &str) -> Value {
    json!({"name": name, "description": format!("the {name} tool"),
        "inputSchema": {"type": "object", "properties": {}}})
}
