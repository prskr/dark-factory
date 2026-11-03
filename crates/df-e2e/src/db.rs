//! The database of the instance, read by the tests for what the UI does not show.

use anyhow::{Context, Result};
use df_db::git_pushes::RefUpdate;
use df_db::push_checks::{ExceptionFinding, PUSH_CHECK_EXCEPTION, PUSH_REJECTED, request_findings};
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Executor, Row};
use uuid::Uuid;

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

/// Number of messages of task `key` whose content contains `text`.
pub async fn messages_containing(pool: &PgPool, key: &str, text: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM task_messages m JOIN tasks t ON t.id = m.task_id \
         WHERE t.key = $1 AND strpos(m.content::text, $2) > 0",
    )
    .bind(key)
    .bind(text)
    .fetch_one(pool)
    .await
    .with_context(|| format!("messages of task {key}"))
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

/// A `network-access` request as the tests compare them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRequest {
    pub id: Uuid,
    pub state: String,
    /// Raised by the control plane (no run), not by the agent.
    pub raised_by_control_plane: bool,
    pub blocking: bool,
    pub reason: String,
    /// The scope of the approval.
    pub scope: Option<String>,
    /// Why a closed request was closed (`task-cancelled`, ...).
    pub close_reason: Option<String>,
}

/// The newest `network-access` request of task `key` for `host:port`.
pub async fn network_request(
    pool: &PgPool,
    key: &str,
    host: &str,
    port: u16,
) -> Result<Option<NetworkRequest>> {
    let row = sqlx::query(
        "SELECT r.id, r.state, r.run_id IS NULL AS raised_by_control_plane, r.blocking, r.reason, \
         r.decision ->> 'scope' AS scope, r.close_reason FROM agent_requests r \
         JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1 AND r.approval_type = 'network-access' AND r.fingerprint = $2 \
         ORDER BY r.created_at DESC LIMIT 1",
    )
    .bind(key)
    .bind(format!("{host}:{port}"))
    .fetch_optional(pool)
    .await
    .with_context(|| format!("network-access request of task {key}"))?;
    row.map(|row| {
        Ok(NetworkRequest {
            id: row.try_get("id")?,
            state: row.try_get("state")?,
            raised_by_control_plane: row.try_get("raised_by_control_plane")?,
            blocking: row.try_get("blocking")?,
            reason: row.try_get("reason")?,
            scope: row.try_get("scope")?,
            close_reason: row.try_get("close_reason")?,
        })
    })
    .transpose()
}

/// A `push-check-exception` request as the tests compare them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionRequest {
    pub id: Uuid,
    pub state: String,
    pub close_reason: Option<String>,
    pub findings: Vec<ExceptionFinding>,
}

/// The `push-check-exception` requests of task `key`, oldest first.
pub async fn exception_requests(pool: &PgPool, key: &str) -> Result<Vec<ExceptionRequest>> {
    let rows = sqlx::query(
        "SELECT r.id, r.state, r.close_reason, r.payload FROM agent_requests r \
         JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1 AND r.approval_type = $2 ORDER BY r.created_at",
    )
    .bind(key)
    .bind(PUSH_CHECK_EXCEPTION)
    .fetch_all(pool)
    .await
    .with_context(|| format!("push-check-exception requests of task {key}"))?;
    rows.iter()
        .map(|row| {
            let payload: Value = row.try_get("payload")?;
            Ok(ExceptionRequest {
                id: row.try_get("id")?,
                state: row.try_get("state")?,
                close_reason: row.try_get("close_reason")?,
                findings: request_findings(&payload),
            })
        })
        .collect()
}

/// The fingerprints of the push check exceptions of task `key`.
pub async fn push_exceptions(pool: &PgPool, key: &str) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT e.fingerprint FROM push_check_exceptions e \
         JOIN tasks t ON t.id = e.task_id WHERE t.key = $1 ORDER BY e.fingerprint",
    )
    .bind(key)
    .fetch_all(pool)
    .await
    .with_context(|| format!("push check exceptions of task {key}"))
}

/// A push of a task through the Git proxy as the tests compare them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRow {
    pub verdict: String,
    pub refs: Vec<RefUpdate>,
}

/// The pushes of the runs of task `key`, oldest first.
pub async fn pushes(pool: &PgPool, key: &str) -> Result<Vec<PushRow>> {
    let rows = sqlx::query(
        "SELECT g.verdict, g.refs FROM git_pushes g JOIN runs r ON r.id = g.run_id \
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 ORDER BY g.created_at",
    )
    .bind(key)
    .fetch_all(pool)
    .await
    .with_context(|| format!("pushes of task {key}"))?;
    rows.iter()
        .map(|row| {
            let refs: Value = row.try_get("refs")?;
            Ok(PushRow {
                verdict: row.try_get("verdict")?,
                refs: serde_json::from_value(refs).context("refs of a push")?,
            })
        })
        .collect()
}

/// The text of the `push.rejected` messages queued on task `key`, oldest first.
pub async fn push_rejected_messages(pool: &PgPool, key: &str) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT m.content::text FROM task_messages m JOIN tasks t ON t.id = m.task_id \
         WHERE t.key = $1 AND m.author ->> 'kind' = 'system' AND m.author ->> 'reason' = $2 \
         ORDER BY m.queued_at",
    )
    .bind(key)
    .bind(PUSH_REJECTED)
    .fetch_all(pool)
    .await
    .with_context(|| format!("push.rejected messages of task {key}"))
}

/// The scopes of the `http-host` allowlist entries for exactly `host:port` that cover task `key`
/// (`task` for an entry of the task itself).
pub async fn http_host_scopes(
    pool: &PgPool,
    key: &str,
    host: &str,
    port: u16,
) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT e.scope FROM egress_allowlist e, tasks t \
         WHERE t.key = $1 AND e.kind = 'http-host' AND e.host = $2 AND e.port = $3 \
         AND (e.scope = 'instance' OR e.project_id = t.project_id OR e.task_id = t.id) \
         ORDER BY e.scope",
    )
    .bind(key)
    .bind(host)
    .bind(i32::from(port))
    .fetch_all(pool)
    .await
    .with_context(|| format!("allowlist entries for {host}:{port} of task {key}"))
}
