//! Updates of task fields that the agent channel makes: the pull request draft, the status line,
//! the divergence flag and the recorded pull request, and the per-task lock of the PR forge
//! handlers. Status changes are not here: they go through `apply_task_event`.

use df_core::TaskId;
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};

use crate::notify::{Notification, publish};
use crate::pool::DbError;

/// Saves the draft title and body of the task's pull request (`None` keeps the stored value) and
/// returns the number of the pull request if it exists already. The caller decides what to do
/// with an existing one; the update does not publish.
pub async fn set_pr_draft_fields(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<Option<i64>, DbError> {
    Ok(sqlx::query_scalar!(
        r#"
        UPDATE tasks SET pr_title = COALESCE($2, pr_title), pr_body = COALESCE($3, pr_body),
            updated_at = now()
        WHERE id = $1
        RETURNING pr_number
        "#,
        task_id.as_uuid(),
        title,
        body
    )
    .fetch_one(executor)
    .await?)
}

/// Stores the status line of the task and publishes `task_changed`.
pub async fn set_status_line(pool: &PgPool, task_id: TaskId, line: &str) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "UPDATE tasks SET status_line = $2, updated_at = now() WHERE id = $1",
        task_id.as_uuid(),
        line
    )
    .execute(&mut *tx)
    .await?;
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
    Ok(())
}

/// Flags the task as diverged (its branch was rejected as a non-fast-forward push) and publishes
/// `task_changed`.
pub async fn mark_diverged(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE tasks SET diverged = TRUE, updated_at = now() WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await
}

/// Records the pull request `number` at `url` on the task unless it has one already, and
/// publishes `task_changed` when it did. Returns whether this call recorded it; `false` means
/// another one was recorded first and stays.
pub async fn record_pr(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    number: u64,
    url: &str,
    attached: bool,
) -> Result<bool, DbError> {
    let number = i64::try_from(number)
        .map_err(|_out_of_range| DbError::InvalidData("pull request number out of range".into()))?;
    let updated = sqlx::query!(
        r#"
        UPDATE tasks SET pr_number = $2, pr_url = $3, pr_attached = $4, updated_at = now()
        WHERE id = $1 AND pr_number IS NULL
        "#,
        task_id.as_uuid(),
        number,
        url,
        attached
    )
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
    Ok(true)
}

/// Takes the per-task lock of the PR forge handlers (`task-pr:<id>`), also across control-plane
/// replicas. It is released when the returned transaction ends, on every path including a
/// dropped future.
///
/// ponytail: only outbox jobs take it, and the dispatcher runs at most `MAX_IN_FLIGHT` (8) of
/// them, so at most 8 pooled connections wait or hold it across forge calls (design D5). Add a
/// separate lock connection pool if the dispatcher's concurrency grows.
pub async fn lock_task_pr(
    pool: &PgPool,
    task_id: TaskId,
) -> Result<Transaction<'static, Postgres>, DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        format!("task-pr:{task_id}")
    )
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}

#[cfg(test)]
mod tests;
