//! The MCP client the gateway is towards an upstream server (iter4-mcp-gateway design D3, D4):
//! one `POST` per message over a fresh connection, JSON or SSE answers, one session per run and
//! server, credential injection and redaction.
//!
//! Everything it returns has the server's credential replaced with `[redacted]`. Errors carry no
//! URL, query or credential.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE, HOST, HeaderMap, HeaderName, HeaderValue,
};
use hyper::{Method, Request};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::sse::SseParser;
use super::target::{Target, TargetError};
use crate::resolve::{BlockedRanges, Resolver};
use crate::upstream::{
    BodyLimit, BoxError, ConnectError, Destination, LimitedBody, UpstreamConnector,
};

/// Protocol version the gateway speaks to upstreams.
pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";
/// Largest answer of one upstream request, and of all pages of one tool list.
pub(crate) const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
/// Most tools of one server.
pub(crate) const MAX_TOOLS: usize = 1_000;
/// Time one upstream `tools/list` may take, all pages together.
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest silence in an upstream answer before it counts as stalled.
const BODY_IDLE: Duration = Duration::from_secs(120);
/// Time to resolve the upstream host.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Time `DELETE` of a session may take.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Guard against an upstream that keeps sending cursors.
const MAX_PAGES: usize = 100;

/// Why an upstream request failed. The messages are the fixed texts the agent sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientError {
    /// The server cannot be used (missing credential or scheme, unusable URL).
    Target(TargetError),
    /// The host resolves to a refused address.
    Blocked,
    /// The host did not resolve.
    Resolve,
    /// Connecting or the TLS handshake failed.
    Connect,
    TimedOut,
    /// The upstream answered with this HTTP status.
    Status(u16),
    Redirect,
    /// The answer is not an MCP response.
    NotMcp,
    TooLarge,
    Cancelled,
}

impl ClientError {
    /// The text for the agent; never contains the credential, the URL or response content.
    pub(crate) fn message(self) -> String {
        match self {
            Self::Target(_) => "upstream credential unavailable".into(),
            Self::Blocked => "upstream address not allowed".into(),
            Self::Resolve => "upstream host not found".into(),
            Self::Connect => "upstream unreachable".into(),
            Self::TimedOut => "upstream timeout".into(),
            Self::Status(status @ (401 | 403)) => {
                format!("upstream refused the credential ({status})")
            }
            Self::Status(status) => format!("upstream error ({status})"),
            Self::Redirect => "upstream redirected".into(),
            Self::NotMcp => "upstream answered with something that is not MCP".into(),
            Self::TooLarge => "result too large".into(),
            Self::Cancelled => "call cancelled".into(),
        }
    }
}

/// The identity of an upstream session on this pod.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SessionKey {
    pub(crate) run_id: String,
    pub(crate) server_id: String,
}

/// The answer of an upstream to a request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    /// The `result` object.
    Result(Value),
    /// The `error` object of a JSON-RPC error response.
    Error(Value),
}

/// The tools of one server.
#[derive(Debug)]
pub(crate) struct ToolList {
    pub(crate) tools: Vec<Value>,
}

/// An upstream session. `opened_with` is the server configuration it was opened with: needed to
/// end it after the configuration changed (the old URL), never to send requests, which carry the
/// credential of the current snapshot.
#[derive(Clone)]
struct Session {
    opened_with: Target,
    id: Option<String>,
    protocol: String,
    used: Instant,
}

#[derive(Default)]
enum SlotState {
    #[default]
    Empty,
    /// A task is initializing the session; others wait on [`Slot::changed`].
    Opening,
    Open(Box<Session>),
}

/// The session of one run and server. The lock is held only to read or change the state, never
/// across a request.
#[derive(Default)]
struct Slot {
    state: std::sync::Mutex<SlotState>,
    changed: Notify,
}

/// What a task asking for a session finds.
enum Claim {
    Ready(Session),
    Wait,
    /// The task opens the session; `stale` is the session of an older configuration to end.
    Open {
        stale: Option<Session>,
    },
}

