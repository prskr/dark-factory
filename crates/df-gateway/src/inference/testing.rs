//! Test support: a fake model provider that answers the three wire APIs with canned bodies and
//! keeps what it received (iter3-inference-gateway design D10), and a plain-HTTP front for
//! [`Inference`](super::Inference).

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{StreamExt, stream};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::Inference;
use super::report::UsageSink;
use crate::policy::Policy;

pub(crate) const ANTHROPIC_STREAM: [&str; 3] = [
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5-5\",\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":15,\"output_tokens\":1}}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
    "event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":40}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
];

pub(crate) const CHAT_STREAM_WITHOUT_USAGE: [&str; 2] = [
    "data: {\"model\":\"gpt-x\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
    "data: [DONE]\n\n",
];

/// What the fake upstream received.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub method: Method,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

/// How the fake upstream answers the next requests.
#[derive(Clone)]
pub(crate) enum Reply {
    /// The canned answer of the request's path (streamed if the body asks for `"stream":true`).
    Canned,
    /// A fixed status with headers and a JSON body.
    Fixed(StatusCode, Vec<(&'static str, &'static str)>, &'static str),
    /// Server-sent events; after the first chunk the answer waits for `gate`.
    Events(Vec<&'static str>, Option<Arc<Notify>>),
    /// The request is read, but no response head is ever sent.
    NoHead,
    /// The connection is closed as soon as it is accepted.
    Hangup,
}

pub(crate) struct FakeUpstream {
    pub addr: SocketAddr,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    reply: Arc<Mutex<Reply>>,
    task: JoinHandle<()>,
}

impl FakeUpstream {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let reply = Arc::new(Mutex::new(Reply::Canned));
        let (seen_in, reply_in) = (seen.clone(), reply.clone());
        let task = tokio::spawn(async move {
            while let Ok((stream, _peer)) = listener.accept().await {
                tokio::spawn(serve_upstream(stream, seen_in.clone(), reply_in.clone()));
            }
        });
        Self {
            addr,
            seen,
            reply,
            task,
        }
    }

    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub(crate) fn reply_with(&self, reply: Reply) {
        *self.reply.lock().unwrap() = reply;
    }

    pub(crate) fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for FakeUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_upstream(
    stream: tokio::net::TcpStream,
    seen: Arc<Mutex<Vec<Seen>>>,
    reply: Arc<Mutex<Reply>>,
) {
    if matches!(*reply.lock().unwrap(), Reply::Hangup) {
        return;
    }
    let service = service_fn(move |request| {
        let (seen, reply) = (seen.clone(), reply.clone());
        async move { Ok::<_, Infallible>(answer(&seen, &reply, request).await) }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

type Answer = Response<http_body_util::combinators::UnsyncBoxBody<Bytes, Infallible>>;

async fn answer(
    seen: &Mutex<Vec<Seen>>,
    reply: &Mutex<Reply>,
    request: Request<Incoming>,
) -> Answer {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map(http_body_util::Collected::to_bytes)
        .unwrap_or_default();
    seen.lock().unwrap().push(Seen {
        method: parts.method.clone(),
        path_and_query: parts
            .uri
            .path_and_query()
            .map(ToString::to_string)
            .unwrap_or_default(),
        headers: parts.headers,
        body: body.clone(),
    });
    let streaming = std::str::from_utf8(&body).is_ok_and(|text| text.contains("\"stream\":true"));
    let reply = reply.lock().unwrap().clone();
    match reply {
        Reply::Fixed(status, headers, text) => {
            let mut response = full(status, "application/json", text);
            for (name, value) in headers {
                response.headers_mut().insert(
                    HeaderName::from_static(name),
                    HeaderValue::from_static(value),
                );
            }
            response
        }
        Reply::Events(chunks, gate) => events(chunks, gate),
        Reply::NoHead => std::future::pending().await,
        Reply::Hangup => panic!("a hangup reply closes the connection before a request is read"),
        Reply::Canned => canned(parts.uri.path(), streaming),
    }
}

fn canned(path: &str, streaming: bool) -> Answer {
    match (path, streaming) {
        ("/v1/messages", true) => events(ANTHROPIC_STREAM.to_vec(), None),
        ("/v1/messages", false) => full(
            StatusCode::OK,
            "application/json",
            r#"{"model":"claude-haiku-5-5","content":[{"type":"text","text":"hello"}],"usage":{"input_tokens":10,"output_tokens":3}}"#,
        ),
        ("/v1/messages/count_tokens", _) => {
            full(StatusCode::OK, "application/json", r#"{"input_tokens":12}"#)
        }
        ("/v1/chat/completions", true) => events(CHAT_STREAM_WITHOUT_USAGE.to_vec(), None),
        ("/v1/chat/completions", false) => full(
            StatusCode::OK,
            "application/json",
            r#"{"model":"gpt-x","choices":[],"usage":{"prompt_tokens":8,"completion_tokens":2}}"#,
        ),
        ("/v1/responses", _) => full(
            StatusCode::OK,
            "application/json",
            r#"{"model":"gpt-r","usage":{"input_tokens":5,"output_tokens":1}}"#,
        ),
        ("/v1/models", _) => full(StatusCode::OK, "application/json", r#"{"data":[]}"#),
        _ => full(StatusCode::NOT_FOUND, "application/json", "{}"),
    }
}

fn full(status: StatusCode, content_type: &'static str, body: &'static str) -> Answer {
    let mut response = Response::new(Full::new(Bytes::from(body)).boxed_unsync());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static(content_type));
    response
}

fn events(chunks: Vec<&'static str>, gate: Option<Arc<Notify>>) -> Answer {
    let frames = stream::unfold(
        (chunks.into_iter(), gate, 0_usize),
        |(mut chunks, gate, index)| async move {
            // The gate holds back everything after the first chunk.
            if index == 1
                && let Some(gate) = &gate
            {
                gate.notified().await;
            }
            let chunk = chunks.next()?;
            Some((
                Ok::<_, Infallible>(Frame::data(Bytes::from(chunk))),
                (chunks, gate, index + 1),
            ))
        },
    );
    let mut response = Response::new(StreamBody::new(frames).boxed_unsync());
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/event-stream"),
    );
    response
}

type StalledBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Infallible>;

/// [`Inference`] behind a plain-HTTP listener; every connection counts as coming from `source`.
pub(crate) struct Front {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Front {
    pub(crate) async fn start(
        inference: Arc<Inference>,
        policy: Option<Arc<Policy>>,
        source: IpAddr,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            while let Ok((stream, _peer)) = listener.accept().await {
                tokio::spawn(serve_front(
                    stream,
                    inference.clone(),
                    policy.clone(),
                    source,
                ));
            }
        });
        Self { addr, task }
    }

    /// Sends a request and returns the response head with the still open body.
    pub(crate) async fn open(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Response<Incoming> {
        let stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream))
                .await
                .unwrap();
        tokio::spawn(connection);
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "gateway");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        sender
            .send_request(
                request
                    .body(Full::new(Bytes::from(body.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Sends a request whose body sends `first` and then nothing more, and returns the response
    /// head.
    pub(crate) async fn open_with_stalled_body(
        &self,
        method: Method,
        path: &str,
        first: &'static str,
    ) -> Response<Incoming> {
        let stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, StalledBody>(TokioIo::new(stream))
                .await
                .unwrap();
        tokio::spawn(connection);
        let frames = stream::iter([Ok(Frame::data(Bytes::from(first)))]).chain(stream::pending());
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "gateway")
            .body(StreamBody::new(frames).boxed_unsync())
            .unwrap();
        sender.send_request(request).await.unwrap()
    }

    /// Sends a request and reads the whole answer.
    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (StatusCode, HeaderMap, String) {
        let response = self.open(method, path, headers, body).await;
        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            parts.headers,
            String::from_utf8_lossy(&body).into_owned(),
        )
    }
}

async fn serve_front(
    stream: tokio::net::TcpStream,
    inference: Arc<Inference>,
    policy: Option<Arc<Policy>>,
    source: IpAddr,
) {
    let service = service_fn(move |request: Request<Incoming>| {
        let (inference, policy) = (inference.clone(), policy.clone());
        async move { Ok::<_, Infallible>(inference.handle(policy.as_deref(), source, request).await) }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

impl Drop for Front {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Collects usage records.
#[derive(Default)]
pub(crate) struct Collector {
    records: Mutex<Vec<df_proto::gateway::v1::UsageRecord>>,
    added: Notify,
}

impl Collector {
    pub(crate) fn records(&self) -> Vec<df_proto::gateway::v1::UsageRecord> {
        self.records.lock().unwrap().clone()
    }

    /// Waits until at least `count` records were collected.
    pub(crate) async fn recorded(&self, count: usize) {
        loop {
            let added = self.added.notified();
            tokio::pin!(added);
            added.as_mut().enable();
            if self.records.lock().unwrap().len() >= count {
                return;
            }
            added.await;
        }
    }
}

impl UsageSink for Collector {
    fn record(&self, record: df_proto::gateway::v1::UsageRecord) {
        self.records.lock().unwrap().push(record);
        self.added.notify_waiters();
    }
}
