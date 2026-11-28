//! The database of the instance, read by the tests for what the UI does not show.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use df_core::system_reason::PUSH_REJECTED;
use df_core::{RunOutcome, RunState, TaskStatus};
use df_db::git_pushes::RefUpdate;
use df_db::push_checks::{ExceptionFinding, PUSH_CHECK_EXCEPTION, request_findings};
use df_db::requests::RequestState;
use df_proto::transcript::v1::{Event, ToolCall, ToolCallStatus, ToolKind, event};
use sqlx::Executor;
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

use crate::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually, status_reached};

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
                // Bounds every lookup of the suite: a stalled query fails instead of hanging the job.
                conn.execute("SET statement_timeout = '30s'").await?;
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
    let status = sqlx::query_scalar!("SELECT status FROM tasks WHERE key = $1", key)
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
    sqlx::query_scalar!("SELECT id FROM tasks WHERE key = $1", key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("id of task {key}"))
}

/// Id of the newest run of task `key`.
pub async fn latest_run_id(pool: &PgPool, key: &str) -> Result<Uuid> {
    sqlx::query_scalar!(
        "SELECT r.id FROM runs r JOIN tasks t ON t.id = r.task_id
         WHERE t.key = $1 ORDER BY r.number DESC LIMIT 1",
        key
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("newest run of task {key}"))
}

/// Appends a completed tool call to the transcript of the newest run of task `key`, written like
/// the events of the runtime. For a test that needs a streak of tool calls at a time it chooses
/// (the fake agent cannot pause between calls). `seq` counts up from a number the runtime never
/// reaches, so it cannot collide with the runtime's own events of the run.
pub async fn append_tool_call(
    pool: &PgPool,
    key: &str,
    seq: u64,
    call_id: &str,
    title: &str,
) -> Result<()> {
    let task = df_core::TaskId::from_uuid(task_id(pool, key).await?);
    let run = df_core::RunId::from_uuid(latest_run_id(pool, key).await?);
    let call = Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "e2e-streak".to_owned(),
        kind: Some(event::Kind::ToolCall(ToolCall {
            tool_call_id: call_id.to_owned(),
            title: title.to_owned(),
            kind: ToolKind::Read.into(),
            status: ToolCallStatus::Completed.into(),
            raw_input_json: serde_json::json!({ "path": "src/lib.rs" }).to_string(),
            ..ToolCall::default()
        })),
    };
    let prepared = df_db::transcript::prepare(
        &df_db::PgBlobStore::new(pool.clone()),
        1_000_000 + seq,
        call,
    )
    .await?;
    let mut tx = pool.begin().await?;
    let lock = df_db::transcript::lock_run_and_task(&mut tx, run, task).await?;
    let mut writer = df_db::transcript::TranscriptWriter::new(&lock);
    writer.append_runtime(&mut tx, &prepared).await?;
    writer.finish(&mut tx).await?;
    tx.commit().await.context("append the tool call")
}

/// The keys of the tasks created by task `parent`, in creation order.
pub async fn children(pool: &PgPool, parent: &str) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT c.key FROM tasks c JOIN tasks p ON p.id = c.parent_id \
         WHERE p.key = $1 ORDER BY c.created_at",
        parent
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("children of task {parent}"))
}

/// The key of the child of task `parent` whose description contains `text` (the marker of the
/// prompt the coordinator created it with), or `None` while the coordinator has not created it.
pub async fn child_described(pool: &PgPool, parent: &str, text: &str) -> Result<Option<String>> {
    sqlx::query_scalar!(
        "SELECT c.key FROM tasks c JOIN tasks p ON p.id = c.parent_id \
         WHERE p.key = $1 AND strpos(c.description, $2) > 0 ORDER BY c.created_at LIMIT 1",
        parent,
        text
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("child of task {parent} described by `{text}`"))
}

/// The key of the base task of task `key`, if it has one.
pub async fn base_task_of(pool: &PgPool, key: &str) -> Result<Option<String>> {
    sqlx::query_scalar!(
        "SELECT b.key FROM tasks t JOIN tasks b ON b.id = t.base_task_id WHERE t.key = $1",
        key
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("base task of task {key}"))
}