impl Slot {
    fn set(&self, state: SlotState) {
        if let Ok(mut current) = self.state.lock() {
            *current = state;
        }
        self.changed.notify_waiters();
    }

    fn claim(&self, config_version: u64) -> Claim {
        let Ok(mut state) = self.state.lock() else {
            return Claim::Open { stale: None };
        };
        match &mut *state {
            SlotState::Open(session) if session.opened_with.config_version == config_version => {
                session.used = Instant::now();
                Claim::Ready(Session::clone(session))
            }
            SlotState::Open(_) => {
                let SlotState::Open(stale) = std::mem::replace(&mut *state, SlotState::Opening)
                else {
                    return Claim::Wait;
                };
                Claim::Open {
                    stale: Some(*stale),
                }
            }
            SlotState::Opening => Claim::Wait,
            SlotState::Empty => {
                *state = SlotState::Opening;
                Claim::Open { stale: None }
            }
        }
    }

    /// The open session of this configuration version.
    fn open(&self, config_version: u64) -> Option<Session> {
        match &*self.state.lock().ok()? {
            SlotState::Open(session) if session.opened_with.config_version == config_version => {
                Some(Session::clone(session))
            }
            _ => None,
        }
    }

    /// Drops the open session `id` (it expired upstream); a newer one stays.
    fn forget(&self, id: Option<&str>) {
        let expired = self.state.lock().is_ok_and(
            |state| matches!(&*state, SlotState::Open(session) if session.id.as_deref() == id),
        );
        if expired {
            self.set(SlotState::Empty);
        }
    }

    /// Takes the open session out (the slot is empty afterwards) if `ended` says so.
    fn take_if(&self, ended: impl Fn(&Session) -> bool) -> Option<Session> {
        let mut state = self.state.lock().ok()?;
        let SlotState::Open(session) = &*state else {
            return None;
        };
        if !ended(session) {
            return None;
        }
        let SlotState::Open(session) = std::mem::take(&mut *state) else {
            return None;
        };
        drop(state);
        self.changed.notify_waiters();
        Some(*session)
    }

    fn is_empty(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| matches!(&*state, SlotState::Empty))
    }
}

/// Puts the slot back to empty if the task that was opening its session is dropped first.
struct OpeningGuard<'a> {
    slot: &'a Slot,
    opened: bool,
}

impl Drop for OpeningGuard<'_> {
    fn drop(&mut self) {
        if !self.opened {
            self.slot.set(SlotState::Empty);
        }
    }
}

type SlotRef = Arc<Slot>;

/// The client of every upstream of a gateway pod.
pub(crate) struct Client {
    connector: UpstreamConnector,
    resolver: Arc<dyn Resolver>,
    blocked: BlockedRanges,
    sessions: std::sync::Mutex<HashMap<SessionKey, SlotRef>>,
    next_id: AtomicU64,
    list_timeout: Duration,
}

struct Answer {
    reply: Option<Value>,
    session_id: Option<String>,
    bytes: u64,
}

impl Client {
    pub(crate) fn new(
        connector: UpstreamConnector,
        resolver: Arc<dyn Resolver>,
        blocked: BlockedRanges,
    ) -> Self {
        Self {
            connector,
            resolver,
            blocked,
            sessions: std::sync::Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            list_timeout: LIST_TIMEOUT,
        }
    }

    /// Shortens the time an upstream `tools/list` may take, so tests need no paused clock (a
    /// paused clock jumps ahead while a real socket connects).
    #[cfg(test)]
    pub(crate) fn with_list_timeout(mut self, timeout: Duration) -> Self {
        self.list_timeout = timeout;
        self
    }

    pub(crate) fn allows_plain_http(&self) -> bool {
        self.connector.allows_plain_http()
    }

    /// All tools of the server, page by page: at most [`MAX_TOOLS`] tools and
    /// [`MAX_RESPONSE_BYTES`] in total, within [`LIST_TIMEOUT`].
    pub(crate) async fn list_tools(
        &self,
        key: &SessionKey,
        target: &Target,
    ) -> Result<ToolList, ClientError> {
        tokio::time::timeout(self.list_timeout, self.list_pages(key, target))
            .await
            .map_err(|_elapsed| ClientError::TimedOut)?
    }

