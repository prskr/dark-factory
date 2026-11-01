//! Push records sent to the control plane over `GatewayGit.RecordPush` (design D6).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_git_client::GatewayGitClient;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::push::{PushRecord, PushRecorder, RecordError};
use crate::link::{CallError, GrpcConnector};

/// Records waiting for a retry after their first attempt window; beyond that, newer records are
/// dropped with a warning.
pub const QUEUE_CAPACITY: usize = 1000;

/// How long and how often a record is retried.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total time for one record before the push is answered without it.
    pub budget: Duration,
    /// Limit of one connect plus call.
    pub attempt_timeout: Duration,
    /// First pause between attempts, doubled up to `max_backoff`.
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            budget: Duration::from_secs(5),
            attempt_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
        }
    }
}

/// Sends push records to the control plane. `FORWARDING` and `REJECTED` records fail after the
/// retry budget; final verdicts that fail go to a background queue that retries them forever.
pub struct GrpcRecorder {
    sender: Arc<Sender>,
    queue: mpsc::Sender<pb::PushRecord>,
    stop: CancellationToken,
    retries: Mutex<Option<JoinHandle<()>>>,
}

impl GrpcRecorder {
    /// Records to `control_plane` (`host:port`) as gateway pod `pod`. Spawns the retry task,
    /// which [`GrpcRecorder::shutdown`] drains and dropping the recorder aborts.
    pub fn new(
        connector: Arc<dyn GrpcConnector>,
        control_plane: String,
        pod: String,
        policy: RetryPolicy,
    ) -> Self {
        let sender = Arc::new(Sender {
            connector,
            control_plane,
            pod,
            policy,
        });
        let (queue, pending) = mpsc::channel(QUEUE_CAPACITY);
        let stop = CancellationToken::new();
        let retries = tokio::spawn(retry_queued(sender.clone(), pending, stop.clone()));
        Self {
            sender,
            queue,
            stop,
            retries: Mutex::new(Some(retries)),
        }
    }

    /// Lets the retry task store the queued records for up to `grace`, then aborts it.
    pub async fn shutdown(&self, grace: Duration) {
        self.stop.cancel();
        let Some(mut retries) = self.retries.lock().ok().and_then(|mut task| task.take()) else {
            return;
        };
        if tokio::time::timeout(grace, &mut retries).await.is_err() {
            tracing::warn!("queued push records not stored within the shutdown grace period");
            retries.abort();
        }
    }
}

impl Drop for GrpcRecorder {
    fn drop(&mut self) {
        if let Ok(Some(retries)) = self.retries.get_mut().map(Option::take) {
            retries.abort();
        }
    }
}

#[async_trait]
impl PushRecorder for GrpcRecorder {
    async fn record(&self, record: PushRecord) -> Result<(), RecordError> {
        let verdict = record.verdict;
        let message = self.sender.message(record);
        let error = match self.sender.send_within_budget(&message).await {
            Ok(()) => return Ok(()),
            Err(Failure::Permanent(error)) => return Err(error),
            Err(Failure::Transient(error)) => error,
        };
        if matches!(
            verdict,
            pb::PushVerdict::Accepted | pb::PushVerdict::UpstreamFailed
        ) {
            tracing::warn!(%error, push_id = %message.push_id, "push record queued for retry");
            // The record of an already forwarded push must not hold up the client's answer.
            if self.queue.try_send(message).is_err() {
                tracing::warn!("push record retry queue full; record dropped");
            }
            return Ok(());
        }
        Err(error)
    }
}

/// What every attempt needs.
struct Sender {
    connector: Arc<dyn GrpcConnector>,
    control_plane: String,
    pod: String,
    policy: RetryPolicy,
}

impl Sender {
    fn message(&self, record: PushRecord) -> pb::PushRecord {
        pb::PushRecord {
            push_id: record.push_id.to_string(),
            run_id: record.run_id,
            gateway_pod: self.pod.clone(),
            refs: record.refs,
            verdict: record.verdict.into(),
            bytes: record.bytes,
            duration: Some(crate::flow::duration_proto(record.duration)),
        }
    }

    /// Retries `message` with backoff until it is stored, the budget is spent or the control
    /// plane refuses it for good.
    async fn send_within_budget(&self, message: &pb::PushRecord) -> Result<(), Failure> {
        let deadline = Instant::now() + self.policy.budget;
        let mut backoff = self.policy.initial_backoff;
        loop {
            let limit = self
                .policy
                .attempt_timeout
                .min(deadline.saturating_duration_since(Instant::now()));
            let error = match self.attempt(message, limit).await {
                Ok(()) => return Ok(()),
                Err(Failure::Permanent(error)) => return Err(Failure::Permanent(error)),
                Err(Failure::Transient(error)) => error,
            };
            if Instant::now() + backoff >= deadline {
                return Err(Failure::Transient(error));
            }
            tracing::debug!(%error, push_id = %message.push_id, "push record attempt failed");
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(self.policy.max_backoff);
        }
    }

    /// One connect plus call, cut off after `limit`.
    async fn attempt(&self, message: &pb::PushRecord, limit: Duration) -> Result<(), Failure> {
        let call = async {
            let channel = self
                .connector
                .connect(&self.control_plane)
                .await
                .map_err(|error| Failure::Transient(RecordError(error.to_string())))?;
            GatewayGitClient::new(channel)
                .record_push(message.clone())
                .await
                .map_err(|status| Failure::from(CallError::from(status)))?;
            Ok(())
        };
        tokio::time::timeout(limit, call)
            .await
            .map_err(|elapsed| Failure::Transient(RecordError(format!("record push: {elapsed}"))))?
    }
}

/// Why a record was not stored: worth another attempt, or refused for good.
enum Failure {
    Transient(RecordError),
    Permanent(RecordError),
}

impl From<CallError> for Failure {
    fn from(error: CallError) -> Self {
        match error {
            CallError::Transient(error) => Self::Transient(RecordError(error)),
            CallError::Permanent(error) => Self::Permanent(RecordError(error)),
        }
    }
}

/// Retries queued records one after the other until each is stored. After `stop` it stores what
/// is still queued and ends.
async fn retry_queued(
    sender: Arc<Sender>,
    mut pending: mpsc::Receiver<pb::PushRecord>,
    stop: CancellationToken,
) {
    loop {
        let next = tokio::select! {
            biased;
            message = pending.recv() => message,
            () = stop.cancelled() => pending.try_recv().ok(),
        };
        let Some(message) = next else { return };
        loop {
            match sender.send_within_budget(&message).await {
                Ok(()) => break,
                Err(Failure::Permanent(error)) => {
                    tracing::warn!(%error, push_id = %message.push_id, "queued push record refused; dropped");
                    break;
                }
                Err(Failure::Transient(error)) => {
                    tracing::warn!(%error, push_id = %message.push_id, "queued push record still not stored");
                    tokio::time::sleep(sender.policy.max_backoff).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
