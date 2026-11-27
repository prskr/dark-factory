//! Call records on their way to the control plane (iter4-mcp-gateway design D9), sent with
//! `GatewayMcp.Report` by the shared [`BatchReporter`], like the usage records of the inference
//! endpoint.

use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_mcp_client::GatewayMcpClient;
use tonic::transport::Channel;

use crate::batch::{Batch, BatchReporter};
use crate::metrics::Metrics;

/// Where finished calls put their record.
pub trait CallSink: Send + Sync {
    fn record(&self, record: pb::McpCallRecord);
}

/// The call records of the MCP endpoint.
pub struct Calls;

impl Batch for Calls {
    type Record = pb::McpCallRecord;
    const NAME: &'static str = "MCP call";

    async fn send(
        channel: Channel,
        pod: String,
        records: Vec<pb::McpCallRecord>,
    ) -> Result<(), tonic::Status> {
        GatewayMcpClient::new(channel)
            .report(pb::McpCallBatch {
                gateway_pod: pod,
                records,
            })
            .await
            .map(|_response| ())
    }

    fn dropped(metrics: &Metrics, count: u64) {
        metrics.mcp_calls_dropped(count);
    }
}

/// Buffers call records and reports them to the control plane.
pub type CallReporter = BatchReporter<Calls>;

impl CallSink for CallReporter {
    fn record(&self, record: pb::McpCallRecord) {
        self.push(record);
    }
}

#[cfg(test)]
mod tests;
