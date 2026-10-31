//! The database of the instance, read by the tests for what the UI does not show.

use anyhow::{Context, Result};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Executor, Row};

/// Schema of the control plane tables unless `E2E_DATABASE_SCHEMA` says otherwise.
pub const DEFAULT_SCHEMA: &str = "dark_factory";

/// Connects to `url` with the search path set to the control plane schema.
pub async fn connect(url: &str) -> Result<PgPool> {
    let schema =
        std::env::var("E2E_DATABASE_SCHEMA").unwrap_or_else(|_unset| DEFAULT_SCHEMA.into());
    let schema = df_db::validate_sql_identifier(&schema)?.to_owned();
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |conn, _meta| {
            let query = format!("SET search_path TO \"{schema}\"");
            Box::pin(async move {
                // `schema` was validated by `validate_sql_identifier`.
                conn.execute(sqlx::AssertSqlSafe(query)).await?;
                Ok(())
            })
        })
        .connect(url)
        .await
        .context("connect to the database")
}

/// A run of a task as the tests compare them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub number: i64,
    pub state: String,
    pub outcome: Option<String>,
    /// Whether the run carries the time to retry after (rate limit).
    pub has_retry_after: bool,
    pub harness_session_id: String,
}

/// Status of task `key` (`queued`, `running`, `waiting`, `completed`, `rate_limited`, ...).
pub async fn task_status(pool: &PgPool, key: &str) -> Result<String> {
    sqlx::query_scalar::<_, String>("SELECT status FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("status of task {key}"))
}

/// The pull request number and URL linked to task `key`.
pub async fn task_pull_request(pool: &PgPool, key: &str) -> Result<Option<(i64, String)>> {
    let row = sqlx::query("SELECT pr_number, pr_url FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("pull request of task {key}"))?;
    let number: Option<i64> = row.try_get("pr_number")?;
    let url: Option<String> = row.try_get("pr_url")?;
    Ok(number.zip(url))
}

/// The runs of task `key`, oldest first.
pub async fn runs(pool: &PgPool, key: &str) -> Result<Vec<RunRow>> {
    let rows = sqlx::query(
        "SELECT r.number, r.state, r.outcome, r.retry_after IS NOT NULL AS has_retry_after, \
         r.harness_session_id FROM runs r \
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 ORDER BY r.number",
    )
    .bind(key)
    .fetch_all(pool)
    .await
    .with_context(|| format!("runs of task {key}"))?;
    rows.iter()
        .map(|row| {
            Ok(RunRow {
                number: row.try_get("number")?,
                state: row.try_get("state")?,
                outcome: row.try_get("outcome")?,
                has_retry_after: row.try_get("has_retry_after")?,
                harness_session_id: row.try_get("harness_session_id")?,
            })
        })
        .collect()
}
