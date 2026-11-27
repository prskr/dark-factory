//! Call records of the MCP endpoint: one row per `tools/call`, or per server left out of a
//! `tools/list`. The call id makes a retried report idempotent.

use std::collections::HashMap;

use df_core::mcp::McpCallOutcome;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::model_usage::Inserted;
use crate::pool::DbError;

/// A call record as stored. Arguments and results are never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallRecord {
    /// Chosen by the gateway (`UUIDv7`).
    pub call_id: Uuid,
    pub run_id: Uuid,
    pub server_name: String,
    /// The upstream tool name, or `tools/list` for a server left out of a list.
    pub tool: String,
    pub outcome: McpCallOutcome,
    pub started_at: Timestamp,
    pub duration_ms: i64,
    pub request_bytes: i64,
    pub response_bytes: i64,
}

#[derive(FromRow)]
struct CallRow {
    call_id: Uuid,
    run_id: Uuid,
    server_name: String,
    tool: String,
    outcome: String,
    started_at: jiff_sqlx::Timestamp,
    duration_ms: i64,
    request_bytes: i64,
    response_bytes: i64,
}

impl TryFrom<CallRow> for McpCallRecord {
    type Error = DbError;

    fn try_from(row: CallRow) -> Result<Self, DbError> {
        Ok(Self {
            call_id: row.call_id,
            run_id: row.run_id,
            server_name: row.server_name,
            tool: row.tool,
            outcome: row.outcome.parse()?,
            started_at: row.started_at.to_jiff(),
            duration_ms: row.duration_ms,
            request_bytes: row.request_bytes,
            response_bytes: row.response_bytes,
        })
    }
}

/// Stores `record` unless its call id is already stored. A record is stored for a run in any
/// state: records may arrive after the run ended.
pub async fn insert_mcp_call(pool: &PgPool, record: &McpCallRecord) -> Result<Inserted, DbError> {
    let inserted = sqlx::query(
        "INSERT INTO mcp_calls (call_id, run_id, server_name, tool, outcome, started_at, \
            duration_ms, request_bytes, response_bytes) \
         SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9 \
         WHERE EXISTS (SELECT 1 FROM runs WHERE id = $2) \
         ON CONFLICT (call_id) DO NOTHING",
    )
    .bind(record.call_id)
    .bind(record.run_id)
    .bind(&record.server_name)
    .bind(&record.tool)
    .bind(record.outcome.as_str())
    .bind(record.started_at.to_sqlx())
    .bind(record.duration_ms)
    .bind(record.request_bytes)
    .bind(record.response_bytes)
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

/// The call records of each of `run_ids`, oldest first; runs without records are left out.
pub async fn list_mcp_calls_of_runs(
    pool: &PgPool,
    run_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<McpCallRecord>>, DbError> {
    let rows: Vec<CallRow> = sqlx::query_as(
        "SELECT call_id, run_id, server_name, tool, outcome, started_at, duration_ms, \
            request_bytes, response_bytes \
         FROM mcp_calls WHERE run_id = ANY($1) ORDER BY started_at, call_id",
    )
    .bind(run_ids)
    .fetch_all(pool)
    .await?;
    let mut calls: HashMap<Uuid, Vec<McpCallRecord>> = HashMap::new();
    for row in rows {
        let record = McpCallRecord::try_from(row)?;
        calls.entry(record.run_id).or_default().push(record);
    }
    Ok(calls)
}
