//! `GatewayPolicy.PolicyWatch`: pushes the snapshot to every connected gateway pod.
//!
//! Each stream rebuilds the snapshot from the database after `egress_policy` and `run_changed`
//! notifications (debounced) and sends it, and at the latest every [`REFRESH`], so rotated forge
//! Secrets reach the gateways. All replicas receive the notifications, so a gateway gets its
//! updates from whichever replica it is connected to.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use df_core::SecretStore;
use df_db::{Notification, Notifier, PgPool};
use df_proto::gateway::v1::gateway_policy_server::GatewayPolicy;
use df_proto::gateway::v1::{PolicyUpdate, PolicyWatchRequest};
use futures::{Stream, stream};
use tokio::sync::broadcast;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use super::snapshot::{self, GitSettings};

/// Wait after a notification before rebuilding, so a burst of changes causes one update.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// Longest time between two snapshots of a stream, so changed Secrets (no notification) arrive.
pub const REFRESH: Duration = Duration::from_secs(60);

/// Serves `PolicyWatch` from the database.
pub struct PolicyService {
    pub(super) pool: PgPool,
    notifier: Arc<Notifier>,
    secrets: Arc<dyn SecretStore>,
    git: GitSettings,
    debounce: Duration,
    refresh: Duration,
    /// Ends the streams when the server shuts down.
    stop: CancellationToken,
}

type UpdateStream = Pin<Box<dyn Stream<Item = Result<PolicyUpdate, Status>> + Send>>;

impl PolicyService {
    pub fn new(
        pool: PgPool,
        notifier: Arc<Notifier>,
        secrets: Arc<dyn SecretStore>,
        git: GitSettings,
        stop: CancellationToken,
    ) -> Self {
        Self {
            pool,
            notifier,
            secrets,
            git,
            debounce: DEBOUNCE,
            refresh: REFRESH,
            stop,
        }
    }

    #[must_use]
    pub fn debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// Longest time between two snapshots of a stream (default [`REFRESH`]).
    #[must_use]
    pub fn refresh(mut self, refresh: Duration) -> Self {
        self.refresh = refresh;
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
            secrets: Arc::clone(&self.secrets),
            git: self.git.clone(),
            changes: self.notifier.subscribe(),
            debounce: self.debounce,
            refresh: refresh_interval(self.refresh),
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

/// Ticks every `period`, first after one period; a late tick does not cause a burst.
fn refresh_interval(period: Duration) -> Interval {
    let mut refresh = tokio::time::interval_at(Instant::now() + period, period);
    refresh.set_missed_tick_behavior(MissedTickBehavior::Delay);
    refresh
}

/// The state of one `PolicyWatch` stream.
struct Watch {
    pool: PgPool,
    secrets: Arc<dyn SecretStore>,
    git: GitSettings,
    changes: broadcast::Receiver<Notification>,
    debounce: Duration,
    refresh: Interval,
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
        let built = snapshot::build(&self.pool, self.secrets.as_ref(), &self.git).await;
        // The period starts once the snapshot is ready, however long the build took.
        self.refresh.reset();
        Some(built.map_err(|error| {
            tracing::error!(%error, "failed to build the policy snapshot");
            Status::internal("policy snapshot failed")
        }))
    }

    /// Waits for a relevant notification and the end of its burst, or for the refresh period.
    /// `false` if the stream should end.
    async fn wait_for_change(&mut self) -> bool {
        loop {
            // All branches are cancel-safe: a lost tick or message is covered by the rebuild.
            let received = tokio::select! {
                () = self.stop.cancelled() => return false,
                _ = self.refresh.tick() => return true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::tests::{TestSecrets, git_settings};

    fn watch(changes: broadcast::Receiver<Notification>) -> Watch {
        Watch {
            // Never connected: these tests only wait.
            pool: PgPool::connect_lazy("postgres://unused@127.0.0.1:1/none").unwrap(),
            secrets: Arc::new(TestSecrets::new("token")),
            git: git_settings(),
            changes,
            debounce: DEBOUNCE,
            refresh: refresh_interval(REFRESH),
            stop: CancellationToken::new(),
            first: false,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn without_notifications_the_watch_rebuilds_after_the_refresh_period() {
        let (_sender, changes) = broadcast::channel(4);
        let mut watch = watch(changes);
        let start = Instant::now();
        assert!(watch.wait_for_change().await);
        assert_eq!(start.elapsed(), REFRESH);
        assert!(watch.wait_for_change().await);
        assert_eq!(start.elapsed(), REFRESH * 2);
    }
}
