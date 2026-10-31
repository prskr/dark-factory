//! The Git endpoint: an HTTPS smart-HTTP server that identifies the sandbox by the source IP of
//! the connection (design D2) and hands the request for its own task repository to an
//! [`Upstream`].

use std::convert::Infallible;
use std::future::Future;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use df_core::TaskKey;
use df_proto::gateway::v1::{ForgeCredential, GitRoute};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use super::push::Push;
use super::spool::BODY_IDLE_TIMEOUT;
use super::upstream::{BodyLimit, LimitedBody, MAX_FETCH_REQUEST_BYTES};
use crate::policy::Policy;

/// Time a client has to complete the TLS handshake.
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Time a client has to send the request head.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Time in-flight requests get to finish when the endpoint shuts down.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(10);
/// Request headers passed on to the upstream (design D3); `Authorization` in particular is not.
/// `Content-Encoding` only for fetches: a push is checked on its raw bytes, which the forge must
/// see unchanged.
const FORWARDED_HEADERS: [&str; 5] = [
    "content-type",
    "content-encoding",
    "accept",
    "git-protocol",
    "user-agent",
];

/// Error type of request and response bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
/// Streaming body of requests and responses.
pub type Body = UnsyncBoxBody<Bytes, BoxError>;

/// The Git service a request belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    UploadPack,
    ReceivePack,
}

impl Service {
    pub const fn name(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "git-upload-pack" => Some(Self::UploadPack),
            "git-receive-pack" => Some(Self::ReceivePack),
            _ => None,
        }
    }
}

/// The step of the smart-HTTP exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// `GET <repo>/info/refs?service=<service>`.
    Advertise,
    /// `POST <repo>/<service>`.
    Rpc,
}

/// An identified request for the sandbox's own task repository.
pub struct GitRequest {
    pub task_key: TaskKey,
    pub service: Service,
    pub operation: Operation,
    pub method: Method,
    /// Only the [`FORWARDED_HEADERS`] of the client request.
    pub headers: HeaderMap,
    pub body: Body,
}

/// The forge repository of a request and the credential the gateway injects for it.
#[derive(Clone, Copy)]
pub struct ForgeTarget<'a> {
    pub route: &'a GitRoute,
    pub credential: &'a ForgeCredential,
}

impl<'a> ForgeTarget<'a> {
    pub const fn new(route: &'a GitRoute, credential: &'a ForgeCredential) -> Self {
        Self { route, credential }
    }
}

/// Where identified requests go: the forge repository of the task.
#[async_trait]
pub trait Upstream: Send + Sync {
    /// Forwards `request` to `target` and returns the answer for the client.
    async fn forward(&self, target: ForgeTarget<'_>, request: GitRequest) -> Response<Body>;
}

/// The Git port of a gateway pod.
pub struct GitEndpoint {
    policy: watch::Receiver<Option<Arc<Policy>>>,
    upstream: Arc<dyn Upstream>,
    push: Arc<Push>,
    fetch_max_bytes: u64,
}

impl GitEndpoint {
    pub fn new(
        policy: watch::Receiver<Option<Arc<Policy>>>,
        upstream: Arc<dyn Upstream>,
        push: Arc<Push>,
    ) -> Self {
        Self {
            policy,
            upstream,
            push,
            fetch_max_bytes: MAX_FETCH_REQUEST_BYTES,
        }
    }

    /// Overrides the largest forwarded `git-upload-pack` body ([`MAX_FETCH_REQUEST_BYTES`]).
    #[must_use]
    pub fn with_fetch_max_bytes(mut self, max_bytes: u64) -> Self {
        self.fetch_max_bytes = max_bytes;
        self
    }

    /// Answers one request of the sandbox at `source`.
    pub async fn handle<B>(&self, source: IpAddr, request: Request<B>) -> Response<Body>
    where
        B: hyper::body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let (parts, body) = request.into_parts();
        let Some(target) = Target::parse(&parts.method, parts.uri.path(), parts.uri.query()) else {
            return text(StatusCode::NOT_FOUND, "df: not found");
        };
        let policy = self.policy.borrow().clone();
        let (forge, run_id) = match identify(policy.as_deref(), source, &target.key) {
            Ok(found) => found,
            Err(rejection) => {
                tracing::warn!(
                    %source,
                    task_key = %target.key,
                    reason = rejection.reason(),
                    "git request denied"
                );
                return rejection.response();
            }
        };
        let headers = forwarded_headers(target.service, &parts.headers);
        let request = GitRequest {
            task_key: target.key,
            service: target.service,
            operation: target.operation,
            method: parts.method,
            headers,
            body: body.map_err(Into::into).boxed_unsync(),
        };
        if (request.service, request.operation) == (Service::ReceivePack, Operation::Rpc) {
            return self
                .push
                .handle(self.upstream.as_ref(), forge, run_id, request)
                .await;
        }
        self.fetch(forge, request).await
    }

