//! `GatewayUsage`: the control plane side of usage reports (iter3-inference-gateway design D4).

use std::sync::Arc;

use chrono::DateTime;
use df_db::PgPool;
use df_db::model_usage::{Inserted, UsageRecord, insert_usage};
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_usage_server::GatewayUsage;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::convert;

/// Counts the tokens of stored usage records (`df.agent.tokens`). A trait, so the binary decides
/// which metrics registry it records into.
pub trait TokenCounter: Send + Sync {
    /// `counts` pair a token type (`input`, `output`, `cache_read`, `cache_write`) with its count.
    fn record_tokens(&self, model: &str, counts: [(&'static str, u64); 4]);
}

/// Serves `Report` from the database.
pub struct UsageRecords {
    pool: PgPool,
    tokens: Arc<dyn TokenCounter>,
}

impl UsageRecords {
    pub fn new(pool: PgPool, tokens: Arc<dyn TokenCounter>) -> Self {
        Self { pool, tokens }
    }
}

#[tonic::async_trait]
impl GatewayUsage for UsageRecords {
    async fn report(
        &self,
        request: Request<pb::UsageBatch>,
    ) -> Result<Response<pb::ReportUsageResponse>, Status> {
        let batch = request.into_inner();
        for message in batch.records {
            let Some(record) = stored(&message) else {
                tracing::warn!(gateway = %batch.gateway_pod, request_id = %message.request_id, "usage record dropped: invalid");
                continue;
            };
            match insert_usage(&self.pool, &record).await {
                Ok(Inserted::New) => {
                    // Only new rows count: a retried report must not count twice.
                    if record.usage_known {
                        self.tokens.record_tokens(
                            record.model.as_deref().unwrap_or_default(),
                            [
                                ("input", record.input_tokens),
                                ("output", record.output_tokens),
                                ("cache_read", record.cache_read_tokens),
                                ("cache_write", record.cache_write_tokens),
                            ]
                            .map(|(kind, count)| (kind, u64::try_from(count).unwrap_or(0))),
                        );
                    }
                }
                Ok(Inserted::Duplicate) => {}
                Ok(Inserted::UnknownRun) => {
                    tracing::warn!(run_id = %record.run_id, request_id = %record.request_id, "usage record dropped: unknown run");
                }
                Err(error) => {
                    tracing::warn!(%error, "storing usage records failed");
                    return Err(Status::unavailable("usage records not stored"));
                }
            }
        }
        Ok(Response::new(pb::ReportUsageResponse {}))
    }
}

/// The record to store; `None` if an id or the wire API does not parse.
fn stored(message: &pb::UsageRecord) -> Option<UsageRecord> {
    let wire_api = convert::wire_api_from_proto(message.wire_api)?;
    let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
    let at = message.at.as_ref();
    Some(UsageRecord {
        request_id: Uuid::parse_str(&message.request_id).ok()?,
        run_id: Uuid::parse_str(&message.run_id).ok()?,
        recorded_at: at
            .and_then(|at| DateTime::from_timestamp(at.seconds, u32::try_from(at.nanos).ok()?))
            .unwrap_or_else(chrono::Utc::now),
        model: Some(message.model.clone()).filter(|model| !model.is_empty()),
        wire_api,
        upstream_status: Some(message.upstream_status)
            .filter(|status| *status != 0)
            .and_then(|status| i32::try_from(status).ok()),
        usage_known: message.usage_known,
        input_tokens: count(message.input_tokens),
        output_tokens: count(message.output_tokens),
        cache_read_tokens: count(message.cache_read_tokens),
        cache_write_tokens: count(message.cache_write_tokens),
    })
}

#[cfg(test)]
mod tests;
