//! The database of the instance, read by the tests for what the UI does not show.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use df_core::system_reason::PUSH_REJECTED;
use df_core::{RunOutcome, RunState, TaskStatus};
use df_db::git_pushes::RefUpdate;
use df_db::push_checks::{ExceptionFinding, PUSH_CHECK_EXCEPTION, request_findings};
use df_db::requests::RequestState;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Executor, Row};
use uuid::Uuid;

use crate::wait::{RUN_TIMEOUT, eventually, status_reached};

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
    pub state: RunState,
    pub outcome: Option<RunOutcome>,
    /// Whether the run carries the time to retry after (rate limit).
    pub has_retry_after: bool,
    pub harness_session_id: String,
}

/// Status of task `key`.
pub async fn task_status(pool: &PgPool, key: &str) -> Result<TaskStatus> {
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("status of task {key}"))?;
    Ok(status.parse()?)
}

/// Waits until task `key` has `status`; fails at once when the task ended in another final status
/// (see [`status_reached`]).
pub async fn wait_for_status(pool: &PgPool, key: &str, status: TaskStatus) -> Result<()> {
    eventually(
        &format!("task {key} to be {status}"),
        RUN_TIMEOUT,
        || async { status_reached(task_status(pool, key).await?, status) },
    )
    .await
}

/// The id of task `key`.
pub async fn task_id(pool: &PgPool, key: &str) -> Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("id of task {key}"))
}

/// The keys of the tasks created by task `parent`, in creation order.
pub async fn children(pool: &PgPool, parent: &str) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT c.key FROM tasks c JOIN tasks p ON p.id = c.parent_id \
         WHERE p.key = $1 ORDER BY c.created_at",
    )
    .bind(parent)
    .fetch_all(pool)
    .await
    .with_context(|| format!("children of task {parent}"))
}

/// The key of the child of task `parent` whose description contains `text` (the marker of the
/// prompt the coordinator created it with), or `None` while the coordinator has not created it.
pub async fn child_described(pool: &PgPool, parent: &str, text: &str) -> Result<Option<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT c.key FROM tasks c JOIN tasks p ON p.id = c.parent_id \
         WHERE p.key = $1 AND strpos(c.description, $2) > 0 ORDER BY c.created_at LIMIT 1",
    )
    .bind(parent)
    .bind(text)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("child of task {parent} described by `{text}`"))
}

/// Number of messages an agent queued on task `key`.
pub async fn agent_messages(pool: &PgPool, key: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM task_messages m JOIN tasks t ON t.id = m.task_id \
         WHERE t.key = $1 AND m.author ->> 'kind' = 'agent'",
    )
    .bind(key)
    .fetch_one(pool)
    .await
    .with_context(|| format!("agent messages of task {key}"))
}

/// The keys of the tasks of project `project` with exactly `description` created at or after
/// `since` (the submit of a creation): what that creation may have left although its page never
/// opened. The description carries the suffix of the test, so no other test's task matches; the
/// time only leaves out an earlier task of the same test.
pub async fn tasks_submitted_since(
    pool: &PgPool,
    project: &str,
    description: &str,
    since: SystemTime,
) -> Result<Vec<String>> {
    // Microseconds, the resolution of the column. `bigint * interval` is computed in float8,
    // which holds every epoch microsecond (below 2^53) exactly.
    let micros = i64::try_from(since.duration_since(UNIX_EPOCH)?.as_micros())?;
    sqlx::query_scalar::<_, String>(
        "SELECT t.key FROM tasks t JOIN projects p ON p.id = t.project_id \
         WHERE p.key = $1 AND t.description = $2 \
         AND t.created_at >= TIMESTAMPTZ 'epoch' + $3 * interval '1 microsecond' \
         ORDER BY t.created_at",
    )
    .bind(project)
    .bind(description)
    .bind(micros)
    .fetch_all(pool)
    .await
    .with_context(|| format!("tasks of {project} described `{description}` since the submit"))
}

