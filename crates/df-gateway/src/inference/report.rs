//! Usage records on their way to the control plane (iter3-inference-gateway design D4): a bounded
//! buffer that a background task sends in batches with `GatewayUsage.Report` and retries with
//! backoff.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_usage_client::GatewayUsageClient;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::link::{CallError, GrpcConnector};
use crate::metrics::Metrics;

/// Records kept while the control plane is unreachable; beyond that the oldest are dropped.
pub const BUFFER_CAPACITY: usize = 10_000;
/// A batch is sent as soon as this many records wait.
pub const BATCH_SIZE: usize = 100;
/// Otherwise a batch is sent this often.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
/// Limit of one connect plus call.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Where finished requests put their usage record.
pub trait UsageSink: Send + Sync {
    fn record(&self, record: pb::UsageRecord);
}

/// How often and how long the reporter waits; the defaults are the constants above.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub flush_interval: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            flush_interval: FLUSH_INTERVAL,
            initial_backoff: INITIAL_BACKOFF,
            max_backoff: MAX_BACKOFF,
        }
    }
}

struct Shared {
    queue: Mutex<VecDeque<pb::UsageRecord>>,
    capacity: usize,
    wake: Notify,
    metrics: Arc<Metrics>,
}

impl Shared {
    fn push(&self, record: pb::UsageRecord) {
        let mut dropped = 0_u64;
        let full = {
            let Ok(mut queue) = self.queue.lock() else {
                return;
            };
            while queue.len() >= self.capacity {
                queue.pop_front();
                dropped += 1;
            }
            queue.push_back(record);
            queue.len() >= BATCH_SIZE
        };
        if dropped > 0 {
            self.metrics.usage_dropped(dropped);
        }
        if full {
            self.wake.notify_one();
        }
    }

    /// Up to [`BATCH_SIZE`] records, oldest first, removed from the queue.
    fn take_batch(&self) -> Vec<pb::UsageRecord> {
        let Ok(mut queue) = self.queue.lock() else {
            return Vec::new();
        };
        let count = queue.len().min(BATCH_SIZE);
        queue.drain(..count).collect()
    }

    /// Puts an unsent batch back in front of newer records, as far as the buffer has room.
    fn requeue(&self, batch: Vec<pb::UsageRecord>) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let room = self.capacity.saturating_sub(queue.len());
        let lost = batch.len().saturating_sub(room);
        for record in batch.into_iter().skip(lost).rev() {
            queue.push_front(record);
        }
        drop(queue);
        if lost > 0 {
            self.metrics.usage_dropped(lost as u64);
        }
    }

    fn is_empty(&self) -> bool {
        self.queue.lock().map_or(true, |queue| queue.is_empty())
    }
}

/// Buffers usage records and reports them to the control plane. Dropping it aborts the sender;
/// [`UsageReporter::shutdown`] sends what is left first.
pub struct UsageReporter {
    shared: Arc<Shared>,
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl UsageReporter {
    /// Reports to `control_plane` (`host:port`) as gateway pod `pod`. Spawns the sender task.
    pub fn new(
        connector: Arc<dyn GrpcConnector>,
        control_plane: String,
        pod: String,
        metrics: Arc<Metrics>,
        timing: Timing,
    ) -> Self {
        Self::with_capacity(
            connector,
            control_plane,
            pod,
            metrics,
            timing,
            BUFFER_CAPACITY,
        )
    }

    fn with_capacity(
        connector: Arc<dyn GrpcConnector>,
        control_plane: String,
        pod: String,
        metrics: Arc<Metrics>,
        timing: Timing,
        capacity: usize,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            capacity,
            wake: Notify::new(),
            metrics,
        });
        let stop = CancellationToken::new();
        let sender = Sender {
            shared: shared.clone(),
            connector,
            control_plane,
            pod,
            timing,
        };
        let task = tokio::spawn(sender.run(stop.clone()));
        Self {
            shared,
            stop,
            task: Mutex::new(Some(task)),
        }
    }

    /// Like [`UsageReporter::new`] with a buffer of `capacity` records.
    #[cfg(test)]
    pub(crate) fn for_test_at(
        connector: Arc<dyn GrpcConnector>,
        control_plane: String,
        metrics: Arc<Metrics>,
        timing: Timing,
        capacity: usize,
    ) -> Self {
        Self::with_capacity(
            connector,
            control_plane,
            "gateway-0".into(),
            metrics,
            timing,
            capacity,
        )
    }

    /// Lets the sender report the buffered records for up to `grace`, then aborts it.
    pub async fn shutdown(&self, grace: Duration) {
        self.stop.cancel();
        let Some(mut task) = self.task.lock().ok().and_then(|mut task| task.take()) else {
            return;
        };
        if tokio::time::timeout(grace, &mut task).await.is_err() {
            tracing::warn!("buffered usage records not reported within the shutdown grace period");
            task.abort();
        }
    }
}

impl Drop for UsageReporter {
    fn drop(&mut self) {
        if let Ok(Some(task)) = self.task.get_mut().map(Option::take) {
            task.abort();
        }
    }
}

impl UsageSink for UsageReporter {
    fn record(&self, record: pb::UsageRecord) {
        self.shared.push(record);
    }
}

struct Sender {
    shared: Arc<Shared>,
    connector: Arc<dyn GrpcConnector>,
    control_plane: String,
    pod: String,
    timing: Timing,
}

impl Sender {
    async fn run(self, stop: CancellationToken) {
        let mut backoff = self.timing.initial_backoff;
        loop {
            let stopping = stop.is_cancelled();
            // Registered before the buffer is looked at: a record pushed in between wakes us.
            let woken = self.shared.wake.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            let batch = self.shared.take_batch();
            if batch.is_empty() {
                if stopping {
                    return;
                }
                tokio::select! {
                    () = &mut woken => {}
                    () = tokio::time::sleep(self.timing.flush_interval) => {}
                    () = stop.cancelled() => {}
                }
                continue;
            }
            match self.send(&batch).await {
                Ok(()) => backoff = self.timing.initial_backoff,
                Err(CallError::Permanent(error)) => {
                    tracing::warn!(%error, records = batch.len(), "usage records refused by the control plane; dropped");
                    self.shared.metrics.usage_dropped(batch.len() as u64);
                }
                Err(CallError::Transient(error)) => {
                    tracing::warn!(%error, records = batch.len(), "usage report failed; retrying");
                    self.shared.requeue(batch);
                    if stopping {
                        return;
                    }
                    tokio::select! {
                        () = tokio::time::sleep(backoff) => {}
                        () = stop.cancelled() => {}
                    }
                    backoff = (backoff * 2).min(self.timing.max_backoff);
                }
            }
            if stopping && self.shared.is_empty() {
                return;
            }
            // More than a batch waits: send on without waiting for the interval.
        }
    }

    async fn send(&self, batch: &[pb::UsageRecord]) -> Result<(), CallError> {
        let call = async {
            let channel = self
                .connector
                .connect(&self.control_plane)
                .await
                .map_err(|error| CallError::Transient(error.to_string()))?;
            GatewayUsageClient::new(channel)
                .report(pb::UsageBatch {
                    gateway_pod: self.pod.clone(),
                    records: batch.to_vec(),
                })
                .await
                .map_err(CallError::from)?;
            Ok(())
        };
        tokio::time::timeout(ATTEMPT_TIMEOUT, call)
            .await
            .map_err(|elapsed| CallError::Transient(format!("report usage: {elapsed}")))?
    }
}

#[cfg(test)]
mod tests;
