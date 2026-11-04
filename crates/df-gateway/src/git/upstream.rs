//! Forwarding of identified Git requests to the forge over HTTPS (design D3), or plain HTTP with
//! `network.allowPlainHttp`.

use std::future::Future;
use std::pin::{Pin, pin};
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
use rustls::RootCertStore;
use tokio::sync::watch;

use super::server::{ForgeTarget, GitRequest, Operation, Upstream};
use crate::upstream::{Body, BoxError, Guarded, UpstreamConnector, UpstreamUrl, text};

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
/// The route's upstream URL is not usable (e.g. `http://` without `network.allowPlainHttp`).
pub(super) const INVALID_URL: &str = "df: invalid forge URL";
/// Largest `git-upload-pack` request body forwarded to the forge.
pub(super) const MAX_FETCH_REQUEST_BYTES: u64 = 64 * 1024 * 1024;

/// Forwards requests to the upstream URL of the route with the forge credential.
pub struct ForgeUpstream {
    connector: UpstreamConnector,
}

impl ForgeUpstream {
    /// An upstream that trusts the public web PKI.
    pub fn new() -> Result<Self, rustls::Error> {
        Ok(Self::from_connector(UpstreamConnector::new()?))
    }

    /// An upstream that trusts `roots` only.
    pub fn with_roots(roots: RootCertStore) -> Result<Self, rustls::Error> {
        Ok(Self::from_connector(UpstreamConnector::with_roots(roots)?))
    }

    /// An upstream with the given TLS settings.
    pub fn from_connector(connector: UpstreamConnector) -> Self {
        Self { connector }
    }
}

#[async_trait]
impl Upstream for ForgeUpstream {
    async fn forward(&self, forge: ForgeTarget<'_>, request: GitRequest) -> Response<Body> {
        let url = self.connector.url(&forge.route.upstream_url);
        let Some(target) = Target::of(url, &request) else {
            tracing::warn!(
                task_key = %request.task_key,
                "git route has no usable upstream URL"
            );
            return text(StatusCode::BAD_GATEWAY, INVALID_URL);
        };
        let (mut sender, task) = match self.connector.open::<Body>(&target.url).await {
            Ok(pair) => pair,
            Err(error) => return unreachable(&target, &error.to_string()),
        };

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
        let body = Guarded::new(body.map_err(BoxError::from).boxed_unsync(), task);
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

pub(super) fn basic(credential: &ForgeCredential) -> HeaderValue {
    let token = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", credential.username, credential.password));
    // Base64 output is always a valid header value.
    let mut value = HeaderValue::from_str(&format!("Basic {token}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""));
    value.set_sensitive(true);
    value
}

/// Where a request goes on the forge.
struct Target {
    url: UpstreamUrl,
    path_and_query: Uri,
}

impl Target {
    /// `<url>/info/refs?service=<service>` or `<url>/<service>`; `None` without a usable URL.
    /// The client's query is not forwarded.
    fn of(url: Option<UpstreamUrl>, request: &GitRequest) -> Option<Self> {
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
