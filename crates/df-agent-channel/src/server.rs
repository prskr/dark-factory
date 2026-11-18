//! The agent port: tonic gRPC server with mutual TLS (rustls/ring) and identity binding.
//!
//! TLS is terminated here, not by tonic, so the certificate, key and client CA can be reloaded
//! when cert-manager rotates the mounted secret, without restarting the pod or dropping streams.

use std::future::Future;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use df_core::{PeerIdentity, RunId};
use df_db::runs::{RunRow, get_run};
use df_proto::agent::v1::agent_channel_server::{AgentChannel, AgentChannelServer};
use df_proto::agent::v1::{AgentMessage, ControlMessage};
use df_tls::{TlsReloader, mtls_incoming, pod_uri_san};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

use crate::{ChannelContext, session};

/// Largest accepted message (ADR-0027: transcript events may carry images).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(20);
/// Outbound buffer per stream; a stalled runtime applies backpressure to its own session only.
const OUTBOUND_BUFFER: usize = 64;
/// Time sessions get to finish in-flight handlers when the server shuts down; below the control
/// plane's overall grace period.
const SESSION_DRAIN: Duration = Duration::from_secs(20);

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
        .await
        .map_err(crate::ChannelError::from)?
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