/// Whether the merge of the pull request of task `key` is recorded (`tasks.pr_merged_at`).
pub async fn pull_request_merged(pool: &PgPool, key: &str) -> Result<bool> {
    sqlx::query_scalar!(
        r#"SELECT pr_merged_at IS NOT NULL AS "merged!" FROM tasks WHERE key = $1"#,
        key
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("merge of the pull request of task {key}"))
}

/// Number of messages an agent queued on task `key`.
pub async fn agent_messages(pool: &PgPool, key: &str) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM task_messages m JOIN tasks t ON t.id = m.task_id
         WHERE t.key = $1 AND m.author ->> 'kind' = 'agent'"#,
        key
    )
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
    sqlx::query_scalar!(
        "SELECT t.key FROM tasks t JOIN projects p ON p.id = t.project_id \
         WHERE p.key = $1 AND t.description = $2 \
         AND t.created_at >= TIMESTAMPTZ 'epoch' + $3::bigint * interval '1 microsecond' \
         ORDER BY t.created_at",
        project,
        description,
        micros
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("tasks of {project} described `{description}` since the submit"))
}

/// The prompt IDs each turn of task `key` started with, oldest turn first.
pub async fn turn_prompt_ids(pool: &PgPool, key: &str) -> Result<Vec<Vec<String>>> {
    let events = sqlx::query_scalar!(
        "SELECT e.payload FROM transcript_events e JOIN tasks t ON t.id = e.task_id \
         WHERE t.key = $1 AND e.kind = 'turn_started' ORDER BY e.id",
        key
    )
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
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM runs r JOIN tasks t ON t.id = r.task_id
         WHERE t.key = $1 AND r.outcome = $2"#,
        key,
        RunOutcome::Completed.as_str()
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("completed runs of task {key}"))
}

/// Number of successful `read_artifact` calls for artifact `name` of task `of`, made by any task.
pub async fn artifact_reads(pool: &PgPool, of: &str, name: &str) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM tool_calls WHERE tool_name = 'read_artifact'
         AND arguments ->> 'task' = $1 AND arguments ->> 'name' = $2
         AND result IS NOT NULL AND COALESCE((result ->> 'isError')::boolean, false) = false"#,
        of,
        name
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("reads of {name} of task {of}"))
}

/// Number of audit entries `action` about the runs of task `key` (`resource_id` is the run id).
pub async fn run_audit_entries(pool: &PgPool, key: &str, action: &str) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM audit_log a JOIN runs r ON r.id::text = a.resource_id
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 AND a.action = $2"#,
        key,
        action
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("{action} audit entries of task {key}"))
}

/// Number of audit entries `action` about persona `handle` (`resource_id` is the handle).
pub async fn persona_audit_entries(pool: &PgPool, handle: &str, action: &str) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE resource_id = $1 AND action = $2")
        .bind(handle)
        .bind(action)
        .fetch_one(pool)
        .await
        .with_context(|| format!("{action} audit entries of persona {handle}"))
}

/// Usage records of the runs of task `key`: `(known, unknown)`.
pub async fn model_usage(pool: &PgPool, key: &str) -> Result<(i64, i64)> {
    let usage = sqlx::query!(
        r#"SELECT count(*) FILTER (WHERE u.usage_known) AS "known!",
         count(*) FILTER (WHERE NOT u.usage_known) AS "unknown!"
         FROM model_usage u JOIN runs r ON r.id = u.run_id JOIN tasks t ON t.id = r.task_id
         WHERE t.key = $1"#,
        key
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("model usage of task {key}"))?;
    Ok((usage.known, usage.unknown))
}

/// Number of transcript events of task `key` that contain `text`.
pub async fn transcript_events_containing(pool: &PgPool, key: &str, text: &str) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM transcript_events e JOIN tasks t ON t.id = e.task_id
         WHERE t.key = $1 AND strpos(e.payload::text, $2) > 0"#,
        key,
        text
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("transcript of task {key}"))
}

