//! `GatewayMcp`: the control plane side of call reports (iter4-mcp-gateway design D9).

use df_db::PgPool;
use df_db::mcp_servers::{McpCallRecord, insert_mcp_call};
use df_db::model_usage::Inserted;
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_mcp_server::GatewayMcp;
use df_proto::time;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use df_gateway::convert;

/// Serves `Report` from the database.
pub struct McpCalls {
    pool: PgPool,
}

impl McpCalls {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl GatewayMcp for McpCalls {
    async fn report(
        &self,
        request: Request<pb::McpCallBatch>,
    ) -> Result<Response<pb::ReportMcpCallsResponse>, Status> {
        let batch = request.into_inner();
        for message in batch.records {
            let Some(record) = stored(&message) else {
                tracing::warn!(gateway = %batch.gateway_pod, call_id = %message.call_id, "MCP call record dropped: invalid");
                continue;
            };
            match insert_mcp_call(&self.pool, &record).await {
                Ok(Inserted::New | Inserted::Duplicate) => {}
                Ok(Inserted::UnknownRun) => {
                    tracing::warn!(run_id = %record.run_id, call_id = %record.call_id, "MCP call record dropped: unknown run");
                }
                Err(error) => {
                    tracing::warn!(%error, "storing MCP call records failed");
                    return Err(Status::unavailable("MCP call records not stored"));
                }
            }
        }
        Ok(Response::new(pb::ReportMcpCallsResponse {}))
    }
}

/// The record to store; `None` if an id or the outcome does not parse.
fn stored(message: &pb::McpCallRecord) -> Option<McpCallRecord> {
    let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
    Some(McpCallRecord {
        call_id: Uuid::parse_str(&message.call_id).ok()?,
        run_id: Uuid::parse_str(&message.run_id).ok()?,
        server_name: message.server_name.clone(),
        tool: message.tool.clone(),
        outcome: convert::mcp_outcome_from_proto(message.outcome)?,
        started_at: message
            .started_at
            .as_ref()
            .and_then(time::from_proto)
            .unwrap_or_else(jiff::Timestamp::now),
        duration_ms: count(message.duration_ms),
        request_bytes: count(message.request_bytes),
        response_bytes: count(message.response_bytes),
    })
}
