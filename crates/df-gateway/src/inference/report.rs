//! Usage records on their way to the control plane (iter3-inference-gateway design D4), sent with
//! `GatewayUsage.Report` by the shared [`BatchReporter`].

use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_usage_client::GatewayUsageClient;
use tonic::transport::Channel;

use crate::batch::Batch;
pub use crate::batch::{BATCH_SIZE, BUFFER_CAPACITY, BatchReporter, FLUSH_INTERVAL, Timing};
use crate::metrics::Metrics;

/// Where finished requests put their usage record.
pub trait UsageSink: Send + Sync {
    fn record(&self, record: pb::UsageRecord);
}

/// The usage records of the inference endpoint.
pub struct Usage;

impl Batch for Usage {
    type Record = pb::UsageRecord;
    const NAME: &'static str = "usage";

    async fn send(
        channel: Channel,
        pod: String,
        records: Vec<pb::UsageRecord>,
    ) -> Result<(), tonic::Status> {
        GatewayUsageClient::new(channel)
            .report(pb::UsageBatch {
                gateway_pod: pod,
                records,
            })
            .await
            .map(|_response| ())
    }

    fn dropped(metrics: &Metrics, count: u64) {
        metrics.usage_dropped(count);
    }
}

/// Buffers usage records and reports them to the control plane.
pub type UsageReporter = BatchReporter<Usage>;

impl UsageSink for UsageReporter {
    fn record(&self, record: pb::UsageRecord) {
        self.push(record);
    }
}

#[cfg(test)]
mod tests;
