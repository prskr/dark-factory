//! A blocking `gix_transport` HTTP backend on hyper and the forge TLS settings. The request is
//! sent on the first read of headers or body, so a POST upload written after `post()` returns is
//! buffered first. Runs on a blocking thread and drives I/O with [`Handle::block_on`].

use std::any::Any;
use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::{Buf, Bytes};
use gix_transport::client::blocking_io::http::{GetResponse, Http, PostBodyDataKind, PostResponse};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, HOST, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::runtime::Handle;
use tokio::time::Instant;

use super::super::upstream::{
    CONNECT_TIMEOUT, ConnectionTask, ForgeConnector, ForgeUrl, RESPONSE_HEAD_TIMEOUT,
};

/// Longest wait for the next frame of a response body.
const FRAME_TIMEOUT: Duration = Duration::from_secs(60);

/// Why the forge exchange failed, as far as the transport can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForgeFailure {
    /// Connect, TLS, a read or a timeout failed.
    Unreachable,
    /// The forge answered 401 or 403.
    Rejected,
    /// The forge answered another non-2xx status.
    Status(u16),
}

/// The first [`ForgeFailure`] of a fetch, shared between the blocking fetch and its caller.
#[derive(Debug, Clone, Default)]
pub(crate) struct FailureCell(Arc<OnceLock<ForgeFailure>>);

impl FailureCell {
    pub(crate) fn get(&self) -> Option<ForgeFailure> {
        self.0.get().copied()
    }

    fn set(&self, failure: ForgeFailure) {
        // The first failure wins.
        self.0.set(failure).ok();
    }
}

pub(super) struct HyperHttp {
    rt: Handle,
    connector: ForgeConnector,
    /// The forge repository; every request goes to its host.
    url: ForgeUrl,
    authorization: HeaderValue,
    failure: FailureCell,
    deadline: Instant,
}

impl HyperHttp {
    /// Exchanges that fail once `deadline` passes.
    pub(super) fn new(
        rt: Handle,
        connector: ForgeConnector,
        url: ForgeUrl,
        authorization: HeaderValue,
        deadline: Instant,
    ) -> Self {
        Self {
            rt,
            connector,
            url,
            authorization,
            failure: FailureCell::default(),
            deadline,
        }
    }

    /// Where the exchanges of this backend report how the forge failed.
    pub(super) fn failure(&self) -> FailureCell {
        self.failure.clone()
    }

    fn exchange(
        &self,
        method: Method,
        url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> gix_error::Result<(HeaderReader, BodyReader, UploadWriter)> {
        let mut builder = Request::builder().method(method).uri(url);
        for line in headers {
            let Some((name, value)) = line.as_ref().split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("authorization") {
                continue;
            }
            builder = builder.header(name.trim(), value.trim());
        }
        builder = builder.header(AUTHORIZATION, self.authorization.clone());
        let (parts, ()) = builder
            .body(())
            .map_err(gix_error::Error::from_error)?
            .into_parts();
        let shared = Rc::new(RefCell::new(Exchange {
            rt: self.rt.clone(),
            connector: self.connector.clone(),
            url: self.url.clone(),
            failure: self.failure.clone(),
            deadline: self.deadline,
            state: State::Pending {
                parts: Some(parts),
                upload: Vec::new(),
            },
        }));
        Ok((
            HeaderReader {
                shared: shared.clone(),
                buf: None,
            },
            BodyReader {
                shared: shared.clone(),
                chunk: Bytes::new(),
            },
            UploadWriter { shared },
        ))
    }
}

impl Http for HyperHttp {
    type Headers = HeaderReader;
    type ResponseBody = BodyReader;
    type PostBody = UploadWriter;

    fn get(
        &mut self,
        url: &str,
        _base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> gix_error::Result<GetResponse<HeaderReader, BodyReader>> {
        let (headers, body, _) = self.exchange(Method::GET, url, headers)?;
        Ok(GetResponse { headers, body })
    }

    fn post(
        &mut self,
        url: &str,
        _base_url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        _body: PostBodyDataKind,
    ) -> gix_error::Result<PostResponse<HeaderReader, BodyReader, UploadWriter>> {
        let (headers, body, post_body) = self.exchange(Method::POST, url, headers)?;
        Ok(PostResponse {
            post_body,
            headers,
            body,
        })
    }

    fn configure(&mut self, _config: &dyn Any) -> gix_error::Result {
        Ok(())
    }
}

pub(super) struct HeaderReader {
    shared: Rc<RefCell<Exchange>>,
    buf: Option<io::Cursor<Vec<u8>>>,
}

impl Read for HeaderReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.read(out)?;
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for HeaderReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buf.is_none() {
            let headers = self.shared.borrow_mut().ensure_sent()?;
            self.buf = Some(io::Cursor::new(headers));
        }
        match &mut self.buf {
            Some(buf) => buf.fill_buf(),
            None => Ok(&[]),
        }
    }

    fn consume(&mut self, n: usize) {
        if let Some(buf) = &mut self.buf {
            buf.consume(n);
        }
    }
}

pub(super) struct BodyReader {
    shared: Rc<RefCell<Exchange>>,
    chunk: Bytes,
}