    /// Forwards a fetch request: no body for `info/refs`, a bounded one for the RPC.
    async fn fetch(&self, forge: ForgeTarget<'_>, mut request: GitRequest) -> Response<Body> {
        if request.operation == Operation::Advertise {
            request.body = Full::new(Bytes::new())
                .map_err(|never| match never {})
                .boxed_unsync();
            return self.upstream.forward(forge, request).await;
        }
        let body = std::mem::take(&mut request.body);
        let limited = LimitedBody::new(body, self.fetch_max_bytes, BODY_IDLE_TIMEOUT);
        let limit = limited.limit();
        request.body = limited.boxed_unsync();
        let response = self.upstream.forward(forge, request).await;
        match limit.get() {
            Some(BodyLimit::TooLarge(max_bytes)) => {
                tracing::warn!(max_bytes, "fetch request body too large");
                text(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    &format!("df:fetch-too-large: the request exceeds {max_bytes} bytes"),
                )
            }
            Some(BodyLimit::TimedOut) => {
                text(StatusCode::REQUEST_TIMEOUT, "df: request body timed out")
            }
            None => response,
        }
    }

    /// Serves the Git port on `listener` until `shutdown` resolves.
    pub async fn serve(
        self: Arc<Self>,
        listener: TcpListener,
        tls: Arc<ServerTls>,
        shutdown: impl Future<Output = ()>,
    ) {
        let stop = CancellationToken::new();
        let mut connections = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            while connections.try_join_next().is_some() {}
            tokio::select! {
                () = &mut shutdown => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let source = peer.ip().to_canonical();
                        connections.spawn(self.clone().connection(stream, source, tls.clone(), stop.clone()));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "accept failed on the git port; backing off");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
        stop.cancel();
        let drained = async { while connections.join_next().await.is_some() {} };
        // A timeout only means the drain gave up; the connections are aborted next.
        tokio::time::timeout(SHUTDOWN_DRAIN, drained).await.ok();
        connections.abort_all();
    }

    async fn connection(
        self: Arc<Self>,
        stream: TcpStream,
        source: IpAddr,
        tls: Arc<ServerTls>,
        stop: CancellationToken,
    ) {
        let acceptor = match tls.acceptor().await {
            Ok(acceptor) => acceptor,
            Err(error) => {
                tracing::error!(%error, "git port TLS material is unusable");
                return;
            }
        };
        let stream =
            match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    tracing::debug!(%source, %error, "git port TLS handshake failed");
                    return;
                }
                Err(_elapsed) => {
                    tracing::debug!(%source, "git port TLS handshake timed out");
                    return;
                }
            };
        let service = service_fn(move |request| {
            let endpoint = self.clone();
            async move { Ok::<_, Infallible>(endpoint.handle(source, request).await) }
        });
        let connection = http1::Builder::new()
            .timer(TokioTimer::new())
            .header_read_timeout(HEADER_READ_TIMEOUT)
            .serve_connection(TokioIo::new(stream), service);
        tokio::pin!(connection);
        let served = tokio::select! {
            served = connection.as_mut() => served,
            () = stop.cancelled() => {
                connection.as_mut().graceful_shutdown();
                connection.await
            }
        };
        if let Err(error) = served {
            tracing::debug!(%source, %error, "git connection ended with an error");
        }
    }
}

/// Failure to load the gateway certificate for the Git port.
#[derive(Debug, Error)]
pub enum TlsError {
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid TLS material: {0}")]
    Invalid(String),
}

/// The gateway certificate of the Git port. The files are read for every connection, so a renewed
/// certificate is used without a restart. Sandboxes present no client certificate.
pub struct ServerTls {
    cert_file: PathBuf,
    key_file: PathBuf,
}

impl ServerTls {
    pub fn new(cert_file: PathBuf, key_file: PathBuf) -> Self {
        Self {
            cert_file,
            key_file,
        }
    }

