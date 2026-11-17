//! Exactly-once bookkeeping of factory tool calls: a call is claimed by its `call_id` before it
//! runs and its result is stored once, so a re-sent call is answered, not executed again
//! (`specs/factory-tools/spec.md`).

use df_core::RunId;
use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::pool::DbError;

/// A claimed call as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCall {
    /// The run that claimed the call.
    pub run_id: RunId,
    /// The result, `None` while the call runs or if its executor died.
    pub result: Option<Value>,
    /// The claim is older than five minutes: an unanswered one was abandoned.
    pub abandoned: bool,
}

/// Claims `call_id` for `run_id`. Returns `true` for the first claim, `false` for a repeat.
pub async fn claim(
    executor: impl PgExecutor<'_>,
    call_id: Uuid,
    run_id: RunId,
    tool_name: &str,
    arguments: &Value,
) -> Result<bool, DbError> {
    let claimed = sqlx::query!(
        r#"
        INSERT INTO tool_calls (call_id, run_id, tool_name, arguments)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (call_id) DO NOTHING
        "#,
        call_id,
        run_id.as_uuid(),
        tool_name,
        arguments
    )
    .execute(executor)
    .await?;
    Ok(claimed.rows_affected() == 1)
}

/// Stores `result` for the call unless a result is already there: the first one is kept.
pub async fn store_result(
    executor: impl PgExecutor<'_>,
    call_id: Uuid,
    result: &Value,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE tool_calls SET result = $2, completed_at = now()
         WHERE call_id = $1 AND result IS NULL",
        call_id,
        result
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Reads the claimed call; an error if `call_id` was never claimed.
pub async fn get(executor: impl PgExecutor<'_>, call_id: Uuid) -> Result<StoredCall, DbError> {
    let row = sqlx::query!(
        r#"
        SELECT run_id, result,
               created_at < now() - interval '5 minutes' AS "abandoned!"
        FROM tool_calls WHERE call_id = $1
        "#,
        call_id
    )
    .fetch_one(executor)
    .await?;
    Ok(StoredCall {
        run_id: RunId::from_uuid(row.run_id),
        result: row.result,
        abandoned: row.abandoned,
    })
}

#[cfg(test)]
mod tests;
