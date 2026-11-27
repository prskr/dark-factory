//! Call records: what the endpoint reports for a call or a server left out of a list.

use std::sync::Arc;
use std::time::Instant;

use df_core::mcp::McpCallOutcome;
use df_proto::gateway::v1 as pb;
use df_proto::pbjson_types::Timestamp;

use super::report::CallSink;
use crate::convert::mcp_outcome_to_proto;

/// The `tool` of the record of a server left out of a `tools/list`.
pub(super) const LIST_TOOL: &str = "tools/list";

/// A call in progress. If it is dropped before [`Pending::finish`], the client went away and the
/// call is recorded as cancelled.
pub(super) struct Pending {
    sink: Arc<dyn CallSink>,
    run_id: String,
    server: String,
    tool: String,
    started_at: Timestamp,
    started: Instant,
    request_bytes: u64,
    done: bool,
}

impl Pending {
    pub(super) fn start(
        sink: &Arc<dyn CallSink>,
        run_id: &str,
        server: &str,
        tool: &str,
        request_bytes: u64,
    ) -> Self {
        Self {
            sink: sink.clone(),
            run_id: run_id.to_owned(),
            server: server.to_owned(),
            tool: tool.to_owned(),
            started_at: crate::flow::timestamp_now(),
            started: Instant::now(),
            request_bytes,
            done: false,
        }
    }

    /// Reports the call with `outcome` and the size of the answer sent to the client.
    pub(super) fn finish(mut self, outcome: McpCallOutcome, response_bytes: u64) {
        self.report(outcome, response_bytes);
    }

    /// Drops the call without a record.
    pub(super) fn discard(mut self) {
        self.done = true;
    }

    fn report(&mut self, outcome: McpCallOutcome, response_bytes: u64) {
        if self.done {
            return;
        }
        self.done = true;
        tracing::info!(
            run_id = %self.run_id,
            server = %self.server,
            tool = %self.tool,
            outcome = outcome.as_str(),
            "MCP call finished"
        );
        self.sink.record(pb::McpCallRecord {
            call_id: uuid::Uuid::now_v7().to_string(),
            run_id: self.run_id.clone(),
            started_at: Some(self.started_at),
            server_name: self.server.clone(),
            tool: self.tool.clone(),
            outcome: mcp_outcome_to_proto(outcome).into(),
            duration_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            request_bytes: self.request_bytes,
            response_bytes,
        });
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.report(McpCallOutcome::Cancelled, 0);
    }
}
