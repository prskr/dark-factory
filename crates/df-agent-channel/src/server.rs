//! The agent port: tonic gRPC server with mutual TLS (rustls/ring) and identity binding.
//!
//! TLS is terminated here, not by tonic, so the certificate, key and client CA can be reloaded
//! when cert-manager rotates the mounted secret, without restarting the pod or dropping streams.

use std::future::Future;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use df_core::config::AgentTlsConfig;
use df_core::{PeerIdentity, RunId};
use df_proto::agent::v1::agent_channel_server::{AgentChannel, AgentChannelServer};
use df_proto::agent::v1::{AgentMessage, ControlMessage};
use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

use crate::identity::pod_uri_san;
use crate::runs::{RunRow, get_run};
use crate::{ChannelContext, session};

/// Largest accepted message (ADR-0027: transcript events may carry images).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(20);
/// Time a client has to complete the TLS handshake.
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Outbound buffer per stream; a stalled runtime applies backpressure to its own session only.
const OUTBOUND_BUFFER: usize = 64;
/// Connections whose handshake completed and wait for the HTTP/2 server to pick them up.
const ACCEPTED_BACKLOG: usize = 16;
/// Handshakes in progress at the same time; further connections wait in the accept queue.
const MAX_HANDSHAKES: usize = 256;
/// Time sessions get to finish in-flight handlers when the server shuts down; below the control
/// plane's overall grace period.
const SESSION_DRAIN: Duration = Duration::from_secs(20);
/// How often connections look at the PEM files for changes.
const TLS_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Failure to load or build the agent port's TLS material.
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

/// Raw contents of the three PEM files.
#[derive(PartialEq, Eq)]
struct TlsFiles {
    cert: Vec<u8>,
    key: Vec<u8>,
    client_ca: Vec<u8>,
}

impl TlsFiles {
    async fn read(config: &AgentTlsConfig) -> Result<Self, TlsError> {
        let read = |path: &std::path::Path| {
            let path = path.to_path_buf();
            async move {
                tokio::fs::read(&path)
                    .await
                    .map_err(|source| TlsError::Read {
                        path: path.display().to_string(),
                        source,
                    })
            }
        };
        Ok(Self {
            cert: read(&config.cert_file).await?,
            key: read(&config.key_file).await?,
            client_ca: read(&config.client_ca_file).await?,
        })
    }

    /// A TLS acceptor that requires client certificates chaining to the CA bundle.
    fn acceptor(&self) -> Result<TlsAcceptor, TlsError> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| TlsError::Invalid(format!("{what}: {e}"));
        let certs = CertificateDer::pem_slice_iter(&self.cert)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid("server certificate", &e))?;
        let key =
            PrivateKeyDer::from_pem_slice(&self.key).map_err(|e| invalid("server key", &e))?;
        let mut roots = RootCertStore::empty();
        for ca in CertificateDer::pem_slice_iter(&self.client_ca) {
            let ca = ca.map_err(|e| invalid("client CA", &e))?;
            roots.add(ca).map_err(|e| invalid("client CA", &e))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier =
            WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .map_err(|e| invalid("client CA", &e))?;
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| invalid("protocol versions", &e))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| invalid("server certificate and key", &e))?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

/// What the reloader serves: the files, the acceptor built from them and when they were last read.
struct Loaded {
    files: TlsFiles,
    acceptor: TlsAcceptor,
    checked_at: Instant,
}

/// Serves TLS handshakes from the current contents of the PEM files: a connection that arrives
/// at least [`TLS_CHECK_INTERVAL`] after the last look compares the files with what was loaded and
/// switches to the new material if they changed. Material that does not parse is ignored (and
/// logged) until it is fixed.
pub struct TlsReloader {
    config: AgentTlsConfig,
    check_every: Duration,
    current: Mutex<Loaded>,
}

impl TlsReloader {
    /// Loads the material; fails if it is missing or invalid, so a bad deployment stops at startup.
    pub async fn load(config: &AgentTlsConfig) -> Result<Self, TlsError> {
        let files = TlsFiles::read(config).await?;
        let acceptor = files.acceptor()?;
        Ok(Self {
            config: config.clone(),
            check_every: TLS_CHECK_INTERVAL,
            current: Mutex::new(Loaded {
                files,
                acceptor,
                checked_at: Instant::now(),
            }),
        })
    }

    /// Looks at the files at most every `interval` (default: 10 s).
    #[must_use]
    pub fn check_every(mut self, interval: Duration) -> Self {
        self.check_every = interval;
        self
    }