    async fn list_pages(&self, key: &SessionKey, target: &Target) -> Result<ToolList, ClientError> {
        let mut tools: Vec<Value> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut bytes = 0_u64;
        for _page in 0..MAX_PAGES {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |cursor| json!({ "cursor": cursor }));
            let id = self.next_request_id();
            let (reply, size) = self.rpc(key, target, "tools/list", params, id).await?;
            bytes += size;
            let Reply::Result(result) = reply else {
                return Err(ClientError::NotMcp);
            };
            if bytes > MAX_RESPONSE_BYTES {
                return Err(ClientError::TooLarge);
            }
            let page = result
                .get("tools")
                .and_then(Value::as_array)
                .ok_or(ClientError::NotMcp)?;
            tools.extend(page.iter().cloned());
            if tools.len() > MAX_TOOLS {
                return Err(ClientError::TooLarge);
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(ToolList { tools });
            }
        }
        Err(ClientError::NotMcp)
    }

    /// Calls `tool` with `arguments` and returns the upstream's answer. `cancel` ends the call:
    /// the upstream request is dropped and `notifications/cancelled` is sent best-effort.
    pub(crate) async fn call_tool(
        &self,
        key: &SessionKey,
        target: &Target,
        tool: &str,
        arguments: Value,
        cancel: &CancellationToken,
    ) -> Result<Reply, ClientError> {
        let id = self.next_request_id();
        let params = json!({ "name": tool, "arguments": arguments });
        tokio::select! {
            outcome = self.rpc(key, target, "tools/call", params, id) => outcome.map(|(reply, _size)| reply),
            () = cancel.cancelled() => {
                self.send_cancelled(key, target, id).await;
                Err(ClientError::Cancelled)
            }
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// One request on the session of `key`, initializing or re-initializing it as needed: an
    /// expired session (404) is replaced once.
    async fn rpc(
        &self,
        key: &SessionKey,
        target: &Target,
        method: &str,
        params: Value,
        id: u64,
    ) -> Result<(Reply, u64), ClientError> {
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        for attempt in 0..2 {
            let session = self.session(key, target).await?;
            match self.post(&session, target, &request, Some(id)).await {
                Err(ClientError::Status(404)) if session.id.is_some() => {
                    self.slot(key).forget(session.id.as_deref());
                    if attempt == 1 {
                        return Err(ClientError::Status(404));
                    }
                }
                Err(error) => return Err(error),
                Ok(answer) => return parse_reply(answer, target),
            }
        }
        Err(ClientError::Status(404))
    }

    /// The initialized session of `key`. The task that finds none (or one of another
    /// configuration version) opens it while the others wait for it, so a run and server never
    /// have two sessions; no lock is held while a request is made.
    async fn session(&self, key: &SessionKey, target: &Target) -> Result<Session, ClientError> {
        let slot = self.slot(key);
        loop {
            // Registered before the state is looked at: a change in between wakes us.
            let changed = slot.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            match slot.claim(target.config_version) {
                Claim::Ready(session) => return Ok(session),
                Claim::Wait => changed.await,
                Claim::Open { stale } => return self.open_session(&slot, target, stale).await,
            }
        }
    }

    /// Initializes the session of `slot` and publishes it as soon as it is ready; the session of
    /// an older configuration is ended alongside, without holding anyone back.
    async fn open_session(
        &self,
        slot: &Slot,
        target: &Target,
        stale: Option<Session>,
    ) -> Result<Session, ClientError> {
        let mut guard = OpeningGuard {
            slot,
            opened: false,
        };
        let mut closing = stale.map(|old| {
            Box::pin(async move {
                let target = old.opened_with.clone();
                self.close(&old, &target).await;
            })
        });
        let mut init = Box::pin(self.initialize(target));
        let mut closed = closing.is_none();
        let opened = if let Some(closing) = closing.as_mut() {
            tokio::select! {
                opened = &mut init => opened,
                () = closing.as_mut() => {
                    closed = true;
                    init.await
                }
            }
        } else {
            init.await
        };
        if let Ok(session) = &opened {
            slot.set(SlotState::Open(Box::new(session.clone())));
            guard.opened = true;
        }
        if let (false, Some(closing)) = (closed, closing) {
            closing.await;
        }
        opened
    }

    /// `initialize`, its successful response, then `notifications/initialized`.
    async fn initialize(&self, target: &Target) -> Result<Session, ClientError> {
        let blank = Session {
            opened_with: target.clone(),
            id: None,
            protocol: String::new(),
            used: Instant::now(),
        };
        let id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0", "id": id, "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "dark-factory-gateway", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let answer = self.post(&blank, target, &request, Some(id)).await?;
        let session_id = answer.session_id.clone();
        let (reply, _size) = parse_reply(answer, target)?;
        let Reply::Result(result) = reply else {
            return Err(ClientError::NotMcp);
        };
        let protocol = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or(ClientError::NotMcp)?
            .to_owned();
        let session = Session {
            id: session_id,
            protocol,
            ..blank
        };
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        self.post(&session, target, &initialized, None).await?;
        Ok(session)
    }

    async fn send_cancelled(&self, key: &SessionKey, target: &Target, request_id: u64) {
        let Some(session) = self.slot(key).open(target.config_version) else {
            return;
        };
        let body = json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": request_id, "reason": "cancelled by the client" },
        });
        let sent =
            tokio::time::timeout(CLOSE_TIMEOUT, self.post(&session, target, &body, None)).await;
        if !matches!(sent, Ok(Ok(_))) {
            tracing::debug!(server = %target.name, "could not forward a cancellation to the upstream");
        }
    }

    fn slot(&self, key: &SessionKey) -> SlotRef {
        let Ok(mut sessions) = self.sessions.lock() else {
            return SlotRef::default();
        };
        sessions.entry(key.clone()).or_default().clone()
    }

    /// Ends the sessions `keep` refuses (`DELETE`), and those idle for longer than `max_idle`.
    /// `keep` gets the configuration version a session was opened with. The `DELETE` carries the
    /// credential `current` gives for the session's server; the one the session was opened with
    /// only if its configuration changed since (or the server is gone).
    pub(crate) async fn sweep(
        &self,
        keep: &(dyn Fn(&SessionKey, u64) -> bool + Sync),
        current: &(dyn Fn(&SessionKey) -> Option<Target> + Sync),
        max_idle: Duration,
    ) {
        let slots: Vec<(SessionKey, SlotRef)> = match self.sessions.lock() {
            Ok(sessions) => sessions
                .iter()
                .map(|(key, slot)| (key.clone(), slot.clone()))
                .collect(),
            Err(_poisoned) => return,
        };
        for (key, slot) in slots {
            if let Some(session) = slot.take_if(|session| {
                !keep(&key, session.opened_with.config_version)
                    || session.used.elapsed() >= max_idle
            }) {
                let version = session.opened_with.config_version;
                let target = current(&key)
                    .filter(|target| target.config_version == version)
                    .unwrap_or_else(|| session.opened_with.clone());
                self.close(&session, &target).await;
            }
            self.drop_empty(&key, &slot);
        }
    }

    /// Removes the entry of `key` if its slot is empty and nobody else holds it.
    fn drop_empty(&self, key: &SessionKey, slot: &SlotRef) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        // The map and the caller hold the slot; a task that asked for it since holds a third.
        if Arc::strong_count(slot) <= 2
            && slot.is_empty()
            && sessions
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            sessions.remove(key);
        }
    }

    /// `DELETE` of the session, best-effort: a stateless upstream has no session to end.
    async fn close(&self, session: &Session, target: &Target) {
        if session.id.is_none() {
            return;
        }
        let closed = tokio::time::timeout(
            CLOSE_TIMEOUT,
            self.exchange(session, target, Method::DELETE, None),
        )
        .await;
        if !matches!(closed, Ok(Ok(_))) {
            tracing::debug!(server = %target.name, "upstream session could not be ended");
        }
    }

    /// Posts `body`. With `await_id` the answer must carry the response to that request;
    /// without, the upstream's answer is only checked for its status.
    async fn post(
        &self,
        session: &Session,
        target: &Target,
        body: &Value,
        await_id: Option<u64>,
    ) -> Result<Answer, ClientError> {
        let bytes = Bytes::from(serde_json::to_vec(body).map_err(|_error| ClientError::NotMcp)?);
        let (parts, incoming, task) = self
            .exchange(session, target, Method::POST, Some(bytes))
            .await?;
        let session_id = parts
            .headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let limited = LimitedBody::new(
            incoming.map_err(BoxError::from).boxed_unsync(),
            MAX_RESPONSE_BYTES,
            BODY_IDLE,
        );
        let limit = limited.limit();
        let result = match await_id {
            None => drain(limited).await.map(|bytes| (None, bytes)),
            Some(id) => {
                let is_sse = parts
                    .headers
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.starts_with("text/event-stream"));
                if is_sse {
                    self.read_sse(session, target, limited, id)
                        .await
                        .map(|(reply, bytes)| (Some(reply), bytes))
                } else {
                    read_json(limited, id)
                        .await
                        .map(|(reply, bytes)| (Some(reply), bytes))
                }
            }
        };
        drop(task);
        let (reply, bytes) = result.map_err(|error| match limit.get() {
            Some(BodyLimit::TooLarge(_)) => ClientError::TooLarge,
            Some(BodyLimit::TimedOut) => ClientError::TimedOut,
            None => error,
        })?;
        Ok(Answer {
            reply,
            session_id,
            bytes,
        })
    }

    /// Posts `body` and discards the answer.
    async fn post_discard(
        &self,
        session: &Session,
        target: &Target,
        body: &Value,
    ) -> Result<(), ClientError> {
        // Boxed: `post` reads an SSE stream, which answers server requests through here.
        Box::pin(self.post(session, target, body, None))
            .await
            .map(drop)
    }

    /// Reads the SSE stream until the response to request `id`; server requests are answered
    /// with `-32601`, notifications dropped.
    async fn read_sse(
        &self,
        session: &Session,
        target: &Target,
        mut body: LimitedBody,
        id: u64,
    ) -> Result<(Value, u64), ClientError> {
        let mut parser = SseParser::default();
        let mut total = 0_u64;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_error| ClientError::Connect)?;
            let Some(data) = frame.data_ref() else {
                continue;
            };
            total += data.len() as u64;
            for event in parser.feed(data) {
                let Ok(message) = serde_json::from_str::<Value>(&event) else {
                    continue;
                };
                if message.get("method").is_some() {
                    self.refuse_server_request(session, target, message.get("id"))
                        .await;
                    continue;
                }
                if is_response_to(&message, id) {
                    return Ok((message, total));
                }
            }
        }
        Err(ClientError::NotMcp)
    }

    /// Answers a request of the upstream with `-32601`; a notification (no `id`) is dropped.
    async fn refuse_server_request(&self, session: &Session, target: &Target, id: Option<&Value>) {
        let Some(id) = id else {
            return;
        };
        let body = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } });
        if let Err(error) = self.post_discard(session, target, &body).await {
            tracing::debug!(server = %target.name, ?error, "refusing an upstream request failed");
        }
    }

    /// Sends one request and returns the response head with its body.
    async fn exchange(
        &self,
        session: &Session,
        target: &Target,
        method: Method,
        body: Option<Bytes>,
    ) -> Result<
        (
            hyper::http::response::Parts,
            hyper::body::Incoming,
            crate::upstream::ConnectionTask,
        ),
        ClientError,
    > {
        let url = target.request_url();
        let address = self.checked_address(target, url.host()).await?;
        let destination = Destination {
            host: url.host(),
            port: url.port(),
            https: url.is_https(),
            address,
        };
        let (mut sender, task) = self
            .connector
            .open_to::<Full<Bytes>>(&destination)
            .await
            .map_err(|error| match error {
                ConnectError::TimedOut => ClientError::TimedOut,
                ConnectError::Failed(_) => ClientError::Connect,
            })?;
        let mut headers = HeaderMap::new();
        if let Ok(host) = HeaderValue::from_str(&url.authority()) {
            headers.insert(HOST, host);
        }
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        if body.is_some() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        if !session.protocol.is_empty()
            && let Ok(version) = HeaderValue::from_str(&session.protocol)
        {
            headers.insert(HeaderName::from_static("mcp-protocol-version"), version);
        }
        if let Some(id) = &session.id
            && let Ok(id) = HeaderValue::from_str(id)
        {
            headers.insert(HeaderName::from_static("mcp-session-id"), id);
        }
        target
            .add_credential(&mut headers)
            .map_err(ClientError::Target)?;
        let mut request = Request::new(Full::new(body.unwrap_or_default()));
        *request.method_mut() = method;
        *request.uri_mut() = url
            .request_target()
            .parse()
            .map_err(|_invalid| ClientError::Target(TargetError::InvalidUrl))?;
        *request.headers_mut() = headers;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_error| ClientError::Connect)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(ClientError::Redirect);
        }
        if !status.is_success() {
            return Err(ClientError::Status(status.as_u16()));
        }
        let (parts, incoming) = response.into_parts();
        Ok((parts, incoming, task))
    }

    /// The address to connect to for a server that must not reach private addresses: every
    /// resolved address is checked, the first one is used. `None` connects by name.
    async fn checked_address(
        &self,
        target: &Target,
        host: &str,
    ) -> Result<Option<IpAddr>, ClientError> {
        if target.private_network {
            return Ok(None);
        }
        let addresses = match host.parse::<IpAddr>() {
            Ok(literal) => vec![literal],
            Err(_not_a_literal) => {
                tokio::time::timeout(RESOLVE_TIMEOUT, self.resolver.resolve(host))
                    .await
                    .map_err(|_elapsed| ClientError::Resolve)?
                    .map_err(|_error| ClientError::Resolve)?
            }
        };
        if addresses
            .iter()
            .any(|address| self.blocked.contains(*address))
        {
            tracing::warn!(server = %target.name, url = %target.url_for_logs(), "upstream resolves to a blocked address");
            return Err(ClientError::Blocked);
        }
        addresses
            .first()
            .copied()
            .map(Some)
            .ok_or(ClientError::Resolve)
    }
}

