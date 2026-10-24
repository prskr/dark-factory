//! The gateway's `PolicyWatch` client: keeps the policy of the pod current.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use df_proto::gateway::v1::PolicyWatchRequest;
use df_proto::gateway::v1::gateway_policy_client::GatewayPolicyClient;
use thiserror::Error;
use tokio::sync::watch;

use crate::link::{GrpcConnector, LinkError};
use crate::metrics::Metrics;
use crate::policy::Policy;

/// Pause before the next attempt to reach a control plane replica.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
enum WatchError {
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    Status(#[from] tonic::Status),
    #[error("the control plane closed the stream")]
    Closed,
}

/// Opens `PolicyWatch` to `control_plane` and publishes every snapshot to `policy`, reconnecting
/// forever (the Service picks any replica). While no connection exists, the last snapshot stays
/// in force. Runs until dropped.
pub async fn run(
    connector: Arc<dyn GrpcConnector>,
    control_plane: String,
    pod: String,
    policy: watch::Sender<Option<Arc<Policy>>>,
    retry: Duration,
    metrics: Arc<Metrics>,
) {
    loop {
        // A session only ever ends with an error.
        let Err(error) = session(&*connector, &control_plane, &pod, &policy, &metrics).await;
        tracing::warn!(%error, %control_plane, "policy watch ended; reconnecting");
        tokio::time::sleep(retry).await;
    }
}

async fn session(
    connector: &dyn GrpcConnector,
    control_plane: &str,
    pod: &str,
    policy: &watch::Sender<Option<Arc<Policy>>>,
    metrics: &Metrics,
) -> Result<Infallible, WatchError> {
    let mut client = GatewayPolicyClient::new(connector.connect(control_plane).await?);
    let mut updates = client
        .policy_watch(PolicyWatchRequest {
            gateway_pod: pod.to_string(),
        })
        .await?
        .into_inner();
    // The first message of a connection is a full snapshot and replaces whatever is in force,
    // even with a lower version (a replica that restarted its sequence). Later ones must be
    // newer, so a delayed update cannot roll the policy back.
    let mut first = true;
    while let Some(update) = updates.message().await? {
        let current = policy.borrow().as_ref().map(|p| p.version);
        if first || current.is_none_or(|version| update.version > version) {
            tracing::debug!(version = update.version, "policy snapshot applied");
            metrics.policy_applied(update.version);
            policy.send_replace(Some(Arc::new(Policy::from_update(update))));
        }
        first = false;
    }
    Err(WatchError::Closed)
}
