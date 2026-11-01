//! Reports of `not-allowlisted` denials to the control plane over `GatewayEvents.ReportDenial`
//! (iter2-egress-approvals design D2): at most once per minute per run, host and port,
//! fire-and-forget with one retry.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_events_client::GatewayEventsClient;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::link::{CallError, GrpcConnector};
use crate::policy::Policy;

/// Minimum time between two reports of the same run, host and port.
pub const REPORT_INTERVAL: Duration = Duration::from_secs(60);
/// Reports waiting for the reporter task; beyond that, new ones are dropped.
pub const QUEUE_CAPACITY: usize = 256;
/// Limit of one connect plus call.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Delivers one report to the control plane.
#[async_trait]
pub trait DenialSink: Send + Sync {
    async fn report(&self, denial: pb::Denial) -> Result<(), CallError>;
}

/// Remembers when each run, host and port was last reported and queues the reports that are due.
pub struct DenialReports {
    last: Mutex<HashMap<(String, String, u16), Instant>>,
    queue: mpsc::Sender<pb::Denial>,
}

impl DenialReports {
    /// The reports and the queue [`send_reports`] drains, holding up to `capacity` reports.
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<pb::Denial>) {
        let (queue, pending) = mpsc::channel(capacity);
        (
            Self {
                last: Mutex::default(),
                queue,
            },
            pending,
        )
    }

    /// Queues a report of a `not-allowlisted` denial of `run_id` unless one was queued for the
    /// same host and port within [`REPORT_INTERVAL`]. A host no allowlist entry can name exactly
    /// (a wildcard, an invalid name) is not reported.
    pub fn denied(&self, run_id: &str, host: &str, port: u16) {
        let host = df_core::host::normalize_host(host);
        if df_core::host::validate_exact_host(&host).as_ref() != Ok(&host) {
            return;
        }
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (run_id.to_string(), host, port);
        if last.get(&key).is_some_and(|at| !expired(*at, now)) {
            return;
        }
        let denial = pb::Denial {
            run_id: key.0.clone(),
            host: key.1.clone(),
            port: u32::from(port),
            at: Some(crate::flow::timestamp_now()),
        };
        match self.queue.try_send(denial) {
            Ok(()) => {
                // Bounded by the report rate: only reports of the last interval stay.
                last.retain(|_, at| !expired(*at, now));
                last.insert(key, now);
            }
            Err(error) => {
                tracing::debug!(%error, run_id, host = %key.1, port, "denial report dropped");
            }
        }
    }

    /// Forgets the runs that are not in `policy` and the reports older than [`REPORT_INTERVAL`].
    pub fn retain_runs(&self, policy: Option<&Policy>) {
        let runs: HashSet<&str> = policy
            .into_iter()
            .flat_map(Policy::sandboxes)
            .map(|sandbox| sandbox.run_id.as_str())
            .collect();
        let now = Instant::now();
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(run_id, _, _), at| runs.contains(run_id.as_str()) && !expired(*at, now));
    }

    #[cfg(test)]
    pub(crate) fn remembered(&self) -> usize {
        self.last.lock().map_or(0, |last| last.len())
    }
}

/// Sends `GatewayEvents.ReportDenial` calls over channels from `connector`.
pub struct GrpcDenialSink {
    connector: Arc<dyn GrpcConnector>,
    control_plane: String,
}

impl GrpcDenialSink {
    /// Reports to `control_plane` (`host:port`).
    pub fn new(connector: Arc<dyn GrpcConnector>, control_plane: String) -> Self {
        Self {
            connector,
            control_plane,
        }
    }
}

#[async_trait]
impl DenialSink for GrpcDenialSink {
    async fn report(&self, denial: pb::Denial) -> Result<(), CallError> {
        let channel = self
            .connector
            .connect(&self.control_plane)
            .await
            .map_err(|error| CallError::Transient(error.to_string()))?;
        GatewayEventsClient::new(channel)
            .report_denial(denial)
            .await?;
        Ok(())
    }
}

/// Sends the queued reports one after the other until `stop` or until the queue closes. A
/// report that fails transiently or takes longer than [`CALL_TIMEOUT`] is retried once, then
/// dropped; a refused one is dropped at once.
pub async fn send_reports(
    mut queue: mpsc::Receiver<pb::Denial>,
    sink: Arc<dyn DenialSink>,
    stop: CancellationToken,
) {
    loop {
        let denial = tokio::select! {
            () = stop.cancelled() => return,
            denial = queue.recv() => match denial {
                Some(denial) => denial,
                None => return,
            },
        };
        let mut error = None;
        for _attempt in 0..2 {
            match tokio::time::timeout(CALL_TIMEOUT, sink.report(denial.clone())).await {
                Ok(Ok(())) => {
                    error = None;
                    break;
                }
                Ok(Err(CallError::Permanent(refused))) => {
                    error = Some(refused);
                    break;
                }
                Ok(Err(CallError::Transient(failed))) => error = Some(failed),
                Err(elapsed) => error = Some(elapsed.to_string()),
            }
        }
        if let Some(error) = error {
            tracing::debug!(%error, run_id = %denial.run_id, host = %denial.host, port = denial.port, "denial report lost");
        }
    }
}

fn expired(at: Instant, now: Instant) -> bool {
    now.duration_since(at) >= REPORT_INTERVAL
}

#[cfg(test)]
mod tests;
