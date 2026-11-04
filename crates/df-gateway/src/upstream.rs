//! Connections to the upstream servers the gateway talks to for sandboxes (forges for Git,
//! model providers for inference) and the body types of requests and responses.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Frame;
use hyper::client::conn::http1::SendRequest;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use rustls::RootCertStore;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_rustls::TlsConnector;

/// Time to open the TCP connection and complete the TLS handshake with the upstream.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Error type of request and response bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
/// Streaming body of requests and responses.
pub type Body = UnsyncBoxBody<Bytes, BoxError>;

/// A plain-text answer. Git shows the body of an error to the user as `remote:` lines; the
/// inference endpoint's clients show it as the error message.
pub(crate) fn text(status: StatusCode, message: &str) -> Response<Body> {
    let body = Full::new(Bytes::from(format!("{message}\n")))
        .map_err(|never| match never {})
        .boxed_unsync();
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// TLS client settings for connections to upstreams (ring, HTTP/1.1), and whether `http://`
/// URLs are used without TLS (`network.allowPlainHttp`).
#[derive(Clone)]
pub struct UpstreamConnector {
    connector: TlsConnector,
    allow_plain_http: bool,
}

/// A connection to an upstream: TLS, or plain TCP for an `http://` URL.
pub(crate) trait UpstreamIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> UpstreamIo for T {}

/// Why [`UpstreamConnector::open`] failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectError {
    #[error("connect timed out")]
    TimedOut,
    #[error("{0}")]
    Failed(BoxError),
}

impl UpstreamConnector {
    /// Trusts the public web PKI.
    pub fn new() -> Result<Self, rustls::Error> {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Self::with_roots(roots)
    }

    /// Trusts `roots` only.
    pub fn with_roots(roots: RootCertStore) -> Result<Self, rustls::Error> {
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            connector: TlsConnector::from(Arc::new(config)),
            allow_plain_http: false,
        })
    }

    /// Uses `http://` URLs without TLS if `allow` (`network.allowPlainHttp`); credentials then
    /// travel unencrypted.
    #[must_use]
    pub fn allowing_plain_http(mut self, allow: bool) -> Self {
        self.allow_plain_http = allow;
        self
    }

    /// `url` if these settings may connect to it.
    pub(crate) fn url(&self, url: &str) -> Option<UpstreamUrl> {
        UpstreamUrl::parse(url, self.allow_plain_http)
    }

    /// Connects to `url` (within [`CONNECT_TIMEOUT`]) and starts the HTTP/1.1 client connection.
    /// The returned task drives the connection and ends it when dropped.
    pub(crate) async fn open<B>(
        &self,
        url: &UpstreamUrl,
    ) -> Result<(SendRequest<B>, ConnectionTask), ConnectError>
    where
        B: hyper::body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<BoxError>,
    {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, self.connect(url))
            .await
            .map_err(|_elapsed| ConnectError::TimedOut)?
            .map_err(ConnectError::Failed)?;
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ConnectError::Failed(Box::new(error)))?;
        let task = ConnectionTask(tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "upstream connection ended with an error");
            }
        }));
        Ok((sender, task))
    }

    /// Opens a connection to the host of `url`, TLS if it is `https`, without a timeout.
    async fn connect(&self, url: &UpstreamUrl) -> Result<Box<dyn UpstreamIo>, BoxError> {
        let host = url.host();
        let stream = TcpStream::connect((host, url.port())).await?;
        if !url.is_https() {
            return Ok(Box::new(stream));
        }
        let name = ServerName::try_from(host.to_owned())?;
        Ok(Box::new(self.connector.connect(name, stream).await?))
    }
}

/// An upstream URL the gateway may use: https (or http with `network.allowPlainHttp`), without
/// userinfo, query or fragment.
#[derive(Debug, Clone)]
pub(crate) struct UpstreamUrl(Uri);

