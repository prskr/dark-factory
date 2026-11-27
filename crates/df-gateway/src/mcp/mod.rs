//! The MCP endpoint (iter4-mcp-gateway): third-party tools of a sandbox, identified by its pod IP,
//! aggregated from the upstream MCP servers its project enables and forwarded with the servers'
//! credentials.
//!
//! It shares the Git port, its TLS and its upstream client (design D3, D4). The downstream
//! session holds no state on the gateway: lists and calls are answered from the snapshot by pod
//! IP, so every gateway replica serves every request (design D6).

mod call;
mod client;
mod jsonrpc;
mod list;
mod record;
pub mod report;
mod sse;
mod target;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
mod url;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::header::{ALLOW, CONTENT_LENGTH, HeaderMap, HeaderValue};
use hyper::{Method, Request, StatusCode};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use self::client::Client;
use self::jsonrpc::{
    INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, empty, is_supported_version, json_response,
    negotiated_version, rpc_error, rpc_result,
};
use self::report::CallSink;
use crate::gateway::{Outside, Settings};
use crate::policy::{Policy, Sandbox};
use crate::resolve::{BlockedRanges, Resolver};
use crate::upstream::{Body, BodyLimit, BoxError, LimitedBody, UpstreamConnector, text};

/// The path of the endpoint.
pub const PATH: &str = "/mcp";

/// Largest request body.
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
/// Longest pause between two frames of the request body.
const REQUEST_IDLE: Duration = Duration::from_secs(60);
/// Upstream sessions idle for this long are ended.
const SESSION_IDLE: Duration = Duration::from_mins(10);
/// How often the janitor looks for idle sessions.
const JANITOR_INTERVAL: Duration = Duration::from_secs(30);

const NO_SNAPSHOT: &str = "df: no policy snapshot yet";
const UNKNOWN_SOURCE: &str = "df: unknown source";

type Response = hyper::Response<Body>;

/// Builds the MCP endpoint of a gateway pod and the reporter that sends its call records. Public
/// servers are checked against the built-in blocked ranges and `settings.blocked_cidrs`; an
/// invalid CIDR is an error.
pub fn start(
    outside: &Outside,
    settings: &Settings,
    connector: UpstreamConnector,
) -> Result<(Arc<Mcp>, Arc<report::CallReporter>), ipnet::AddrParseError> {
    let calls = Arc::new(report::CallReporter::new(
        outside.connector.clone(),
        settings.control_plane.clone(),
        settings.pod.clone(),
        settings.metrics.clone(),
        crate::inference::report::Timing::default(),
    ));
    let blocked = BlockedRanges::new(settings.blocked_cidrs.iter().map(String::as_str))?;
    let mcp = Arc::new(Mcp::new(
        connector,
        outside.resolver.clone(),
        blocked,
        calls.clone(),
    ));
    Ok((mcp, calls))
}

/// Whether `path` belongs to the MCP endpoint.
pub fn is_mcp_path(path: &str) -> bool {
    path == PATH
}

/// A run's aggregated tool list with what it was built from.
struct CachedList {
    signature: Vec<(String, u64)>,
    at: Instant,
    tools: Arc<Vec<Value>>,
}

/// The MCP endpoint of a gateway pod.
pub struct Mcp {
    client: Client,
    sink: Arc<dyn CallSink>,
    lists: Mutex<HashMap<String, CachedList>>,
    /// Calls in progress per run.
    slots: Mutex<HashMap<String, Arc<Semaphore>>>,
    /// Calls that `notifications/cancelled` can end, by run and JSON-RPC request id.
    cancels: Mutex<HashMap<(String, String), CancellationToken>>,
}