fn is_response_to(message: &Value, id: u64) -> bool {
    message.get("id").and_then(Value::as_u64) == Some(id)
        && (message.get("result").is_some() || message.get("error").is_some())
}

/// The reply of a JSON answer: the response object of request `id`, alone or in a batch.
async fn read_json(body: LimitedBody, id: u64) -> Result<(Value, u64), ClientError> {
    let bytes = body
        .collect()
        .await
        .map_err(|_error| ClientError::Connect)?
        .to_bytes();
    let size = bytes.len() as u64;
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|_error| ClientError::NotMcp)?;
    let found = match parsed {
        Value::Array(items) => items.into_iter().find(|item| is_response_to(item, id)),
        single if is_response_to(&single, id) => Some(single),
        _ => None,
    };
    found.map(|reply| (reply, size)).ok_or(ClientError::NotMcp)
}

/// Reads and discards a body.
async fn drain(body: LimitedBody) -> Result<u64, ClientError> {
    let bytes = body
        .collect()
        .await
        .map_err(|_error| ClientError::Connect)?
        .to_bytes();
    Ok(bytes.len() as u64)
}

/// Splits a response object into its result or error, with the credential redacted.
fn parse_reply(answer: Answer, target: &Target) -> Result<(Reply, u64), ClientError> {
    let mut message = answer.reply.ok_or(ClientError::NotMcp)?;
    target.redactor().value(&mut message);
    let reply = if let Some(error) = message.get_mut("error") {
        Reply::Error(error.take())
    } else if let Some(result) = message.get_mut("result") {
        Reply::Result(result.take())
    } else {
        return Err(ClientError::NotMcp);
    };
    Ok((reply, answer.bytes))
}

#[cfg(test)]
mod tests;