/// The prompt IDs each turn of task `key` started with, oldest turn first.
pub async fn turn_prompt_ids(pool: &PgPool, key: &str) -> Result<Vec<Vec<String>>> {
    let events = sqlx::query_scalar::<_, Value>(
        "SELECT e.payload FROM transcript_events e JOIN tasks t ON t.id = e.task_id \
         WHERE t.key = $1 AND e.kind = 'turn_started' ORDER BY e.id",
    )
    .bind(key)
    .fetch_all(pool)
    .await
    .with_context(|| format!("turns of task {key}"))?;
    events
        .into_iter()
        .map(|event| {
            let ids = event
                .pointer("/turnStarted/promptIds")
                .with_context(|| format!("prompt IDs in {event}"))?;
            serde_json::from_value(ids.clone()).context("decode the turn's prompt IDs")
        })
        .collect()
}

/// Number of runs of task `key` that ended `completed`.
pub async fn completed_runs(pool: &PgPool, key: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM runs r JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1 AND r.outcome = $2",
    )
    .bind(key)
    .bind(RunOutcome::Completed.as_str())
    .fetch_one(pool)
    .await
    .with_context(|| format!("completed runs of task {key}"))
}

/// Number of successful `read_artifact` calls for artifact `name` of task `of`, made by any task.
pub async fn artifact_reads(pool: &PgPool, of: &str, name: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM tool_calls WHERE tool_name = 'read_artifact' \
         AND arguments ->> 'task' = $1 AND arguments ->> 'name' = $2 \
         AND result IS NOT NULL AND COALESCE((result ->> 'isError')::boolean, false) = false",
    )
    .bind(of)
    .bind(name)
    .fetch_one(pool)
    .await
    .with_context(|| format!("reads of {name} of task {of}"))
}

/// Number of audit entries `action` about the runs of task `key` (`resource_id` is the run id).
pub async fn run_audit_entries(pool: &PgPool, key: &str, action: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM audit_log a JOIN runs r ON r.id::text = a.resource_id \
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 AND a.action = $2",
    )
    .bind(key)
    .bind(action)
    .fetch_one(pool)
    .await
    .with_context(|| format!("{action} audit entries of task {key}"))
}

/// Usage records of the runs of task `key`: `(known, unknown)`.
pub async fn model_usage(pool: &PgPool, key: &str) -> Result<(i64, i64)> {
    sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*) FILTER (WHERE u.usage_known), count(*) FILTER (WHERE NOT u.usage_known) \
         FROM model_usage u JOIN runs r ON r.id = u.run_id JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1",
    )
    .bind(key)
    .fetch_one(pool)
    .await
    .with_context(|| format!("model usage of task {key}"))
}

/// Number of transcript events of task `key` that contain `text`.
pub async fn transcript_events_containing(pool: &PgPool, key: &str, text: &str) -> Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM transcript_events e JOIN tasks t ON t.id = e.task_id \
         WHERE t.key = $1 AND strpos(e.payload::text, $2) > 0",
    )
    .bind(key)
    .bind(text)
    .fetch_one(pool)
    .await
    .with_context(|| format!("transcript of task {key}"))
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
            let outcome: Option<String> = row.try_get("outcome")?;
            Ok(RunRow {
                number: row.try_get("number")?,
                state: row.try_get::<String, _>("state")?.parse()?,
                outcome: outcome.map(|outcome| outcome.parse()).transpose()?,
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
    pub state: RequestState,
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
            state: row.try_get::<String, _>("state")?.parse()?,
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
    pub state: RequestState,
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
                state: row.try_get::<String, _>("state")?.parse()?,
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
    system_messages(pool, key, PUSH_REJECTED).await
}

/// The text of the messages the control plane queued on task `key` for `reason` (`push.rejected`,
/// `webhook`, ...), oldest first.
pub async fn system_messages(pool: &PgPool, key: &str, reason: &str) -> Result<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT m.content::text FROM task_messages m JOIN tasks t ON t.id = m.task_id \
         WHERE t.key = $1 AND m.author ->> 'kind' = 'system' AND m.author ->> 'reason' = $2 \
         ORDER BY m.queued_at",
    )
    .bind(key)
    .bind(reason)
    .fetch_all(pool)
    .await
    .with_context(|| format!("{reason} messages of task {key}"))
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