    /// An acceptor built from the current contents of the files.
    pub async fn acceptor(&self) -> Result<TlsAcceptor, TlsError> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| TlsError::Invalid(format!("{what}: {e}"));
        let cert = read(&self.cert_file).await?;
        let key = read(&self.key_file).await?;
        let certs = CertificateDer::pem_slice_iter(&cert)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid("server certificate", &e))?;
        let key = PrivateKeyDer::from_pem_slice(&key).map_err(|e| invalid("server key", &e))?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| invalid("protocol versions", &e))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| invalid("server certificate and key", &e))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

/// A plain-text answer; Git shows the body of an error to the user as `remote:` lines.
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

/// Why a request is not forwarded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Rejection {
    NoSnapshot,
    UnknownSource,
    OtherTask {
        own: String,
    },
    NoRepository,
    /// The route names a credential the snapshot does not contain. That is a control plane
    /// configuration error, not a transient state, so the answer is 502 rather than 503.
    CredentialUnavailable,
}

impl Rejection {
    const fn reason(&self) -> &'static str {
        match self {
            Self::NoSnapshot => "no-snapshot",
            Self::UnknownSource => "unknown-source",
            Self::OtherTask { .. } => "other-task",
            Self::NoRepository => "no-repository",
            Self::CredentialUnavailable => "credential-unavailable",
        }
    }

    fn response(&self) -> Response<Body> {
        match self {
            Self::NoSnapshot => text(
                StatusCode::SERVICE_UNAVAILABLE,
                "df: no policy snapshot yet",
            ),
            Self::UnknownSource => text(StatusCode::FORBIDDEN, "df: unknown source"),
            Self::OtherTask { own } => text(
                StatusCode::FORBIDDEN,
                &format!("df: this sandbox may only access {own}"),
            ),
            Self::NoRepository => text(StatusCode::FORBIDDEN, "df: no repository for this task"),
            Self::CredentialUnavailable => {
                text(StatusCode::BAD_GATEWAY, "df: forge credential unavailable")
            }
        }
    }
}

async fn read(path: &Path) -> Result<Vec<u8>, TlsError> {
    tokio::fs::read(path)
        .await
        .map_err(|source| TlsError::Read {
            path: path.display().to_string(),
            source,
        })
}

/// Repository and step a request path names.
struct Target {
    key: TaskKey,
    service: Service,
    operation: Operation,
}

impl Target {
    /// `/git/<KEY>.git/info/refs?service=…` (GET) or `/git/<KEY>.git/<service>` (POST).
    fn parse(method: &Method, path: &str, query: Option<&str>) -> Option<Self> {
        let (key, tail) = path.strip_prefix("/git/")?.split_once(".git/")?;
        let parsed: TaskKey = key.parse().ok()?;
        // Only the canonical spelling: `DF-01` would name the repository of `DF-1`.
        if parsed.to_string() != key {
            return None;
        }
        let (service, operation) = match (method, tail) {
            (&Method::GET, "info/refs") => {
                let name = query?
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("service="))?;
                (Service::parse(name)?, Operation::Advertise)
            }
            (&Method::POST, name) => (Service::parse(name)?, Operation::Rpc),
            _ => return None,
        };
        Some(Self {
            key: parsed,
            service,
            operation,
        })
    }
}

/// The forge target and the run id of the sandbox at `source`, if it may access `key`.
fn identify<'a>(
    policy: Option<&'a Policy>,
    source: IpAddr,
    key: &TaskKey,
) -> Result<(ForgeTarget<'a>, &'a str), Rejection> {
    let policy = policy.ok_or(Rejection::NoSnapshot)?;
    let sandbox = policy.sandbox(source).ok_or(Rejection::UnknownSource)?;
    if sandbox.task_key != key.to_string() {
        return Err(Rejection::OtherTask {
            own: sandbox.task_key.clone(),
        });
    }
    let route = policy.git_route(source).ok_or(Rejection::NoRepository)?;
    let credential = policy
        .forge_credential(&route.credential_ref)
        .ok_or(Rejection::CredentialUnavailable)?;
    Ok((ForgeTarget::new(route, credential), &sandbox.run_id))
}

/// The [`FORWARDED_HEADERS`] of `headers` that apply to `service`.
fn forwarded_headers(service: Service, headers: &HeaderMap) -> HeaderMap {
    headers
        .iter()
        .filter(|(name, _)| FORWARDED_HEADERS.contains(&name.as_str()))
        .filter(|(name, _)| service == Service::UploadPack || *name != CONTENT_ENCODING)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
