//! Generic transactional outbox (`lifecycle-events` design D1).
//!
//! A caller enqueues a job of some `kind` inside the transaction of the state change that causes
//! it, so the job exists exactly when the change commits. A dispatcher on the leader claims due
//! jobs and runs the handler registered for their kind.

mod dispatcher;

use df_core::TaskId;
use sqlx::{PgConnection, PgExecutor};

pub use self::dispatcher::{
    CLEANUP_INTERVAL, Dispatcher, JobError, JobResult, MAX_IN_FLIGHT, OutboxHandler, POLL_INTERVAL,
    RETAIN_FINISHED, RetryPolicy,
};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::tasks::PR_SYNC_KIND;

/// Enqueues one job of `kind` and notifies `outbox` when the transaction commits.
pub async fn enqueue(
    conn: &mut PgConnection,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO outbox (kind, payload) VALUES ($1, $2)",
        kind,
        payload
    )
    .execute(&mut *conn)
    .await?;
    publish(&mut *conn, &Notification::Outbox).await
}

/// Whether a `pr.sync` job of the task that opens the pull request has not finished.
pub async fn has_unfinished_pr_open_job(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM outbox
            WHERE kind = $1 AND payload ->> 'task_id' = $2
              AND (payload ->> 'open')::boolean
              AND done_at IS NULL AND gave_up_at IS NULL) AS "pending!"
        "#,
        PR_SYNC_KIND,
        task_id.to_string()
    )
    .fetch_one(executor)
    .await?)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use sqlx::PgPool;

    use super::*;
    use crate::notify::Notifier;

    async fn jobs(pool: &PgPool) -> Vec<(String, serde_json::Value)> {
        sqlx::query_as("SELECT kind, payload FROM outbox ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn enqueue_writes_the_job_and_notifies_on_commit() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let notifier = Notifier::start(&pool).await.unwrap();
        let mut rx = notifier.subscribe();

        let mut tx = pool.begin().await.unwrap();
        enqueue(&mut tx, "test.kind", &json!({"n": 1}))
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(jobs(&pool).await, [("test.kind".into(), json!({"n": 1}))]);
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, Notification::Outbox);
    }

    #[tokio::test]
    async fn a_rolled_back_transaction_leaves_no_job() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

        let mut tx = pool.begin().await.unwrap();
        enqueue(&mut tx, "test.kind", &json!({})).await.unwrap();
        tx.rollback().await.unwrap();

        assert!(jobs(&pool).await.is_empty());
    }
}