impl Read for BodyReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.read(out)?;
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for BodyReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.chunk.is_empty() {
            self.chunk = self.shared.borrow_mut().next_chunk()?;
        }
        Ok(&self.chunk)
    }

    fn consume(&mut self, n: usize) {
        self.chunk.advance(n.min(self.chunk.len()));
    }
}

pub(super) struct UploadWriter {
    shared: Rc<RefCell<Exchange>>,
}

impl Write for UploadWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match &mut self.shared.borrow_mut().state {
            State::Pending { upload, .. } => upload.extend_from_slice(data),
            _ => return Err(io::Error::other("request already sent")),
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

enum State {
    Pending {
        parts: Option<hyper::http::request::Parts>,
        upload: Vec<u8>,
    },
    Streaming {
        headers: Vec<u8>,
        body: Incoming,
        _connection: ConnectionTask,
    },
    Failed(io::ErrorKind, String),
}

struct Exchange {
    rt: Handle,
    connector: ForgeConnector,
    url: ForgeUrl,
    failure: FailureCell,
    deadline: Instant,
    state: State,
}

impl Exchange {
    /// Sends the request once and returns the response headers as `name: value\n` lines.
    fn ensure_sent(&mut self) -> io::Result<Vec<u8>> {
        if let State::Pending { parts, upload } = &mut self.state {
            let Some(parts) = parts.take() else {
                return Err(io::Error::other("request without parts"));
            };
            let request =
                Request::from_parts(parts, Full::new(Bytes::from(std::mem::take(upload))));
            self.state = match self.rt.block_on(async {
                tokio::time::timeout_at(self.deadline, send(&self.connector, &self.url, request))
                    .await
                    .unwrap_or_else(|_elapsed| {
                        Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "fetch deadline passed",
                        ))
                    })
            }) {
                Ok((response, connection)) => received(response, connection, &self.failure),
                Err(error) => {
                    self.failure.set(ForgeFailure::Unreachable);
                    State::Failed(error.kind(), error.to_string())
                }
            };
        }
        match &mut self.state {
            State::Streaming { headers, .. } => Ok(std::mem::take(headers)),
            State::Failed(kind, message) => Err(io::Error::new(*kind, message.clone())),
            State::Pending { .. } => Err(io::Error::other("request not sent")),
        }
    }

    fn next_chunk(&mut self) -> io::Result<Bytes> {
        self.ensure_sent()?;
        let State::Streaming { body, .. } = &mut self.state else {
            return Err(io::Error::other("no response body"));
        };
        let limit = self.deadline.min(Instant::now() + FRAME_TIMEOUT);
        loop {
            let frame = self
                .rt
                .block_on(async { tokio::time::timeout_at(limit, body.frame()).await })
                .map_err(|_elapsed| {
                    self.failure.set(ForgeFailure::Unreachable);
                    io::Error::new(io::ErrorKind::TimedOut, "forge read timed out")
                })?;
            match frame {
                None => return Ok(Bytes::new()),
                Some(Err(error)) => {
                    self.failure.set(ForgeFailure::Unreachable);
                    return Err(io::Error::other(error));
                }
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data()
                        && !data.is_empty()
                    {
                        return Ok(data);
                    }
                }
            }
        }
    }
}

fn received(
    response: Response<Incoming>,
    connection: ConnectionTask,
    failure: &FailureCell,
) -> State {
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        failure.set(ForgeFailure::Rejected);
        return State::Failed(io::ErrorKind::PermissionDenied, format!("HTTP {status}"));
    }
    if !status.is_success() {
        failure.set(ForgeFailure::Status(status.as_u16()));
        return State::Failed(io::ErrorKind::Other, format!("HTTP {status}"));
    }
    let mut headers = Vec::new();
    for (name, value) in response.headers() {
        headers.extend_from_slice(name.as_str().as_bytes());
        headers.extend_from_slice(b": ");
        headers.extend_from_slice(value.as_bytes());
        headers.push(b'\n');
    }
    State::Streaming {
        headers,
        body: response.into_body(),
        _connection: connection,
    }
}

async fn send(
    connector: &ForgeConnector,
    url: &ForgeUrl,
    request: Request<Full<Bytes>>,
) -> io::Result<(Response<Incoming>, ConnectionTask)> {
    let (mut parts, body) = request.into_parts();
    parts.headers.insert(
        HOST,
        HeaderValue::from_str(url.authority()).map_err(io::Error::other)?,
    );
    // The connection API sends the URI verbatim: origin-form. gix only requests below `url`.
    parts.uri = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .parse()
        .map_err(io::Error::other)?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(url))
        .await
        .map_err(|_elapsed| io::Error::new(io::ErrorKind::TimedOut, "forge connect timed out"))?
        .map_err(io::Error::other)?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(io::Error::other)?;
    let task = ConnectionTask(tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::debug!(%error, "forge connection ended with an error");
        }
    }));
    let response = tokio::time::timeout(
        RESPONSE_HEAD_TIMEOUT,
        sender.send_request(Request::from_parts(parts, body)),
    )
    .await
    .map_err(|_elapsed| io::Error::new(io::ErrorKind::TimedOut, "forge response timed out"))?
    .map_err(io::Error::other)?;
    Ok((response, task))
}
