//! Task management persistence operations (`specs/task-management/spec.md`).

use chrono::{DateTime, Utc};
use df_core::task_transition::{DoneBy, StopScope, TaskEvent, TaskSnapshot, transition};
use df_core::{
    DependencyKind, FALLBACK_TITLE, PersonaId, Principal, ProjectId, ProjectKey, PromptId, RunId,
    TITLE_MAX_CHARS, TaskId, TaskKey, TaskStatus, TitleSource, is_satisfied, placeholder_title,
    sanitize_title, system_reason, title_too_long,
};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::task_events::{apply_task_event, execute_effects};

/// Outbox kind that marks a task's open pull request as draft again; payload [`MarkDraftJob`].
pub const MARK_DRAFT_KIND: &str = "pr.mark_draft";

/// Payload of a [`MARK_DRAFT_KIND`] job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkDraftJob {
    pub task_id: TaskId,
}

/// Outbox kind for the forge actions of a task's pull request; payload [`PrSyncJob`].
pub const PR_SYNC_KIND: &str = "pr.sync";

/// Payload of a [`PR_SYNC_KIND`] job (`pr-forge-outbox` design D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSyncJob {
    pub task_id: TaskId,
    /// The run whose transcript gets the forge events and whose summary is posted; mark done has
    /// none.
    pub run_id: Option<RunId>,
    /// Open the draft PR if the task has none.
    pub open: bool,
    /// Mark the PR ready for review. The value is the time of the completion; a message queued
    /// after it keeps the PR a draft.
    pub ready: Option<DateTime<Utc>>,
}

/// The database clock, for times that are ordered against `task_messages.queued_at` (a column
/// defaulting to `clock_timestamp()`, set by the insert after the task row lock): the `ready`
/// time of a [`PrSyncJob`]. `runs.stop_requested_at` uses the same clock (`clock_timestamp()` in
/// `request_stop_tx`).
///
/// The caller holds the task row lock. A message whose transaction waited for that lock inserts
/// after it, so its `queued_at` is later than the value read here; one committed before is
/// earlier. `now()` would be the transaction start and break this order (`pr-forge-outbox`
/// design D3).
pub async fn clock_under_task_lock(
    conn: &mut sqlx::PgConnection,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(conn)
        .await
}

/// Parameters for creating a new task.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    /// Title given by the creator; without one the first sentence of the description is used.
    pub title: Option<String>,
    pub description: String,
    pub issue_number: Option<i64>,
    /// Who creates the task: a board member, or the agent of the creating task.
    pub created_by: Principal,
    /// The task that creates this one; `None` for a task created in the UI.
    pub parent_id: Option<TaskId>,
    /// Tasks of the project this one waits for before its first start.
    pub depends_on: Vec<NewDependency>,
}

/// A dependency of a new task on another task of the project, named by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDependency {
    pub key: TaskKey,
    pub kind: DependencyKind,
}