impl Mcp {
    /// Talks to upstreams with `connector` (TLS roots, plain HTTP only where the instance allows
    /// it), checks the addresses of public servers with `resolver` and `blocked`, and hands the
    /// record of every call to `sink`.
    pub fn new(
        connector: UpstreamConnector,
        resolver: Arc<dyn Resolver>,
        blocked: BlockedRanges,
        sink: Arc<dyn CallSink>,
    ) -> Self {
        Self {
            client: Client::new(connector, resolver, blocked),
            sink,
            lists: Mutex::new(HashMap::new()),
            slots: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// Shortens the time an upstream `tools/list` may take (see `Client::with_list_timeout`).
    #[cfg(test)]
    pub(crate) fn with_list_timeout(mut self, timeout: Duration) -> Self {
        self.client = self.client.with_list_timeout(timeout);
        self
    }

    /// Answers one request of the sandbox at `source`; `request.uri().path()` is [`PATH`].
    pub async fn handle<B>(
        &self,
        policy: Option<&Policy>,
        source: IpAddr,
        request: Request<B>,
    ) -> Response
    where
        B: hyper::body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let Some(policy) = policy else {
            return text(StatusCode::SERVICE_UNAVAILABLE, NO_SNAPSHOT);
        };
        let Some(sandbox) = policy.sandbox(source) else {
            tracing::warn!(%source, "MCP request denied: unknown source");
            return text(StatusCode::FORBIDDEN, UNKNOWN_SOURCE);
        };
        let (parts, body) = request.into_parts();
        match parts.method {
            Method::POST => {
                self.post(policy, source, sandbox, &parts.headers, body)
                    .await
            }
            Method::DELETE => empty(StatusCode::NO_CONTENT),
            _ => {
                let mut response = empty(StatusCode::METHOD_NOT_ALLOWED);
                response
                    .headers_mut()
                    .insert(ALLOW, HeaderValue::from_static("POST, DELETE"));
                response
            }
        }
    }

    async fn post<B>(
        &self,
        policy: &Policy,
        source: IpAddr,
        sandbox: &Sandbox,
        headers: &HeaderMap,
        body: B,
    ) -> Response
    where
        B: hyper::body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let unsupported = headers
            .get("mcp-protocol-version")
            .is_some_and(|version| !version.to_str().is_ok_and(is_supported_version));
        if unsupported {
            return text(
                StatusCode::BAD_REQUEST,
                "df: unsupported MCP-Protocol-Version",
            );
        }
        let bytes = match read_body(headers, body).await {
            Ok(bytes) => bytes,
            Err(response) => return *response,
        };
        let message = match serde_json::from_slice::<Value>(&bytes) {
            Ok(message) if message.is_object() => message,
            Ok(Value::Array(_)) => {
                let error = rpc_error(&Value::Null, INVALID_REQUEST, "batches are not supported");
                return json_response(StatusCode::BAD_REQUEST, &[], &error);
            }
            Ok(_) | Err(_) => {
                let error = rpc_error(&Value::Null, PARSE_ERROR, "not a JSON-RPC message");
                return json_response(StatusCode::BAD_REQUEST, &[], &error);
            }
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("initialize") {
            return initialize(
                &sandbox.run_id,
                id.as_ref().unwrap_or(&Value::Null),
                &message,
            );
        }
        match session_state(headers, &sandbox.run_id) {
            Session::Missing => return text(StatusCode::BAD_REQUEST, "df: Mcp-Session-Id missing"),
            Session::Foreign => return text(StatusCode::NOT_FOUND, "df: unknown session"),
            Session::Valid => {}
        }
        let Some(method) = method else {
            // A response of the client to a request: the endpoint sends none.
            return empty(StatusCode::ACCEPTED);
        };
        let Some(id) = id else {
            if method == "notifications/cancelled" {
                self.cancel(&sandbox.run_id, message.get("params"));
            }
            return empty(StatusCode::ACCEPTED);
        };
        match method {
            "ping" => json_response(StatusCode::OK, &[], &rpc_result(&id, json!({}))),
            "tools/list" => self.tools_list(policy, source, sandbox, &id).await,
            "tools/call" => {
                let params = message.get("params").unwrap_or(&Value::Null);
                self.tools_call(policy, source, sandbox, &id, params, bytes.len() as u64)
                    .await
            }
            _ => json_response(
                StatusCode::OK,
                &[],
                &rpc_error(&id, METHOD_NOT_FOUND, "method not found"),
            ),
        }
    }

    /// Ends the call `params.requestId` of `run_id` if this pod holds it.
    fn cancel(&self, run_id: &str, params: Option<&Value>) {
        let Some(request_id) = params.and_then(|params| params.get("requestId")) else {
            return;
        };
        let token = self.cancels.lock().ok().and_then(|cancels| {
            cancels
                .get(&(run_id.to_owned(), request_id.to_string()))
                .cloned()
        });
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// Ends upstream sessions of runs and servers that left the snapshot, of servers whose
    /// configuration changed and of sessions idle for ten minutes, and forgets what this pod
    /// kept for runs that are gone.
    pub async fn sweep(&self, policy: Option<&Policy>) {
        let Some(policy) = policy else {
            return;
        };
        self.client
            .sweep(
                &|key, target| {
                    policy.enables_mcp_server(&key.run_id, &key.server_id, target.config_version)
                },
                SESSION_IDLE,
            )
            .await;
        if let Ok(mut lists) = self.lists.lock() {
            lists.retain(|run_id, _| policy.has_run(run_id));
        }
        if let Ok(mut slots) = self.slots.lock() {
            slots.retain(|run_id, slot| {
                policy.has_run(run_id) || slot.available_permits() < call::MAX_CONCURRENT_CALLS
            });
        }
    }

    /// Sweeps whenever the snapshot changes and every 30 seconds, until `stop` is cancelled.
    pub async fn run_janitor(
        self: Arc<Self>,
        mut policy: watch::Receiver<Option<Arc<Policy>>>,
        stop: CancellationToken,
    ) {
        let mut interval = tokio::time::interval(JANITOR_INTERVAL);
        loop {
            tokio::select! {
                () = stop.cancelled() => return,
                _ = interval.tick() => {}
                changed = policy.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
            let current = policy.borrow().clone();
            self.sweep(current.as_deref()).await;
        }
    }
}

/// `initialize`: no session id needed; the answer carries the new one.
fn initialize(run_id: &str, id: &Value, message: &Value) -> Response {
    let requested = message
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    let result = json!({
        "protocolVersion": negotiated_version(requested),
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "dark-factory-gateway", "version": env!("CARGO_PKG_VERSION") },
    });
    let session = format!("df-{run_id}-{}", uuid::Uuid::now_v7().simple());
    json_response(
        StatusCode::OK,
        &[("mcp-session-id", session)],
        &rpc_result(id, result),
    )
}

enum Session {
    Missing,
    /// Malformed, or issued to another run.
    Foreign,
    Valid,
}

/// Checks the `Mcp-Session-Id` header: `df-<run id>-<random>` issued to `run_id`.
fn session_state(headers: &HeaderMap, run_id: &str) -> Session {
    let Some(value) = headers.get("mcp-session-id") else {
        return Session::Missing;
    };
    let issued_to_run = value
        .to_str()
        .ok()
        .and_then(|id| id.strip_prefix("df-"))
        .and_then(|rest| rest.strip_prefix(run_id))
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|random| random.len() == 32 && random.bytes().all(|b| b.is_ascii_hexdigit()));
    if issued_to_run {
        Session::Valid
    } else {
        Session::Foreign
    }
}

/// The request body, at most [`MAX_REQUEST_BYTES`].
async fn read_body<B>(headers: &HeaderMap, body: B) -> Result<Bytes, Box<Response>>
where
    B: hyper::body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let declared = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let too_large = || {
        Box::new(text(
            StatusCode::PAYLOAD_TOO_LARGE,
            "df: request body too large",
        ))
    };
    if declared.is_some_and(|length| length > MAX_REQUEST_BYTES) {
        return Err(too_large());
    }
    let limited = LimitedBody::new(
        body.map_err(Into::into).boxed_unsync(),
        MAX_REQUEST_BYTES,
        REQUEST_IDLE,
    );
    let limit = limited.limit();
    match limited.collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(_error) => Err(Box::new(match limit.get() {
            Some(BodyLimit::TooLarge(_)) => *too_large(),
            Some(BodyLimit::TimedOut) => {
                text(StatusCode::REQUEST_TIMEOUT, "df: request body timed out")
            }
            None => text(StatusCode::BAD_REQUEST, "df: request body unreadable"),
        })),
    }
}
