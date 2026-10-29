//! Task management persistence operations (`specs/task-management/spec.md`).

use chrono::{DateTime, Utc};
use df_core::{
    FALLBACK_TITLE, PersonaId, Principal, ProjectId, ProjectKey, PromptId, RunId, TITLE_MAX_CHARS,
    TaskId, TaskKey, TaskStatus, TitleSource, UserId, placeholder_title, sanitize_title,
    title_too_long,
};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::notify::{Notification, publish};
use crate::pool::DbError;

/// Parameters for creating a new task.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    /// Title given by the creator; without one the first sentence of the description is used.
    pub title: Option<String>,
    pub description: String,
    pub issue_number: Option<i64>,
    pub created_by: Option<UserId>,
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
    pub created_by: Option<UserId>,
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

fn text_block(text: String, hidden: bool) -> ContentBlock {
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
    let (task_key, number) = allocate_task_key(&mut tx, input.project_id).await?;
    let user_title = input.title.as_deref().and_then(sanitize_title);
    let task_row = insert_task(&mut tx, &input, task_key, number, user_title.as_deref()).await?;
    let message_row = enqueue_assignment_prompt(
        &mut tx,
        task_row.id,
        user_title.as_deref(),
        &task_row.description,
    )
    .await?;

    publish(
        &mut *tx,
        &Notification::TaskChanged {
            task_id: task_row.id,
        },
    )
    .await?;
    publish(
        &mut *tx,
        &Notification::TaskMessageQueued {
            task_id: task_row.id,
        },
    )
    .await?;
    tx.commit().await?;

    Ok((task_row, message_row))
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
        .map_err(|_| DbError::InvalidData("negative task number".into()))?;
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
) -> Result<TaskRow, DbError> {
    let (title, title_source) = match user_title {
        Some(title) => (title.to_string(), TitleSource::User),
        None => (
            placeholder_title(&input.description).unwrap_or_else(|| FALLBACK_TITLE.to_string()),
            TitleSource::Placeholder,
        ),
    };
    let task_id = TaskId::new();
    let status = TaskStatus::Queued;
    let key_str = task_key.to_string();
    let created_by_uuid = input.created_by.map(|u| u.as_uuid());

    let row = sqlx::query!(
        r#"
        INSERT INTO tasks (
            id, project_id, number, key, title, title_source, description,
            persona_id, issue_number, status, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING
            id, project_id, number, key, title, title_source, description,
            persona_id, issue_number, status, status_line,
            pr_number, pr_url, pr_title, pr_body,
            diverged, harness_session_id, created_by, created_at, updated_at
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
        created_by_uuid
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
        created_by: row.created_by.map(UserId::from_uuid),
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
    let author = Principal::system("task.assigned");
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
pub async fn queue_message_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: TaskId,
    author: Principal,
    content: Vec<ContentBlock>,
) -> Result<TaskMessageRow, DbError> {
    let prompt_id = PromptId::new();
    let author_json = serde_json::to_value(&author)?;
    let content_json = serde_json::to_value(&content)?;

    let updated = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = CASE
                WHEN EXISTS (
                    SELECT 1 FROM runs
                    WHERE runs.task_id = tasks.id
                      AND runs.state NOT IN ('finalizing', 'succeeded', 'failed', 'cancelled')
                ) THEN 'running'
                ELSE 'queued'
            END,
            updated_at = now()
        WHERE id = $1
        RETURNING id
        "#,
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;

    if updated.is_none() {
        return Err(DbError::NotFound(format!("task `{task_id}` not found")));
    }

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

    publish(&mut **tx, &Notification::TaskMessageQueued { task_id }).await?;
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;

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
            diverged, harness_session_id, created_by, created_at, updated_at
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
            created_by: r.created_by.map(UserId::from_uuid),
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
        WHERE task_id = $1 AND delivered_at IS NULL
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
          AND author->>'kind' = 'human'
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
          AND author->>'kind' = 'human'
        RETURNING content
        "#,
        prompt_id.as_uuid(),
        task_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(not_editable)?;
    let back_to_waiting = sqlx::query!(
        r#"
        UPDATE tasks SET status = 'waiting', updated_at = now()
        WHERE id = $1 AND status = 'queued'
          AND NOT EXISTS (SELECT 1 FROM task_messages WHERE task_id = $1 AND delivered_at IS NULL)
        "#,
        task_id.as_uuid()
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    let entry = NewAuditEntry::new(
        actor,
        Vec::new(),
        "message.remove",
        "task_message",
        prompt_id.to_string(),
    )
    .with_details(&serde_json::json!({ "task_id": task_id, "text": plain_text(old)? }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::TaskMessageChanged { task_id }).await?;
    if back_to_waiting {
        publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    }
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

/// Updates a task's status (`tasks.status`) and publishes `task_changed` in the same transaction.
pub async fn update_task_status(
    pool: &PgPool,
    task_id: TaskId,
    status: TaskStatus,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;

    let res = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = $2, updated_at = now()
        WHERE id = $1
        "#,
        task_id.as_uuid(),
        status.as_str()
    )
    .execute(&mut *tx)
    .await?;

    if res.rows_affected() == 0 {
        return Err(DbError::NotFound(format!("task `{task_id}` not found")));
    }

    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
    Ok(())
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
    // Lock order everywhere: run rows, then the task row (see `lock_task` in df-agent-channel).
    sqlx::query("SELECT 1 FROM runs WHERE task_id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut *tx)
        .await?;
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("task `{task_id}` not found")))?;
    let parsed: TaskStatus = status.parse()?;
    if !parsed.can_mark_done() {
        return Err(DbError::Conflict(format!(
            "a {status} task cannot be marked done"
        )));
    }
    sqlx::query("UPDATE tasks SET status = 'completed', updated_at = now() WHERE id = $1")
        .bind(task_id.as_uuid())
        .execute(&mut *tx)
        .await?;
    let idle_run: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE runs SET state = 'finalizing' WHERE task_id = $1 AND state = 'awaiting_input'
         RETURNING id",
    )
    .bind(task_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;
    let entry = NewAuditEntry::new(
        actor.clone(),
        Vec::new(),
        "task.done",
        "task",
        task_id.to_string(),
    )
    .with_details(&serde_json::json!({ "old_status": status }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    crate::requests::close_pending_for_task(&mut tx, task_id, "task-completed", &actor).await?;
    if let Some(run_id) = idle_run {
        publish(
            &mut *tx,
            &Notification::RunChanged {
                run_id: RunId::from_uuid(run_id),
                task_id,
            },
        )
        .await?;
    }
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
    Ok(())
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
mod tests;
