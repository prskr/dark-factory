//! Forwarding of identified Git requests to the forge over HTTPS (design D3), or plain HTTP with
//! `forge.allowPlainHttp`.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use df_proto::gateway::v1::ForgeCredential;
use http_body_util::BodyExt;
use hyper::body::Frame;
use hyper::header::{AUTHORIZATION, HOST, HeaderValue};
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use rustls::RootCertStore;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_rustls::TlsConnector;

use super::server::{Body, BoxError, ForgeTarget, GitRequest, Operation, Upstream, text};

/// Time to open the TCP connection and complete the TLS handshake with the forge.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time the forge has to send the response head after the request body was sent, and to take
/// the next request body frame while it is sent.
pub(super) const RESPONSE_HEAD_TIMEOUT: Duration = Duration::from_secs(30);
/// Response headers passed back to the client; hop-by-hop headers are not among them.
const RELAYED_HEADERS: [&str; 5] = [
    "content-type",
    "cache-control",
    "expires",
    "pragma",
    "content-encoding",
];
pub(super) const UNREACHABLE: &str = "df: forge unreachable";
pub(super) const REJECTED: &str = "df: the forge rejected the gateway's credential";
/// The route's upstream URL is not usable (e.g. `http://` without `forge.allowPlainHttp`).
pub(super) const INVALID_URL: &str = "df: invalid forge URL";
/// Largest `git-upload-pack` request body forwarded to the forge.
pub(super) const MAX_FETCH_REQUEST_BYTES: u64 = 64 * 1024 * 1024;

/// TLS client settings for connections to the forge (ring, HTTP/1.1), and whether `http://`
/// forge URLs are used without TLS (`forge.allowPlainHttp`).
#[derive(Clone)]
pub struct ForgeConnector {
    connector: TlsConnector,
    allow_plain_http: bool,
}

/// A connection to the forge: TLS, or plain TCP for an `http://` URL.
pub(super) trait ForgeIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ForgeIo for T {}

impl ForgeConnector {
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

    /// Uses `http://` forge URLs without TLS if `allow` (`forge.allowPlainHttp`); the forge
    /// credential then travels unencrypted.
    #[must_use]
    pub fn allowing_plain_http(mut self, allow: bool) -> Self {
        self.allow_plain_http = allow;
        self
    }

    /// `url` if these settings may connect to it.
    pub(super) fn forge_url(&self, url: &str) -> Option<ForgeUrl> {
        ForgeUrl::parse(url, self.allow_plain_http)
    }

    /// Opens a connection to the host of `url`, TLS if it is `https`, without a timeout.
    pub(super) async fn connect(&self, url: &ForgeUrl) -> Result<Box<dyn ForgeIo>, BoxError> {
        let host = url.host();
        let stream = TcpStream::connect((host, url.port())).await?;
        if !url.is_https() {
            return Ok(Box::new(stream));
        }
        let name = ServerName::try_from(host.to_owned())?;
        Ok(Box::new(self.connector.connect(name, stream).await?))
    }
}

/// Forwards requests to the upstream URL of the route with the forge credential.
pub struct ForgeUpstream {
    connector: ForgeConnector,
}

impl ForgeUpstream {
    /// An upstream that trusts the public web PKI.
    pub fn new() -> Result<Self, rustls::Error> {
        Ok(Self::from_connector(ForgeConnector::new()?))
    }

    /// An upstream that trusts `roots` only.
    pub fn with_roots(roots: RootCertStore) -> Result<Self, rustls::Error> {
        Ok(Self::from_connector(ForgeConnector::with_roots(roots)?))
    }

    /// An upstream with the given TLS settings.
    pub fn from_connector(connector: ForgeConnector) -> Self {
        Self { connector }
    }
}

#[async_trait]
impl Upstream for ForgeUpstream {
    async fn forward(&self, forge: ForgeTarget<'_>, request: GitRequest) -> Response<Body> {
        let url = self.connector.forge_url(&forge.route.upstream_url);
        let Some(target) = Target::of(url, &request) else {
            tracing::warn!(
                task_key = %request.task_key,
                "git route has no usable upstream URL"
            );
            return text(StatusCode::BAD_GATEWAY, INVALID_URL);
        };
        let connected =
            tokio::time::timeout(CONNECT_TIMEOUT, self.connector.connect(&target.url)).await;
        let stream = match connected {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return unreachable(&target, &error.to_string()),
            Err(_elapsed) => return unreachable(&target, "connect timed out"),
        };
        let (mut sender, connection) =
            match hyper::client::conn::http1::handshake::<_, Body>(TokioIo::new(stream)).await {
                Ok(pair) => pair,
                Err(error) => return unreachable(&target, &error.to_string()),
            };
        let task = ConnectionTask(tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "forge connection ended with an error");
            }
        }));

        let (upload, progress) = Upload::new(request.body);
        let mut outgoing = Request::new(upload.boxed_unsync());
        *outgoing.method_mut() = request.method;
        *outgoing.uri_mut() = target.path_and_query.clone();
        *outgoing.headers_mut() = request.headers;
        let headers = outgoing.headers_mut();
        if let Ok(host) = HeaderValue::from_str(target.url.authority()) {
            headers.insert(HOST, host);
        }
        headers.insert(AUTHORIZATION, basic(forge.credential));

        let response = match response_head(sender.send_request(outgoing), progress).await {
            Some(Ok(response)) => response,
            Some(Err(error)) => return unreachable(&target, &error.to_string()),
            None => return unreachable(&target, "response timed out"),
        };
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            tracing::warn!(host = target.url.host(), %status, "forge rejected the gateway credential");
            return text(StatusCode::BAD_GATEWAY, REJECTED);
        }
        let (parts, body) = response.into_parts();
        let body = Guarded {
            inner: body.map_err(BoxError::from).boxed_unsync(),
            _task: task,
        };
        let mut relayed = Response::new(body.boxed_unsync());
        *relayed.status_mut() = status;
        for (name, value) in &parts.headers {
            if RELAYED_HEADERS.contains(&name.as_str()) {
                relayed.headers_mut().append(name.clone(), value.clone());
            }
        }
        relayed
    }
}