    fn loaded(&self) -> std::sync::MutexGuard<'_, Loaded> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The acceptor for the next connection, after picking up changed files.
    async fn acceptor(&self) -> TlsAcceptor {
        {
            let mut current = self.loaded();
            if current.checked_at.elapsed() < self.check_every {
                return current.acceptor.clone();
            }
            // Claimed before reading, so concurrent connections don't all read the files.
            current.checked_at = Instant::now();
        }
        let latest = TlsFiles::read(&self.config).await;
        let mut current = self.loaded();
        match latest {
            Ok(files) if files != current.files => match files.acceptor() {
                Ok(acceptor) => {
                    tracing::info!("agent port TLS material reloaded");
                    current.files = files;
                    current.acceptor = acceptor;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "keeping the previous agent port TLS material");
                }
            },
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "keeping the previous agent port TLS material"),
        }
        current.acceptor.clone()
    }
}

/// Serves the agent port on `listener` until `shutdown` resolves.
pub async fn serve(
    ctx: ChannelContext,
    listener: TcpListener,
    tls: TlsReloader,
    shutdown: impl Future<Output = ()>,
) -> Result<(), tonic::transport::Error> {
    let incoming = mtls_incoming(listener, tls);
    let service = AgentService::new(ctx);
    let owner = service.clone();
    // Graceful shutdown waits for open streams, which the sessions keep open: end them first.
    // Whatever they had not finished is derived from the database again after a reconnect.
    let shutdown = async move {
        shutdown.await;
        owner.stop_sessions().await;
    };
    Server::builder()
        .http2_keepalive_interval(Some(KEEPALIVE_INTERVAL))
        .http2_keepalive_timeout(Some(KEEPALIVE_TIMEOUT))
        .add_service(
            AgentChannelServer::new(service)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
        )
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await
}

/// Connections of `listener` with completed mutual TLS handshakes, as input of a tonic server.
/// Accepting stops when the stream is dropped.
pub fn mtls_incoming(listener: TcpListener, tls: TlsReloader) -> MtlsIncoming {
    let (accepted, incoming) = mpsc::channel(ACCEPTED_BACKLOG);
    MtlsIncoming {
        incoming: ReceiverStream::new(incoming),
        acceptor: tokio::spawn(accept_loop(listener, Arc::new(tls), accepted)),
    }
}

/// See [`mtls_incoming`].
pub struct MtlsIncoming {
    incoming: ReceiverStream<Result<TlsStream<TcpStream>, std::io::Error>>,
    acceptor: tokio::task::JoinHandle<()>,
}

impl futures::Stream for MtlsIncoming {
    type Item = Result<TlsStream<TcpStream>, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.incoming).poll_next(cx)
    }
}

impl Drop for MtlsIncoming {
    fn drop(&mut self) {
        self.acceptor.abort();
    }
}

/// Accepts TCP connections and completes their TLS handshakes concurrently, each bounded by
/// [`TLS_HANDSHAKE_TIMEOUT`]. Failed handshakes only concern the client and are not forwarded.
async fn accept_loop(
    listener: TcpListener,
    tls: Arc<TlsReloader>,
    accepted: mpsc::Sender<Result<TlsStream<TcpStream>, std::io::Error>>,
) {
    let mut handshakes = JoinSet::new();
    let slots = Arc::new(Semaphore::new(MAX_HANDSHAKES));
    loop {
        while handshakes.try_join_next().is_some() {}
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let (tcp, remote) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "accept failed on an mTLS port; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let (tls, accepted) = (tls.clone(), accepted.clone());
        handshakes.spawn(async move {
            let acceptor = tls.acceptor().await;
            let handshake = tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await;
            drop(slot);
            match handshake {
                Ok(Ok(stream)) => {
                    // The receiver is gone when the server stopped.
                    accepted.send(Ok(stream)).await.ok(); // receiver may be gone
                }
                Ok(Err(e)) => tracing::debug!(error = %e, %remote, "TLS handshake failed"),
                Err(_) => tracing::debug!(%remote, "TLS handshake timed out"),
            }
        });
    }
}

/// `AgentChannel` implementation; stateless apart from the shared context and the sessions it
/// owns.
#[derive(Clone)]
pub struct AgentService {
    ctx: ChannelContext,
    /// Running sessions. Dropping the service (the server stopped) aborts them.
    sessions: Arc<Mutex<JoinSet<()>>>,
    /// Set when the server shuts down: sessions finish their in-flight work and end.
    stop: Arc<watch::Sender<bool>>,
}