/// Number of transcript events of task `key` that contain both `first` and `second`.
pub async fn transcript_events_containing_both(
    pool: &PgPool,
    key: &str,
    first: &str,
    second: &str,
) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM transcript_events e JOIN tasks t ON t.id = e.task_id
         WHERE t.key = $1 AND strpos(e.payload::text, $2) > 0
           AND strpos(e.payload::text, $3) > 0"#,
        key,
        first,
        second
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("transcript of task {key}"))
}

/// Number of messages of task `key` whose content contains `text`.
pub async fn messages_containing(pool: &PgPool, key: &str, text: &str) -> Result<i64> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM task_messages m JOIN tasks t ON t.id = m.task_id
         WHERE t.key = $1 AND strpos(m.content::text, $2) > 0"#,
        key,
        text
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("messages of task {key}"))
}

/// The pull request number and URL linked to task `key`.
pub async fn task_pull_request(pool: &PgPool, key: &str) -> Result<Option<(i64, String)>> {
    let row = sqlx::query!("SELECT pr_number, pr_url FROM tasks WHERE key = $1", key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("pull request of task {key}"))?;
    Ok(row.pr_number.zip(row.pr_url))
}

/// The runs of task `key`, oldest first.
pub async fn runs(pool: &PgPool, key: &str) -> Result<Vec<RunRow>> {
    let rows = sqlx::query!(
        r#"SELECT r.number, r.state, r.outcome, r.retry_after IS NOT NULL AS "has_retry_after!",
         r.harness_session_id FROM runs r
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 ORDER BY r.number"#,
        key
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("runs of task {key}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(RunRow {
                number: row.number,
                state: row.state.parse()?,
                outcome: row.outcome.map(|outcome| outcome.parse()).transpose()?,
                has_retry_after: row.has_retry_after,
                harness_session_id: row.harness_session_id,
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
    let fingerprint = format!("{host}:{port}");
    let row = sqlx::query!(
        r#"SELECT r.id, r.state, r.run_id IS NULL AS "raised_by_control_plane!", r.blocking, r.reason,
         r.decision ->> 'scope' AS scope, r.close_reason FROM agent_requests r
         JOIN tasks t ON t.id = r.task_id
         WHERE t.key = $1 AND r.approval_type = 'network-access' AND r.fingerprint = $2
         ORDER BY r.created_at DESC LIMIT 1"#,
        key,
        fingerprint
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("network-access request of task {key}"))?;
    row.map(|row| {
        Ok(NetworkRequest {
            id: row.id,
            state: row.state.parse()?,
            raised_by_control_plane: row.raised_by_control_plane,
            blocking: row.blocking,
            reason: row.reason,
            scope: row.scope,
            close_reason: row.close_reason,
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
    let rows = sqlx::query!(
        "SELECT r.id, r.state, r.close_reason, r.payload FROM agent_requests r \
         JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1 AND r.approval_type = $2 ORDER BY r.created_at",
        key,
        PUSH_CHECK_EXCEPTION
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("push-check-exception requests of task {key}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(ExceptionRequest {
                id: row.id,
                state: row.state.parse()?,
                close_reason: row.close_reason,
                findings: request_findings(&row.payload),
            })
        })
        .collect()
}

/// The fingerprints of the push check exceptions of task `key`.
pub async fn push_exceptions(pool: &PgPool, key: &str) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT e.fingerprint FROM push_check_exceptions e \
         JOIN tasks t ON t.id = e.task_id WHERE t.key = $1 ORDER BY e.fingerprint",
        key
    )
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
    let rows = sqlx::query!(
        "SELECT g.verdict, g.refs FROM git_pushes g JOIN runs r ON r.id = g.run_id \
         JOIN tasks t ON t.id = r.task_id WHERE t.key = $1 ORDER BY g.created_at",
        key
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("pushes of task {key}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(PushRow {
                verdict: row.verdict,
                refs: serde_json::from_value(row.refs).context("refs of a push")?,
            })
        })
        .collect()
}

/// The public signing key (OpenSSH format) of the persona with `handle`; `None` while it has none.
pub async fn persona_signing_key(pool: &PgPool, handle: &str) -> Result<Option<String>> {
    sqlx::query_scalar!(
        "SELECT signing_public_key FROM personas WHERE handle = $1",
        handle
    )
    .fetch_one(pool)
    .await
    .with_context(|| format!("signing key of persona {handle}"))
}