/// A forge repository URL the gateway may use: https (or http with `forge.allowPlainHttp`),
/// without userinfo, query or fragment.
#[derive(Debug, Clone)]
pub(super) struct ForgeUrl(Uri);

impl ForgeUrl {
    pub(super) fn parse(url: &str, allow_plain_http: bool) -> Option<Self> {
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

    pub(super) fn uri(&self) -> &Uri {
        &self.0
    }

    pub(super) fn is_https(&self) -> bool {
        self.0.scheme_str() == Some("https")
    }

    /// The host without IPv6 brackets.
    pub(super) fn host(&self) -> &str {
        self.0.host().unwrap_or_default().trim_matches(['[', ']'])
    }

    /// `host[:port]` as written in the URL, for the `Host` header.
    pub(super) fn authority(&self) -> &str {
        self.0.authority().map_or("", |a| a.as_str())
    }

    /// The port of the URL, else the default of its scheme.
    pub(super) fn port(&self) -> u16 {
        self.0
            .port_u16()
            .unwrap_or(if self.is_https() { 443 } else { 80 })
    }
}

/// Why a [`LimitedBody`] ended with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum BodyLimit {
    #[error("request body exceeds {0} bytes")]
    TooLarge(u64),
    #[error("request body stalled")]
    TimedOut,
}

/// A request body that fails once it exceeds `max_bytes` or stalls longer than `idle` between
/// frames; the failure is also kept in [`LimitedBody::limit`] for the answer to the client. The
/// idle deadline starts at the first poll, so connecting to the forge doesn't count.
pub(super) struct LimitedBody {
    inner: Body,
    max_bytes: u64,
    read: u64,
    idle: Duration,
    deadline: Option<Pin<Box<Sleep>>>,
    limit: Arc<OnceLock<BodyLimit>>,
}

impl LimitedBody {
    pub(super) fn new(inner: Body, max_bytes: u64, idle: Duration) -> Self {
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
    pub(super) fn limit(&self) -> Arc<OnceLock<BodyLimit>> {
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

pub(super) fn basic(credential: &ForgeCredential) -> HeaderValue {
    let token = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", credential.username, credential.password));
    // Base64 output is always a valid header value.
    let mut value = HeaderValue::from_str(&format!("Basic {token}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""));
    value.set_sensitive(true);
    value
}

/// The task driving the forge connection; aborted when the response body is dropped.
pub(super) struct ConnectionTask(pub(super) JoinHandle<()>);

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Where a request goes on the forge.
struct Target {
    url: ForgeUrl,
    path_and_query: Uri,
}

impl Target {
    /// `<url>/info/refs?service=<service>` or `<url>/<service>`; `None` without a usable URL.
    /// The client's query is not forwarded.
    fn of(url: Option<ForgeUrl>, request: &GitRequest) -> Option<Self> {
        let url = url?;
        let path = url.uri().path().trim_end_matches('/');
        let path_and_query = match request.operation {
            Operation::Advertise => {
                format!("{path}/info/refs?service={}", request.service.name())
            }
            Operation::Rpc => format!("{path}/{}", request.service.name()),
        };
        let path_and_query = path_and_query.parse().ok()?;
        Some(Self {
            url,
            path_and_query,
        })
    }
}

fn unreachable(target: &Target, error: &str) -> Response<Body> {
    tracing::warn!(
        host = target.url.host(),
        port = target.url.port(),
        error,
        "forge unreachable"
    );
    text(StatusCode::SERVICE_UNAVAILABLE, UNREACHABLE)
}

/// A response body that keeps its connection task alive while it streams.
struct Guarded {
    inner: Body,
    _task: ConnectionTask,
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

/// `response` within [`RESPONSE_HEAD_TIMEOUT`] of the last upload progress; `None` on timeout.
/// While the body is sent, the timeout restarts with every frame the forge takes.
async fn response_head<F: Future>(
    response: F,
    mut progress: watch::Receiver<bool>,
) -> Option<F::Output> {
    let mut response = pin!(response);
    let mut sending = true;
    loop {
        sending = sending && !*progress.borrow_and_update();
        tokio::select! {
            result = &mut response => return Some(result),
            // A dropped body (sender gone) is not sent any further.
            changed = progress.changed(), if sending => sending = changed.is_ok(),
            () = tokio::time::sleep(RESPONSE_HEAD_TIMEOUT) => return None,
        }
    }
}

/// A request body that reports its progress: `false` per frame taken, `true` at its end.
struct Upload {
    inner: Body,
    progress: watch::Sender<bool>,
}

impl Upload {
    fn new(inner: Body) -> (Self, watch::Receiver<bool>) {
        let (progress, receiver) = watch::channel(false);
        (Self { inner, progress }, receiver)
    }
}

impl hyper::body::Body for Upload {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        match &polled {
            Poll::Ready(Some(Ok(_))) => {
                self.progress.send_replace(false);
            }
            Poll::Ready(None) => {
                self.progress.send_replace(true);
            }
            Poll::Ready(Some(Err(_))) | Poll::Pending => {}
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        let ended = self.inner.is_end_stream();
        if ended {
            self.progress.send_replace(true);
        }
        ended
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests;