impl AgentService {
    pub fn new(ctx: ChannelContext) -> Self {
        Self {
            ctx,
            sessions: Arc::default(),
            stop: Arc::new(watch::channel(false).0),
        }
    }

    /// Asks all sessions to stop and waits for them to finish their in-flight tool calls; what
    /// takes longer than [`SESSION_DRAIN`] is aborted.
    async fn stop_sessions(&self) {
        self.stop.send_replace(true);
        let mut sessions = std::mem::take(
            &mut *self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let finished = async { while sessions.join_next().await.is_some() {} };
        if tokio::time::timeout(SESSION_DRAIN, finished).await.is_err() {
            tracing::warn!("aborting sessions still busy at shutdown");
        }
        // Dropping the set aborts what is left.
    }

    fn spawn_session(&self, session: impl Future<Output = ()> + Send + 'static) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while sessions.try_join_next().is_some() {}
        sessions.spawn(session);
    }
}

type ControlStream = ReceiverStream<Result<ControlMessage, Status>>;

#[tonic::async_trait]
impl AgentChannel for AgentService {
    type ConnectStream = ControlStream;

    async fn connect(
        &self,
        request: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let client_cert = request
            .peer_certs()
            .and_then(|certs| certs.first().cloned());
        let run = authenticate(
            &self.ctx,
            client_cert.as_ref().map(AsRef::as_ref),
            request.remote_addr().map(|addr| addr.ip()),
        )
        .await?;

        let (tx, rx) = mpsc::channel(OUTBOUND_BUFFER);
        let ctx = self.ctx.clone();
        let inbound = request.into_inner();
        let stop = self.stop.subscribe();
        self.spawn_session(async move {
            if let Err(status) = session::run(ctx, run, inbound, tx.clone(), stop).await {
                // Best effort: the peer may already be gone.
                tx.send(Err(status)).await.ok(); // receiver may be gone
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Binds a connection to its active run: the certificate must carry a pod SAN, the provider must
/// resolve SAN and peer IP to a run, and that run must not be finished.
pub async fn authenticate(
    ctx: &ChannelContext,
    client_cert_der: Option<&[u8]>,
    peer_ip: Option<IpAddr>,
) -> Result<RunRow, Status> {
    let unauthenticated = |reason: &str| Status::unauthenticated(reason.to_string());
    let der = client_cert_der.ok_or_else(|| unauthenticated("no client certificate"))?;
    let peer_ip = peer_ip.ok_or_else(|| unauthenticated("unknown peer address"))?;
    let uri_san = pod_uri_san(der).map_err(|e| unauthenticated(&e.to_string()))?;

    let run_id: RunId = ctx
        .provider
        .resolve_identity(&PeerIdentity { uri_san, peer_ip })
        .await
        .ok_or_else(|| unauthenticated("unknown sandbox"))?;
    let run = get_run(&ctx.pool, run_id)
        .await?
        .filter(|run| !run.state.is_terminal())
        .ok_or_else(|| unauthenticated("run is not active"))?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Env;

    /// Signals through a channel when dropped.
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                tx.send(()).ok(); // receiver may be gone
            }
        }
    }

    #[tokio::test]
    async fn pem_files_are_not_read_for_every_connection() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::testing::TestPki::new()
            .unwrap()
            .write_agent_tls(dir.path())
            .unwrap();
        let reloader = TlsReloader::load(&config).await.unwrap();
        let loaded = |r: &TlsReloader| r.loaded().files.cert.clone();
        let before = loaded(&reloader);

        // cert-manager renewed the certificate.
        crate::testing::TestPki::new()
            .unwrap()
            .write_agent_tls(dir.path())
            .unwrap();
        reloader.acceptor().await;
        assert_eq!(
            loaded(&reloader),
            before,
            "looked at the files again too soon"
        );

        let reloader = reloader.check_every(Duration::ZERO);
        reloader.acceptor().await;
        assert_ne!(loaded(&reloader), before);
    }

    #[tokio::test]
    async fn sessions_end_with_the_service_that_owns_them() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let service = AgentService::new(env.ctx.clone());
        let (tx, ended) = tokio::sync::oneshot::channel();
        let signal = DropSignal(Some(tx));
        service.spawn_session(async move {
            let _signal = signal;
            std::future::pending::<()>().await;
        });

        drop(service);

        // The task was aborted, which dropped its state.
        assert!(ended.await.is_ok());
    }
}
