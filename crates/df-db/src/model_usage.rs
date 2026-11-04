//! Token usage per model request, as reported by the gateways (iter3-inference-gateway design D4,
//! D5). One row per request; the request id makes a retried report idempotent.

use chrono::{DateTime, Utc};
use df_core::model_provider::WireApi;
use sqlx::PgPool;
use uuid::Uuid;

use crate::pool::DbError;

/// A usage record as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    /// Chosen by the gateway (`UUIDv7`).
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub recorded_at: DateTime<Utc>,
    /// The model named in the response; `None` if unknown.
    pub model: Option<String>,
    pub wire_api: WireApi,
    /// Status of the upstream response; `None` if there was none.
    pub upstream_status: Option<i32>,
    /// False if the response carried no usage; the counts are then 0.
    pub usage_known: bool,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// What storing a record did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    /// The record is new.
    New,
    /// A record with this request id was stored before.
    Duplicate,
    /// The run does not exist; nothing was stored.
    UnknownRun,
}

/// Stores `record` unless its request id is already stored. A record is stored for a run in any
/// state: usage may arrive after the run ended.
pub async fn insert_usage(pool: &PgPool, record: &UsageRecord) -> Result<Inserted, DbError> {
    let inserted = sqlx::query(
        "INSERT INTO model_usage (request_id, run_id, recorded_at, model, wire_api, upstream_status, \
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usage_known) \
         SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11 \
         WHERE EXISTS (SELECT 1 FROM runs WHERE id = $2) \
         ON CONFLICT (request_id) DO NOTHING",
    )
    .bind(record.request_id)
    .bind(record.run_id)
    .bind(record.recorded_at)
    .bind(&record.model)
    .bind(record.wire_api.as_str())
    .bind(record.upstream_status)
    .bind(record.input_tokens)
    .bind(record.output_tokens)
    .bind(record.cache_read_tokens)
    .bind(record.cache_write_tokens)
    .bind(record.usage_known)
    .execute(pool)
    .await?
    .rows_affected();
    if inserted > 0 {
        return Ok(Inserted::New);
    }
    let run_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM runs WHERE id = $1)")
        .bind(record.run_id)
        .fetch_one(pool)
        .await?;
    Ok(if run_exists {
        Inserted::Duplicate
    } else {
        Inserted::UnknownRun
    })
}

#[cfg(test)]
mod tests;