/// Stored task record from `tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRow {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub number: u64,
    pub key: TaskKey,
    pub title: String,
    pub title_source: TitleSource,
    pub description: String,
    pub persona_id: PersonaId,
    pub issue_number: Option<i64>,
    pub status: TaskStatus,
    pub status_line: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_body: Option<String>,
    pub diverged: bool,
    pub harness_session_id: String,
    pub created_by: Principal,
    pub parent_id: Option<TaskId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Stored prompt / message record from `task_messages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMessageRow {
    pub id: PromptId,
    pub task_id: TaskId,
    pub author: Principal,
    pub content: Vec<ContentBlock>,
    pub queued_at: DateTime<Utc>,
    pub delivered_run_id: Option<RunId>,
    pub delivered_at: Option<DateTime<Utc>>,
    /// Set when the message was handed to the run's runtime; held-back messages are `None`.
    pub forwarded_at: Option<DateTime<Utc>>,
}

impl TaskMessageRow {
    /// Whether a board member may still edit or remove the message.
    pub fn is_editable(&self) -> bool {
        self.delivered_at.is_none()
            && self.forwarded_at.is_none()
            && matches!(self.author, Principal::Human { .. })
    }
}

/// Formats the initial assignment prompt text. `title` is the title given by a person; without
/// one the prompt asks the persona to set a title with `set_task_title`.
pub fn format_assignment_prompt(title: Option<&str>, description: &str) -> Vec<ContentBlock> {
    let description = description.trim();
    let text = match title {
        Some(title) if description.is_empty() => format!("# {title}"),
        Some(title) => format!("# {title}\n\n{description}"),
        None => description.to_string(),
    };
    let mut blocks = Vec::new();
    if !text.is_empty() {
        blocks.push(text_block(text, false));
    }
    if title.is_none() {
        blocks.push(text_block(ask_for_title(), true));
    }
    blocks
}

/// A text content block, hidden from the transcript view when `hidden`.
pub fn text_block(text: String, hidden: bool) -> ContentBlock {
    ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text })),
        hidden,
    }
}

/// Creates a project-scoped task with gap-free numbering (`projects.next_task_number` row lock)
/// and queues the initial `task.assigned` prompt in the same transaction.
///
/// Publishes `task_changed` and `task_message_queued` notifications transactionally.
pub async fn create_task(
    pool: &PgPool,
    input: NewTask,
) -> Result<(TaskRow, TaskMessageRow), DbError> {
    let mut tx = pool.begin().await?;
    let created = create_task_tx(&mut tx, input).await?;
    tx.commit().await?;
    Ok(created)
}

/// [`create_task`] inside the caller's transaction; the notifications go out on commit.
pub async fn create_task_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: NewTask,
) -> Result<(TaskRow, TaskMessageRow), DbError> {
    let (task_key, number) = allocate_task_key(tx, input.project_id).await?;
    let resolved = resolve_dependencies(tx, &input).await?;
    let user_title = input.title.as_deref().and_then(sanitize_title);
    let snapshot = TaskSnapshot {
        unsatisfied_dependencies: resolved.unsatisfied_count,
        ..TaskSnapshot::new_task()
    };
    let created = transition(&snapshot, TaskEvent::Created)
        .map_err(|rejected| DbError::Conflict(rejected.to_string()))?;
    let task_row = insert_task(
        tx,
        &input,
        task_key,
        number,
        user_title.as_deref(),
        created.status,
    )
    .await?;
    for (target, kind) in resolved.targets {
        sqlx::query(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id, kind) VALUES ($1, $2, $3)",
        )
        .bind(task_row.id.as_uuid())
        .bind(target)
        .bind(kind.as_str())
        .execute(&mut **tx)
        .await?;
    }
    let message_row = enqueue_assignment_prompt(
        tx,
        task_row.id,
        user_title.as_deref(),
        &task_row.description,
    )
    .await?;

    // `Created` has effects on this task only, so there is no work for other tasks.
    let followups = execute_effects(
        tx,
        task_row.id,
        &created,
        &Principal::system("task.created"),
    )
    .await?;
    debug_assert!(followups.is_empty(), "Created yields no follow-up work");
    Ok((task_row, message_row))
}

/// A dependency target as [`resolve_dependencies`] reads it.
#[derive(sqlx::FromRow)]
struct TargetRow {
    id: Uuid,
    key: String,
    first_completed_at: Option<DateTime<Utc>>,
    first_done_at: Option<DateTime<Utc>>,
}

/// The dependencies of a new task, resolved: the target rows and how many are unsatisfied.
struct ResolvedDependencies {
    targets: Vec<(Uuid, DependencyKind)>,
    unsatisfied_count: u32,
}

/// Resolves the dependency keys of `input` in its project and counts the unsatisfied ones. The
/// targets are read `FOR SHARE`: a target's event locks its row `FOR UPDATE` before it releases
/// its dependents, so either this creation commits first and the release sees the new dependent,
/// or the event commits first and this read sees its `first_completed_at` (design D3).
async fn resolve_dependencies(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &NewTask,
) -> Result<ResolvedDependencies, DbError> {
    let mut seen = std::collections::HashSet::new();
    if let Some(twice) = input.depends_on.iter().find(|d| !seen.insert(&d.key)) {
        return Err(DbError::InvalidDependency(twice.key.to_string()));
    }
    let keys: Vec<String> = input.depends_on.iter().map(|d| d.key.to_string()).collect();
    let rows: Vec<TargetRow> = sqlx::query_as(
        "SELECT id, key, first_completed_at, first_done_at FROM tasks
         WHERE project_id = $1 AND key = ANY($2) ORDER BY id FOR SHARE",
    )
    .bind(input.project_id.as_uuid())
    .bind(&keys)
    .fetch_all(&mut **tx)
    .await?;
    let mut resolved = ResolvedDependencies {
        targets: Vec::new(),
        unsatisfied_count: 0,
    };
    for dependency in &input.depends_on {
        let key = dependency.key.to_string();
        let Some(target) = rows.iter().find(|row| row.key == key) else {
            return Err(DbError::InvalidDependency(key));
        };
        resolved.targets.push((target.id, dependency.kind));
        if !is_satisfied(
            dependency.kind,
            target.first_completed_at,
            target.first_done_at,
        ) {
            resolved.unsatisfied_count += 1;
        }
    }
    Ok(resolved)
}

/// Locks the project row and allocates the next gap-free task number. Returns the task key and
/// the number.
async fn allocate_task_key(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: ProjectId,
) -> Result<(TaskKey, i64), DbError> {
    let project_alloc = sqlx::query!(
        r#"
        UPDATE projects
        SET next_task_number = next_task_number + 1,
            updated_at = now()
        WHERE id = $1
        RETURNING key, next_task_number - 1 AS "allocated_number!"
        "#,
        project_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("project `{project_id}` not found")))?;

    let project_key =
        ProjectKey::new(project_alloc.key).map_err(|e| DbError::InvalidData(e.to_string()))?;
    let number = u64::try_from(project_alloc.allocated_number)
        .map_err(|_negative| DbError::InvalidData("negative task number".into()))?;
    let task_key =
        TaskKey::new(project_key, number).map_err(|e| DbError::InvalidData(e.to_string()))?;
    Ok((task_key, project_alloc.allocated_number))
}

/// Inserts the queued task. Without a usable `user_title` the title is a placeholder derived from
/// the description.
async fn insert_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &NewTask,
    task_key: TaskKey,
    allocated_number: i64,
    user_title: Option<&str>,
    status: TaskStatus,
) -> Result<TaskRow, DbError> {
    // A title the creating agent chose is an agent title, which a generated or edited one replaces.
    let given_source = if matches!(input.created_by, Principal::Agent { .. }) {
        TitleSource::Agent
    } else {
        TitleSource::User
    };
    let (title, title_source) = match user_title {
        Some(title) => (title.to_string(), given_source),
        None => (
            placeholder_title(&input.description).unwrap_or_else(|| FALLBACK_TITLE.to_string()),
            TitleSource::Placeholder,
        ),
    };
    let task_id = TaskId::new();
    let key_str = task_key.to_string();
    let created_by = serde_json::to_value(&input.created_by)?;

    let row = sqlx::query!(
        r#"
        INSERT INTO tasks (
            id, project_id, number, key, title, title_source, description,
            persona_id, issue_number, status, created_by, parent_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING
            id, project_id, number, key, title, title_source, description,
            persona_id, issue_number, status, status_line,
            pr_number, pr_url, pr_title, pr_body,
            diverged, harness_session_id, created_at, updated_at
        "#,
        task_id.as_uuid(),
        input.project_id.as_uuid(),
        allocated_number,
        key_str,
        title,
        title_source.as_str(),
        input.description,
        input.persona_id.as_uuid(),
        input.issue_number,
        status.as_str(),
        created_by,
        input.parent_id.map(|id| id.as_uuid())
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(TaskRow {
        id: TaskId::from_uuid(row.id),
        project_id: ProjectId::from_uuid(row.project_id),
        number: task_key.number(),
        key: task_key,
        title: row.title,
        title_source,
        description: row.description,
        persona_id: PersonaId::from_uuid(row.persona_id),
        issue_number: row.issue_number,
        status,
        status_line: row.status_line,
        pr_number: row.pr_number,
        pr_url: row.pr_url,
        pr_title: row.pr_title,
        pr_body: row.pr_body,
        diverged: row.diverged,
        harness_session_id: row.harness_session_id,
        created_by: input.created_by.clone(),
        parent_id: input.parent_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Queues the initial `task.assigned` prompt of a new task.
async fn enqueue_assignment_prompt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
    user_title: Option<&str>,
    description: &str,
) -> Result<TaskMessageRow, DbError> {
    let prompt_id = PromptId::new();
    let author = Principal::system(system_reason::TASK_ASSIGNED);
    let content = format_assignment_prompt(user_title, description);
    let author_json = serde_json::to_value(&author)?;
    let content_json = serde_json::to_value(&content)?;

    let msg_row = sqlx::query!(
        r#"
        INSERT INTO task_messages (id, task_id, author, content)
        VALUES ($1, $2, $3, $4)
        RETURNING id, task_id, author, content, queued_at, delivered_run_id, delivered_at
        "#,
        prompt_id.as_uuid(),
        task_id.as_uuid(),
        author_json,
        content_json
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(TaskMessageRow {
        id: PromptId::from_uuid(msg_row.id),
        task_id: TaskId::from_uuid(msg_row.task_id),
        author,
        content,
        queued_at: msg_row.queued_at,
        delivered_run_id: msg_row.delivered_run_id.map(RunId::from_uuid),
        delivered_at: msg_row.delivered_at,
        forwarded_at: None,
    })
}

/// Queues a message on a task.
///
/// If the task has an active run that is not `finalizing` (running, or idle in `awaiting_input`),
/// the status becomes `running` and the run picks the message up. Otherwise (`waiting`,
/// `in_review`, `failed`, `completed`, `rate_limited`, `limit_reached`, or a run that is ending)
/// it becomes `queued` and the scheduler starts a new run once the old one ended
/// (`iter1-run-orchestration` spec: *"the task is `rate_limited` and no new run starts until a
/// message is queued"*). The `UPDATE` locks the task row, which serializes with the run's phase
/// updates and its end.
pub async fn queue_message(
    pool: &PgPool,
    task_id: TaskId,
    author: Principal,
    content: Vec<ContentBlock>,
) -> Result<TaskMessageRow, DbError> {
    let mut tx = pool.begin().await?;
    let row = queue_message_tx(&mut tx, task_id, author, content).await?;
    tx.commit().await?;
    Ok(row)
}

/// [`queue_message`] inside the caller's transaction; the notifications go out on commit.
///
/// A message that moves the task out of `in_review` or `completed` while it has an unmerged pull
/// request enqueues a [`MARK_DRAFT_KIND`] job, so the forge shows the task is working on it again
/// (`iter3-webhooks` design D5). The job leaves a closed pull request alone.
pub async fn queue_message_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
    author: Principal,
    content: Vec<ContentBlock>,
) -> Result<TaskMessageRow, DbError> {
    let prompt_id = PromptId::new();
    let author_json = serde_json::to_value(&author)?;
    let content_json = serde_json::to_value(&content)?;

    apply_task_event(tx, task_id, TaskEvent::MessageQueued, &author).await?;

    let msg_row = sqlx::query!(
        r#"
        INSERT INTO task_messages (id, task_id, author, content)
        VALUES ($1, $2, $3, $4)
        RETURNING id, task_id, queued_at, delivered_run_id, delivered_at
        "#,
        prompt_id.as_uuid(),
        task_id.as_uuid(),
        author_json,
        content_json
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(TaskMessageRow {
        id: PromptId::from_uuid(msg_row.id),
        task_id: TaskId::from_uuid(msg_row.task_id),
        author,
        content,
        queued_at: msg_row.queued_at,
        delivered_run_id: msg_row.delivered_run_id.map(RunId::from_uuid),
        delivered_at: msg_row.delivered_at,
        forwarded_at: None,
    })
}

/// Text of the prompt a resumed run starts with when nothing else is queued.
pub const RESUME_PROMPT: &str = "Continue the task where the previous run stopped.";

/// Queues [`RESUME_PROMPT`] (author `system`, reason `run.resumed`) for a task whose run starts
/// from its resume time, unless the task has an undelivered message to continue with. Unlike
/// [`queue_message_tx`] it leaves status, resume time and resume count to the caller. Returns
/// whether a prompt was queued.
pub async fn queue_resume_prompt_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
) -> Result<bool, DbError> {
    let author = serde_json::to_value(Principal::system(system_reason::RUN_RESUMED))?;
    let content = serde_json::to_value(vec![text_block(RESUME_PROMPT.into(), false)])?;
    let inserted = sqlx::query(
        "INSERT INTO task_messages (id, task_id, author, content)
         SELECT $1, $2, $3, $4
         WHERE NOT EXISTS (
             SELECT 1 FROM task_messages
             WHERE task_id = $2 AND delivered_at IS NULL AND discarded_at IS NULL
         )",
    )
    .bind(PromptId::new().as_uuid())
    .bind(task_id.as_uuid())
    .bind(author)
    .bind(content)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(inserted > 0)
}

/// Marks a queued prompt as delivered by `run_id` when its `UserMessage` event is persisted.
/// Idempotent: returns `true` if a row existed for `prompt_id`.
pub async fn mark_prompt_delivered(
    pool: &PgPool,
    prompt_id: PromptId,
    run_id: RunId,
) -> Result<bool, DbError> {
    let result = sqlx::query!(
        r#"
        UPDATE task_messages
        SET delivered_run_id = COALESCE(delivered_run_id, $2),
            delivered_at = COALESCE(delivered_at, now())
        WHERE id = $1
        "#,
        prompt_id.as_uuid(),
        run_id.as_uuid()
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Fetches a task by its [`TaskId`].
pub async fn get_task(pool: &PgPool, task_id: TaskId) -> Result<Option<TaskRow>, DbError> {
    let row = sqlx::query!(
        r#"
        SELECT
            id, project_id, number, key, title, title_source, description,
            persona_id, issue_number, status, status_line,
            pr_number, pr_url, pr_title, pr_body,
            diverged, harness_session_id, created_by, parent_id, created_at, updated_at
        FROM tasks
        WHERE id = $1
        "#,
        task_id.as_uuid()
    )
    .fetch_optional(pool)
    .await?;

    row.map(|r| {
        let number = u64::try_from(r.number)
            .ok()
            .filter(|&n| n >= 1)
            .ok_or_else(|| {
                DbError::InvalidData(format!("non-positive task number `{}`", r.number))
            })?;
        let key: TaskKey = r
            .key
            .parse()
            .map_err(|e: df_core::KeyError| DbError::InvalidData(e.to_string()))?;
        if key.number() != number {
            return Err(DbError::InvalidData(format!(
                "task number `{number}` does not match key `{key}`"
            )));
        }
        Ok(TaskRow {
            id: TaskId::from_uuid(r.id),
            project_id: ProjectId::from_uuid(r.project_id),
            number,
            key,
            title: r.title,
            title_source: r.title_source.parse()?,
            description: r.description,
            persona_id: PersonaId::from_uuid(r.persona_id),
            issue_number: r.issue_number,
            status: r.status.parse()?,
            status_line: r.status_line,
            pr_number: r.pr_number,
            pr_url: r.pr_url,
            pr_title: r.pr_title,
            pr_body: r.pr_body,
            diverged: r.diverged,
            harness_session_id: r.harness_session_id,
            created_by: serde_json::from_value(r.created_by)?,
            parent_id: r.parent_id.map(TaskId::from_uuid),
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    })
    .transpose()
}

/// A `task_messages` row as the queries return it.
struct MessageRecord {
    id: Uuid,
    task_id: Uuid,
    author: serde_json::Value,
    content: serde_json::Value,
    queued_at: DateTime<Utc>,
    delivered_run_id: Option<Uuid>,
    delivered_at: Option<DateTime<Utc>>,
    forwarded_at: Option<DateTime<Utc>>,
}

impl TryFrom<MessageRecord> for TaskMessageRow {
    type Error = DbError;

    fn try_from(r: MessageRecord) -> Result<Self, DbError> {
        Ok(Self {
            id: PromptId::from_uuid(r.id),
            task_id: TaskId::from_uuid(r.task_id),
            author: serde_json::from_value(r.author)?,
            content: serde_json::from_value(r.content)?,
            queued_at: r.queued_at,
            delivered_run_id: r.delivered_run_id.map(RunId::from_uuid),
            delivered_at: r.delivered_at,
            forwarded_at: r.forwarded_at,
        })
    }
}

/// Lists all undelivered prompts for `task_id` in queue order.
pub async fn list_undelivered_messages(
    pool: &PgPool,
    task_id: TaskId,
) -> Result<Vec<TaskMessageRow>, DbError> {
    let rows = sqlx::query_as!(
        MessageRecord,
        r#"
        SELECT id, task_id, author, content, queued_at, delivered_run_id, delivered_at,
               forwarded_at
        FROM task_messages
        WHERE task_id = $1 AND delivered_at IS NULL AND discarded_at IS NULL
        ORDER BY queued_at ASC, id ASC
        "#,
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

/// Marks the held-back undelivered messages of `task_id` as forwarded and returns them in queue
/// order. Contends with [`edit_queued_message`] and [`remove_queued_message`] on the row, so a
/// message is either claimed or edited, never both.
pub async fn claim_held_back_messages(
    pool: &PgPool,
    task_id: TaskId,
) -> Result<Vec<TaskMessageRow>, DbError> {
    let rows = sqlx::query_as!(
        MessageRecord,
        r#"
        UPDATE task_messages SET forwarded_at = now()
        WHERE task_id = $1 AND delivered_at IS NULL AND forwarded_at IS NULL
          AND discarded_at IS NULL
        RETURNING id, task_id, author, content, queued_at, delivered_run_id, delivered_at,
                  forwarded_at
        "#,
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    let mut messages = rows
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<TaskMessageRow>, _>>()?;
    messages.sort_by_key(|m| (m.queued_at, m.id.as_uuid()));
    Ok(messages)
}

/// Replaces the text of a queued message that is still held back (human author, neither
/// forwarded nor delivered; the same rule as [`TaskMessageRow::is_editable`]). The prompt id and
/// queue position stay. Audited as `message.edit`.
pub async fn edit_queued_message(
    pool: &PgPool,
    task_id: TaskId,
    prompt_id: PromptId,
    text: &str,
    actor: Principal,
) -> Result<(), DbError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(DbError::InvalidData("message is empty".into()));
    }
    let mut tx = pool.begin().await?;
    let old = sqlx::query_scalar!(
        r#"
        SELECT content FROM task_messages
        WHERE id = $1 AND task_id = $2 AND delivered_at IS NULL AND forwarded_at IS NULL
          AND discarded_at IS NULL AND author->>'kind' = 'human'
        FOR UPDATE
        "#,
        prompt_id.as_uuid(),
        task_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(not_editable)?;
    let new = serde_json::to_value(vec![text_block(text.into(), false)])?;
    sqlx::query!(
        "UPDATE task_messages SET content = $2 WHERE id = $1",
        prompt_id.as_uuid(),
        new
    )
    .execute(&mut *tx)
    .await?;
    let entry = NewAuditEntry::new(
        actor,
        Vec::new(),
        "message.edit",
        "task_message",
        prompt_id.to_string(),
    )
    .with_details(&serde_json::json!({
        "task_id": task_id,
        "old_text": plain_text(old)?,
        "new_text": text,
    }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::TaskMessageChanged { task_id }).await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes a queued message that is still held back (the rule of [`edit_queued_message`]). A
/// `queued` task left without undelivered message returns to `waiting`. Audited as
/// `message.remove`.
pub async fn remove_queued_message(
    pool: &PgPool,
    task_id: TaskId,
    prompt_id: PromptId,
    actor: Principal,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    // Same lock order as `queue_message`: every later statement sees a message it committed.
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut *tx)
        .await?;
    let old = sqlx::query_scalar!(
        r#"
        DELETE FROM task_messages
        WHERE id = $1 AND task_id = $2 AND delivered_at IS NULL AND forwarded_at IS NULL
          AND discarded_at IS NULL AND author->>'kind' = 'human'
        RETURNING content
        "#,
        prompt_id.as_uuid(),
        task_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(not_editable)?;
    let entry = NewAuditEntry::new(
        actor.clone(),
        Vec::new(),
        "message.remove",
        "task_message",
        prompt_id.to_string(),
    )
    .with_details(&serde_json::json!({ "task_id": task_id, "text": plain_text(old)? }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::TaskMessageChanged { task_id }).await?;
    apply_task_event(&mut tx, task_id, TaskEvent::MessageWithdrawn, &actor).await?;
    tx.commit().await?;
    Ok(())
}

fn not_editable() -> DbError {
    DbError::Conflict("the message was already received or cannot be changed".into())
}

/// Concatenated text blocks of stored message content, for audit details.
fn plain_text(content: serde_json::Value) -> Result<String, DbError> {
    let blocks: Vec<ContentBlock> = serde_json::from_value(content)?;
    Ok(blocks
        .iter()
        .filter_map(|b| match &b.kind {
            Some(content_block::Kind::Text(t)) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Sets a task's title to the sanitized `title` if `source` may replace the current title's
/// source (`TitleSource::may_replace`; the rule lives in the `WHERE` so concurrent writers cannot
/// violate it). Returns whether the title changed and publishes `task_changed` if it did.
pub async fn set_task_title(
    pool: &PgPool,
    task_id: TaskId,
    title: &str,
    source: TitleSource,
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let changed = set_title_in(&mut tx, task_id, title, source).await?;
    tx.commit().await?;
    Ok(changed)
}

/// A board member renames a task: the title is stored with source `user`, the change is audited
/// as `task.rename` with the old and new title and `task_changed` is published.
pub async fn rename_task(
    pool: &PgPool,
    task_id: TaskId,
    actor: Principal,
    title: &str,
) -> Result<(), DbError> {
    if title_too_long(title) {
        return Err(DbError::InvalidData(format!(
            "title is longer than {TITLE_MAX_CHARS} characters"
        )));
    }
    let mut tx = pool.begin().await?;
    let old: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("task `{task_id}` not found")))?;
    set_title_in(&mut tx, task_id, title, TitleSource::User).await?;
    let new = sanitize_title(title).unwrap_or_default();
    let entry = NewAuditEntry::new(
        actor,
        Vec::new(),
        "task.rename",
        "task",
        task_id.to_string(),
    )
    .with_details(&serde_json::json!({ "old_title": old, "new_title": new }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(())
}

/// A board member marks a task done: it becomes `completed`, an idle run (`awaiting_input`)
/// becomes `finalizing` so the replica holding its stream cancels it (no prompt reaches it from
/// here on), the change is audited as `task.done` and `task_changed` and `run_changed` are
/// published. Pending agent requests of the task are closed (`task-completed`). Tasks that are
/// `queued`, `running` or already `completed` are rejected with [`DbError::Conflict`].
pub async fn mark_task_done(
    pool: &PgPool,
    task_id: TaskId,
    actor: Principal,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    apply_task_event(
        &mut tx,
        task_id,
        TaskEvent::MarkedDone(DoneBy::Board),
        &actor,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A board member stops the active run of a task (design D1). The run gets the stop marker; a
/// connected run (`preparing`, `running`, `awaiting_input`) also becomes `finalizing`, which makes
/// the replica holding its stream send `Cancel`. A run that is not connected yet is torn down by
/// the scheduler, which follows `run_changed`. The task becomes `waiting` when the run reports.
/// Audited as `run.stop`. Stopping a stopped run changes nothing; a task without an active run
/// is rejected with [`DbError::Conflict`].
pub async fn stop_run(pool: &PgPool, task_id: TaskId, actor: Principal) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    lock_runs_then_task(&mut tx, task_id).await?;
    if let Some(run_id) = request_stop_tx(&mut tx, task_id, StopScope::ActiveOnly).await? {
        let entry = NewAuditEntry::new(actor, Vec::new(), "run.stop", "run", run_id.to_string())
            .with_details(&serde_json::json!({ "task_id": task_id }))?;
        write_audit_entry(&mut *tx, &entry).await?;
    } else if !has_active_run(&mut tx, task_id).await? {
        return Err(DbError::Conflict("the task has no active run".into()));
    }
    tx.commit().await?;
    Ok(())
}

/// A board member cancels a task (design D2): one transaction sets `cancelled`, discards the
/// undelivered messages, closes the pending requests (`task-cancelled`), stops the active run
/// as in [`stop_run`] and audits `task.cancel`. A `completed` or `cancelled` task is rejected
/// with [`DbError::Conflict`]. The PR and the issue are not touched.
pub async fn cancel_task(pool: &PgPool, task_id: TaskId, actor: Principal) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    apply_task_event(&mut tx, task_id, TaskEvent::Cancelled, &actor).await?;
    tx.commit().await?;
    Ok(())
}

/// Lock order everywhere: run rows, then the task row (see `lock_task` in df-agent-channel).
pub(crate) async fn lock_runs_then_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
) -> Result<(), DbError> {
    sqlx::query("SELECT 1 FROM runs WHERE task_id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Sets the stop marker on the task's active run if it has none and publishes `run_changed`.
/// A `finalizing` run is ending already (`complete_task`, release): it is marked only when
/// `scope` is [`StopScope::IncludingFinalizing`] (cancelling the task), because the marker also
/// stops messages queued before the stop from requeuing the task. The caller holds the task row
/// lock; the stop time is the database clock ([`clock_under_task_lock`]), like `queued_at`.
/// Returns the run when this call stopped it.
pub(crate) async fn request_stop_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
    scope: StopScope,
) -> Result<Option<RunId>, DbError> {
    let run: Option<Uuid> = sqlx::query_scalar(
        "UPDATE runs SET stop_requested_at = clock_timestamp(),
             state = CASE WHEN state IN ('preparing', 'running', 'awaiting_input')
                          THEN 'finalizing' ELSE state END
         WHERE task_id = $1 AND stop_requested_at IS NULL
           AND state NOT IN ('succeeded', 'failed', 'cancelled')
           AND ($2 OR state <> 'finalizing')
         RETURNING id",
    )
    .bind(task_id.as_uuid())
    .bind(scope == StopScope::IncludingFinalizing)
    .fetch_optional(&mut **tx)
    .await?;
    let run_id = run.map(RunId::from_uuid);
    if let Some(run_id) = run_id {
        publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await?;
    }
    Ok(run_id)
}

pub(crate) async fn has_active_run(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs
         WHERE task_id = $1 AND state NOT IN ('succeeded', 'failed', 'cancelled'))",
    )
    .bind(task_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?)
}

/// Asks a persona to name a task that has no title from a person.
fn ask_for_title() -> String {
    format!(
        "Before you start, call the `set_task_title` tool with a short title (at most {TITLE_MAX_CHARS} characters) for this task."
    )
}

async fn set_title_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
    title: &str,
    source: TitleSource,
) -> Result<bool, DbError> {
    let title = sanitize_title(title).ok_or_else(|| DbError::InvalidData("empty title".into()))?;
    let changed = sqlx::query(
        "UPDATE tasks SET title = $2, title_source = $3, updated_at = now()
         WHERE id = $1 AND CASE $3
             WHEN 'user' THEN TRUE
             WHEN 'generated' THEN title_source IN ('placeholder', 'agent')
             WHEN 'agent' THEN title_source = 'placeholder'
             ELSE FALSE
         END",
    )
    .bind(task_id.as_uuid())
    .bind(&title)
    .bind(source.as_str())
    .execute(&mut **tx)
    .await?
    .rows_affected()
        > 0;
    if changed {
        publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
    }
    Ok(changed)
}

#[cfg(test)]
mod dependency_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
pub(crate) mod tests;
