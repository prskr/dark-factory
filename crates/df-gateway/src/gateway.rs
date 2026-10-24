//! `dark-factory gateway`: proxy, `Tail` server, policy client and health endpoint of one pod.

use std::sync::Arc;
use std::time::Duration;

use futures::Stream;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::flow::FlowLog;
use crate::link::GrpcConnector;
use crate::policy::Policy;
use crate::proxy::{Dialer, Proxy};
use crate::resolve::{BlockedRanges, Resolver};
use crate::{health, tail, watch as policy_watch};

/// Settings of a gateway pod.
pub struct Settings {
    /// Name of the pod, recorded in flow events.
    pub pod: String,
    /// `host:port` of the control plane's internal port.
    pub control_plane: String,
    /// Events kept in the ring buffer.
    pub buffer_size: usize,
    /// Cluster pod and Service CIDRs on top of the built-in blocked ranges.
    pub blocked_cidrs: Vec<String>,
    /// Pause between attempts to reach the control plane.
    pub retry: Duration,
    /// DNS name the control plane's certificate must have to call `Tail`; `None` accepts any
    /// peer the TLS layer let in (tests without TLS).
    pub control_plane_name: Option<String>,
}

/// The parts of a gateway that talk to the outside world.
pub struct Outside {
    pub connector: Arc<dyn GrpcConnector>,
    pub resolver: Arc<dyn Resolver>,
    pub dialer: Arc<dyn Dialer>,
}

/// Why the gateway stopped.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid blocked CIDR: {0}")]
    BlockedCidr(#[from] ipnet::AddrParseError),
    #[error("{0} stopped unexpectedly")]
    Stopped(&'static str),
}

/// Runs the gateway until `shutdown` is cancelled (`Ok`) or one of its parts stops (`Err`).
///
/// `policy` holds the latest snapshot (`None` until the first one arrived); the caller may
/// subscribe to it to observe the policy of the pod.
pub async fn run<I, IO>(
    settings: Settings,
    policy_tx: watch::Sender<Option<Arc<Policy>>>,
    outside: Outside,
    health_listener: TcpListener,
    proxy_listener: TcpListener,
    tail_incoming: I,
    shutdown: CancellationToken,
) -> Result<(), GatewayError>
where
    I: Stream<Item = Result<IO, std::io::Error>> + Send + 'static,
    IO: tokio::io::AsyncRead
        + tokio::io::AsyncWrite
        + tonic::transport::server::Connected
        + Unpin
        + Send
        + 'static,
{
    let blocked = BlockedRanges::new(settings.blocked_cidrs.iter().map(String::as_str))?;
    let flows = Arc::new(FlowLog::new(settings.buffer_size));
    let policy_rx = policy_tx.subscribe();
    let proxy = Arc::new(Proxy::new(
        settings.pod.clone(),
        policy_rx.clone(),
        outside.resolver,
        outside.dialer,
        blocked,
        flows.clone(),
    ));

    let mut parts = JoinSet::new();
    let stop = shutdown.clone();
    parts.spawn(async move {
        proxy.serve(proxy_listener, stop.cancelled_owned()).await;
        "the proxy"
    });
    let stop = shutdown.clone();
    parts.spawn(async move {
        let served = tail::serve(
            flows,
            tail_incoming,
            settings.control_plane_name,
            stop.cancelled_owned(),
        )
        .await;
        if let Err(error) = served {
            tracing::error!(%error, "tail server failed");
        }
        "the tail server"
    });
    let stop = shutdown.clone();
    parts.spawn(async move {
        let router = health::router(policy_rx);
        if let Err(error) = axum::serve(health_listener, router)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
        {
            tracing::error!(%error, "health server failed");
        }
        "the health endpoint"
    });
    parts.spawn(async move {
        policy_watch::run(
            outside.connector,
            settings.control_plane,
            settings.pod,
            policy_tx,
            settings.retry,
        )
        .await;
        "the policy watch"
    });

    let finished = parts.join_next().await;
    parts.abort_all();
    match finished {
        Some(Ok(part)) if !shutdown.is_cancelled() => Err(GatewayError::Stopped(part)),
        Some(Err(_)) if !shutdown.is_cancelled() => Err(GatewayError::Stopped("a gateway task")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
