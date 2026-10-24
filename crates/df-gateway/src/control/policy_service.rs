//! `GatewayPolicy.PolicyWatch`: pushes the snapshot to every connected gateway pod.
//!
//! Each stream rebuilds the snapshot from the database after `egress_policy` and `run_changed`
//! notifications (debounced) and sends it. All replicas receive the notifications, so a gateway
//! gets its updates from whichever replica it is connected to.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use df_db::{Notification, Notifier, PgPool};
use df_proto::gateway::v1::gateway_policy_server::{GatewayPolicy, GatewayPolicyServer};
use df_proto::gateway::v1::{PolicyUpdate, PolicyWatchRequest};
use futures::{Stream, stream};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use super::snapshot;
use crate::peer::require_dns_san;

/// Wait after a notification before rebuilding, so a burst of changes causes one update.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// Serves `PolicyWatch` from the database.
pub struct PolicyService {
    pool: PgPool,
    notifier: Arc<Notifier>,
    debounce: Duration,
    /// Ends the streams when the server shuts down.
    stop: CancellationToken,
}

type UpdateStream = Pin<Box<dyn Stream<Item = Result<PolicyUpdate, Status>> + Send>>;

impl PolicyService {
    pub fn new(pool: PgPool, notifier: Arc<Notifier>, stop: CancellationToken) -> Self {
        Self {
            pool,
            notifier,
            debounce: DEBOUNCE,
            stop,
        }
    }

    pub fn debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }
}

#[tonic::async_trait]
impl GatewayPolicy for PolicyService {
    type PolicyWatchStream = UpdateStream;

    async fn policy_watch(
        &self,
        request: Request<PolicyWatchRequest>,
    ) -> Result<Response<UpdateStream>, Status> {
        let gateway = request.into_inner().gateway_pod;
        tracing::info!(%gateway, "gateway connected to policy watch");
        // Subscribed before the first snapshot is read: a change in between is not lost.
        let watch = Watch {
            pool: self.pool.clone(),
            changes: self.notifier.subscribe(),
            debounce: self.debounce,
            stop: self.stop.clone(),
            first: true,
        };
        // The stream is polled by tonic and ends with the client's connection: no task of ours.
        let updates = stream::unfold(Some(watch), |watch| async move {
            let mut watch = watch?;
            match watch.next().await? {
                Ok(update) => Some((Ok(update), Some(watch))),
                // The error is the last item: the gateway reconnects and gets a fresh snapshot.
                Err(status) => Some((Err(status), None)),
            }
        });
        Ok(Response::new(Box::pin(updates)))
    }
}

fn is_relevant(notification: &Notification) -> bool {
    matches!(
        notification,
        Notification::EgressPolicy | Notification::RunChanged { .. } | Notification::Reconnected
    )
}

/// The state of one `PolicyWatch` stream.
struct Watch {
    pool: PgPool,
    changes: broadcast::Receiver<Notification>,
    debounce: Duration,
    stop: CancellationToken,
    /// The first update is sent without waiting for a change.
    first: bool,
}

impl Watch {
    /// The next snapshot; `None` once the server stops.
    async fn next(&mut self) -> Option<Result<PolicyUpdate, Status>> {
        if !std::mem::take(&mut self.first) && !self.wait_for_change().await {
            return None;
        }
        Some(snapshot::build(&self.pool).await.map_err(|error| {
            tracing::error!(%error, "failed to build the policy snapshot");
            Status::internal("policy snapshot failed")
        }))
    }

    /// Waits for a relevant notification and the end of its burst. `false` if the stream should
    /// end.
    async fn wait_for_change(&mut self) -> bool {
        loop {
            let received = tokio::select! {
                () = self.stop.cancelled() => return false,
                received = self.changes.recv() => received,
            };
            match received {
                Ok(notification) if !is_relevant(&notification) => {}
                Err(broadcast::error::RecvError::Closed) => return false,
                // A relevant notification, or missed ones that may have been.
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => break,
            }
        }
        tokio::select! {
            () = self.stop.cancelled() => return false,
            () = tokio::time::sleep(self.debounce) => {}
        }
        // Everything that arrived meanwhile is covered by the rebuild that follows.
        while self.changes.try_recv().is_ok() {}
        true
    }
}

/// Serves `PolicyWatch` on `incoming` until `shutdown` is cancelled. Only a peer whose
/// certificate has the DNS name `gateway_name` may call it; `None` accepts any peer the TLS layer
/// let in (tests without TLS).
pub async fn serve<I, IO>(
    service: PolicyService,
    incoming: I,
    gateway_name: Option<String>,
    shutdown: CancellationToken,
) -> Result<(), tonic::transport::Error>
where
    I: Stream<Item = Result<IO, std::io::Error>>,
    IO: tokio::io::AsyncRead
        + tokio::io::AsyncWrite
        + tonic::transport::server::Connected
        + Unpin
        + Send
        + 'static,
{
    let guard = move |request: Request<()>| match &gateway_name {
        Some(name) => require_dns_san(&request, name).map(|()| request),
        None => Ok(request),
    };
    Server::builder()
        .add_service(GatewayPolicyServer::with_interceptor(service, guard))
        .serve_with_incoming_shutdown(incoming, shutdown.cancelled_owned())
        .await
}