impl UpstreamUrl {
    fn parse(url: &str, allow_plain_http: bool) -> Option<Self> {
        let uri: Uri = url.parse().ok()?;
        let authority = uri.authority()?;
        let scheme_allowed = match uri.scheme_str() {
            Some("https") => true,
            Some("http") => allow_plain_http,
            _ => false,
        };
        // Credentials in the URL would bypass the injected one.
        let usable = scheme_allowed
            && uri.query().is_none()
            && !url.contains('#')
            && !authority.as_str().contains('@');
        usable.then_some(Self(uri))
    }

    pub(crate) fn uri(&self) -> &Uri {
        &self.0
    }

    pub(crate) fn is_https(&self) -> bool {
        self.0.scheme_str() == Some("https")
    }

    /// The host without IPv6 brackets.
    pub(crate) fn host(&self) -> &str {
        self.0.host().unwrap_or_default().trim_matches(['[', ']'])
    }

    /// `host[:port]` as written in the URL, for the `Host` header.
    pub(crate) fn authority(&self) -> &str {
        self.0.authority().map_or("", |a| a.as_str())
    }

    /// The port of the URL, else the default of its scheme.
    pub(crate) fn port(&self) -> u16 {
        self.0
            .port_u16()
            .unwrap_or(if self.is_https() { 443 } else { 80 })
    }
}

/// The task driving an upstream connection; aborted when the response body is dropped.
pub(crate) struct ConnectionTask(JoinHandle<()>);

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A response body that keeps its connection task alive while it streams.
pub(crate) struct Guarded {
    inner: Body,
    _task: ConnectionTask,
}

impl Guarded {
    pub(crate) fn new(inner: Body, task: ConnectionTask) -> Self {
        Self { inner, _task: task }
    }
}

impl hyper::body::Body for Guarded {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

/// Why a [`LimitedBody`] ended with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BodyLimit {
    #[error("body exceeds {0} bytes")]
    TooLarge(u64),
    #[error("body stalled")]
    TimedOut,
}

/// A body that fails once it exceeds `max_bytes` or stalls longer than `idle` between frames;
/// the failure is also kept in [`LimitedBody::limit`] for the answer to the client. The idle
/// deadline starts at the first poll, so connecting to the upstream doesn't count.
pub(crate) struct LimitedBody {
    inner: Body,
    max_bytes: u64,
    read: u64,
    idle: Duration,
    deadline: Option<Pin<Box<Sleep>>>,
    limit: Arc<OnceLock<BodyLimit>>,
}

impl LimitedBody {
    pub(crate) fn new(inner: Body, max_bytes: u64, idle: Duration) -> Self {
        Self {
            inner,
            max_bytes,
            read: 0,
            idle,
            deadline: None,
            limit: Arc::new(OnceLock::new()),
        }
    }

    /// The limit the body hit, set once it failed.
    pub(crate) fn limit(&self) -> Arc<OnceLock<BodyLimit>> {
        self.limit.clone()
    }

    fn fail(&self, limit: BodyLimit) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        self.limit.set(limit).ok();
        Poll::Ready(Some(Err(Box::new(limit))))
    }
}

impl hyper::body::Body for LimitedBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        let idle = this.idle;
        let polled = Pin::new(&mut this.inner).poll_frame(cx);
        let deadline = this
            .deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
        match polled {
            Poll::Ready(Some(Ok(frame))) => {
                deadline.as_mut().reset(Instant::now() + idle);
                let len = frame.data_ref().map_or(0, Bytes::len);
                this.read = this
                    .read
                    .saturating_add(u64::try_from(len).unwrap_or(u64::MAX));
                if this.read > this.max_bytes {
                    return this.fail(BodyLimit::TooLarge(this.max_bytes));
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending if deadline.as_mut().poll(cx).is_ready() => {
                this.fail(BodyLimit::TimedOut)
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
}

#[cfg(test)]
mod tests;