/// The text of the `push.rejected` messages queued on task `key`, oldest first.
pub async fn push_rejected_messages(pool: &PgPool, key: &str) -> Result<Vec<String>> {
    system_messages(pool, key, PUSH_REJECTED).await
}

/// The text of the messages the control plane queued on task `key` for `reason` (`push.rejected`,
/// `webhook`, ...), oldest first.
pub async fn system_messages(pool: &PgPool, key: &str, reason: &str) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        r#"SELECT m.content::text AS "content!" FROM task_messages m JOIN tasks t ON t.id = m.task_id
         WHERE t.key = $1 AND m.author ->> 'kind' = 'system' AND m.author ->> 'reason' = $2
         ORDER BY m.queued_at"#,
        key,
        reason
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("{reason} messages of task {key}"))
}

/// The id of the newest `create-issue` request of task `key`.
pub async fn create_issue_request(pool: &PgPool, key: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT r.id FROM agent_requests r JOIN tasks t ON t.id = r.task_id \
         WHERE t.key = $1 AND r.approval_type = 'create-issue' ORDER BY r.created_at DESC LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("create-issue request of task {key}"))
}

/// The number of the issue the control plane created for the approved `create-issue` request `id`;
/// `None` until the job recorded it.
pub async fn created_issue_number(pool: &PgPool, id: Uuid) -> Result<Option<u64>> {
    let number: Option<String> =
        sqlx::query_scalar("SELECT decision ->> 'issue_number' FROM agent_requests WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .with_context(|| format!("decision of request {id}"))?;
    number
        .map(|n| n.parse().with_context(|| format!("issue number `{n}`")))
        .transpose()
}

/// The number of the issue task `key` is linked to.
pub async fn linked_issue(pool: &PgPool, key: &str) -> Result<Option<i64>> {
    sqlx::query_scalar("SELECT issue_number FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .with_context(|| format!("linked issue of task {key}"))
}

/// The scopes of the `http-host` allowlist entries for exactly `host:port` that cover task `key`
/// (`task` for an entry of the task itself).
pub async fn http_host_scopes(
    pool: &PgPool,
    key: &str,
    host: &str,
    port: u16,
) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT e.scope FROM egress_allowlist e, tasks t \
         WHERE t.key = $1 AND e.kind = 'http-host' AND e.host = $2 AND e.port = $3 \
         AND (e.scope = 'instance' OR e.project_id = t.project_id OR e.task_id = t.id) \
         ORDER BY e.scope",
        key,
        host,
        i32::from(port)
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("allowlist entries for {host}:{port} of task {key}"))
}

/// Waits until the agent of task `key` asked the question with `text`; returns its row.
pub async fn question(pool: &PgPool, key: &str, text: &str) -> Result<df_db::requests::RequestRow> {
    let task = df_core::TaskId::from_uuid(task_id(pool, key).await?);
    eventually(
        &format!("question `{text}` of {key}"),
        RUN_TIMEOUT,
        || async {
            Ok(df_db::requests::list_for_task(pool, task)
                .await?
                .into_iter()
                .find(|r| r.question_text() == text))
        },
    )
    .await
}

/// The request `id` as stored now.
pub async fn request(pool: &PgPool, id: df_core::RequestId) -> Result<df_db::requests::RequestRow> {
    df_db::requests::get_request(pool, id)
        .await?
        .with_context(|| format!("request {id}"))
}

/// Waits until the question `id` is answered; returns the row.
pub async fn answered_question(
    pool: &PgPool,
    id: df_core::RequestId,
) -> Result<df_db::requests::RequestRow> {
    eventually(
        &format!("question {id} to be answered"),
        UI_TIMEOUT,
        || async {
            let row = request(pool, id).await?;
            Ok((row.state == RequestState::Answered).then_some(row))
        },
    )
    .await
}

/// Raises a text question on task `key` the way its agent's `ask_question` call does (the same
/// storage call), for a test that needs one more request at a chosen moment.
pub async fn raise_text_question(
    pool: &PgPool,
    key: &str,
    text: &str,
) -> Result<df_core::RequestId> {
    let task = df_core::TaskId::from_uuid(task_id(pool, key).await?);
    let actor = df_core::Principal::system("e2e");
    let (row, _) = df_db::requests::raise_question(
        pool,
        df_db::requests::Raise {
            task_id: task,
            run_id: None,
            actor: &actor,
            blocking: false,
        },
        df_core::QuestionSpec::text_question(text),
    )
    .await?;
    Ok(row.id)
}

/// Makes `names` the MCP servers of the suite: any server of that name is disabled everywhere and
/// deleted first (so a repeated run starts clean), then each is registered with the credential
/// reference `fake-mcp` (bearer) and the URL of the fake MCP server for its name, with "Private
/// network" because the fake runs on a cluster address. Returns the ids in the order of `names`.
pub async fn register_mcp_servers(pool: &PgPool, names: &[&str]) -> Result<Vec<Uuid>> {
    let principal = df_core::Principal::system("e2e");
    let roles = vec!["admin".to_owned()];
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    remove_mcp_servers(pool, names).await?;
    let credential = df_db::catalog::list_credential_refs(pool)
        .await?
        .into_iter()
        .find(|c| c.name == crate::data::MCP_CREDENTIAL_NAME)
        .context("the seeded credential reference of the MCP servers")?;
    let mut ids = Vec::new();
    for name in names {
        let saved = df_db::mcp_servers::save_mcp_server(
            pool,
            actor,
            None,
            &df_db::mcp_servers::McpServerInput {
                name: (*name).to_owned(),
                url: crate::data::fake_mcp_url(name),
                credential_ref_id: credential.id.as_uuid(),
                scheme: df_core::mcp::McpCredentialScheme::Bearer,
                private_network: true,
            },
        )
        .await
        .with_context(|| format!("register MCP server {name}"))?;
        ids.push(saved.id);
    }
    Ok(ids)
}

/// Disables the MCP servers `names` for every project and deletes them.
pub async fn remove_mcp_servers(pool: &PgPool, names: &[&str]) -> Result<()> {
    let principal = df_core::Principal::system("e2e");
    let roles = vec!["admin".to_owned()];
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    let existing = df_db::mcp_servers::list_mcp_servers(pool).await?;
    for server in existing
        .iter()
        .filter(|s| names.contains(&s.input.name.as_str()))
    {
        for key in &server.projects {
            let project = df_db::catalog::get_project_by_key(pool, key)
                .await?
                .with_context(|| format!("project {key}"))?;
            let keep: Vec<Uuid> = df_db::mcp_servers::project_mcp_server_ids(pool, project.id)
                .await?
                .into_iter()
                .filter(|id| *id != server.id)
                .collect();
            df_db::mcp_servers::set_project_mcp_servers(pool, actor, project.id, &keep).await?;
        }
        df_db::mcp_servers::delete_mcp_server(pool, actor, server.id).await?;
    }
    Ok(())
}

/// Enables exactly the servers `ids` for project `key`.
pub async fn enable_mcp_servers(pool: &PgPool, key: &str, ids: &[Uuid]) -> Result<()> {
    let principal = df_core::Principal::system("e2e");
    let roles = vec!["admin".to_owned()];
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    let project = df_db::catalog::get_project_by_key(pool, key)
        .await?
        .with_context(|| format!("project {key}"))?;
    df_db::mcp_servers::set_project_mcp_servers(pool, actor, project.id, ids)
        .await
        .with_context(|| format!("enable MCP servers for {key}"))?;
    Ok(())
}

/// The MCP call records of the runs of task `key`, oldest first: server, tool and outcome.
pub async fn mcp_calls(
    pool: &PgPool,
    key: &str,
) -> Result<Vec<(String, String, df_core::mcp::McpCallOutcome)>> {
    let task = df_core::TaskId::from_uuid(task_id(pool, key).await?);
    let mut calls: Vec<_> = df_db::task_views::list_runs(pool, task)
        .await?
        .into_iter()
        .flat_map(|run| run.mcp_calls)
        .collect();
    calls.sort_by_key(|call| call.started_at);
    Ok(calls
        .into_iter()
        .map(|call| (call.server_name, call.tool, call.outcome))
        .collect())
}
