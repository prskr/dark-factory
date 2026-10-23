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
        None if description.is_empty() => ask_for_title(),
        None => format!("{description}\n\n{}", ask_for_title()),
    };
    vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text })),
    }]
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

    // Lock the project row and allocate the next gap-free task number.
    let project_alloc = sqlx::query!(
        r#"
        UPDATE projects
        SET next_task_number = next_task_number + 1,
            updated_at = now()
        WHERE id = $1
        RETURNING key, next_task_number - 1 AS "allocated_number!"
        "#,
        input.project_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("project `{}` not found", input.project_id)))?;

    let project_key =
        ProjectKey::new(project_alloc.key).map_err(|e| DbError::InvalidData(e.to_string()))?;
    let number = u64::try_from(project_alloc.allocated_number)
        .map_err(|_| DbError::InvalidData("negative task number".into()))?;
    let task_key =
        TaskKey::new(project_key, number).map_err(|e| DbError::InvalidData(e.to_string()))?;

    let user_title = input.title.as_deref().and_then(sanitize_title);
    let (title, title_source) = match &user_title {
        Some(title) => (title.clone(), TitleSource::User),
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
        project_alloc.allocated_number,
        key_str,
        title,
        title_source.as_str(),
        input.description,
        input.persona_id.as_uuid(),
        input.issue_number,
        status.as_str(),
        created_by_uuid
    )
    .fetch_one(&mut *tx)
    .await?;

    let prompt_id = PromptId::new();
    let author = Principal::system("task.assigned");
    let content = format_assignment_prompt(user_title.as_deref(), &row.description);
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
    .fetch_one(&mut *tx)
    .await?;

    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    publish(&mut *tx, &Notification::TaskMessageQueued { task_id }).await?;

    tx.commit().await?;

    let task_row = TaskRow {
        id: TaskId::from_uuid(row.id),
        project_id: ProjectId::from_uuid(row.project_id),
        number,
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
    };

    let message_row = TaskMessageRow {
        id: PromptId::from_uuid(msg_row.id),
        task_id: TaskId::from_uuid(msg_row.task_id),
        author,
        content,
        queued_at: msg_row.queued_at,
        delivered_run_id: msg_row.delivered_run_id.map(RunId::from_uuid),
        delivered_at: msg_row.delivered_at,
        forwarded_at: None,
    };

    Ok((task_row, message_row))
}

/// Queues a message on a task.
///
/// If the task is not currently `running` (i.e. `waiting`, `failed`, `completed`,
/// `rate_limited`, or `limit_reached`), its status transitions to `queued` so the run
/// scheduler starts a new run (`iter1-run-orchestration` spec: *"the task is `rate_limited`
/// and no new run starts until a message is queued"*; `iter1-demo-e2e` spec: *"the task is
/// `rate_limited`, and a new message starts a run that continues the work"*). If the task is
/// `running`, it remains `running` so the active run picks up the message on its next turn.
pub async fn queue_message(
    pool: &PgPool,
    task_id: TaskId,
    author: Principal,
    content: Vec<ContentBlock>,
) -> Result<TaskMessageRow, DbError> {
    let mut tx = pool.begin().await?;

    let prompt_id = PromptId::new();
    let author_json = serde_json::to_value(&author)?;
    let content_json = serde_json::to_value(&content)?;

    let updated = sqlx::query!(
        r#"
        UPDATE tasks
        SET status = CASE
                WHEN status = 'running' THEN 'running'
                ELSE 'queued'
            END,
            updated_at = now()
        WHERE id = $1
        RETURNING id
        "#,
        task_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
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
    .fetch_one(&mut *tx)
    .await?;

    publish(&mut *tx, &Notification::TaskMessageQueued { task_id }).await?;
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;

    tx.commit().await?;

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
            title_source: r
                .title_source
                .parse()
                .map_err(|e: df_core::ParseEnumError| DbError::InvalidData(e.to_string()))?,
            description: r.description,
            persona_id: PersonaId::from_uuid(r.persona_id),
            issue_number: r.issue_number,
            status: r
                .status
                .parse()
                .map_err(|e: df_core::ParseEnumError| DbError::InvalidData(e.to_string()))?,
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
    let new = serde_json::to_value(vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: text.into() })),
    }])?;
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
mod tests {
    use super::*;
    use crate::notify::Notifier;
    use std::collections::HashSet;
    use std::time::Duration;
    use uuid::Uuid;

    async fn seed_project_and_persona(
        pool: &PgPool,
        project_key: &ProjectKey,
        persona_name: &str,
    ) -> Result<(ProjectId, PersonaId), DbError> {
        let project_id = ProjectId::new();
        let persona_id = PersonaId::new();
        let empty_cmd: Vec<String> = Vec::new();

        sqlx::query(
            r#"
            INSERT INTO projects (id, key, name, repo_url, image, base_branch)
            VALUES ($1, $2, $3, $4, $5, 'main')
            "#,
        )
        .bind(project_id.as_uuid())
        .bind(project_key.as_str())
        .bind(format!("Project {project_key}"))
        .bind("https://forge.example.test/prskr/dark-factory.git")
        .bind("harbor.example.test/df/project:latest")
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO personas (id, name, system_prompt, harness, adapter_command)
            VALUES ($1, $2, $3, 'claude-code', $4)
            "#,
        )
        .bind(persona_id.as_uuid())
        .bind(persona_name)
        .bind("You are Linus, a careful software engineer.")
        .bind(&empty_cmd)
        .execute(pool)
        .await?;

        Ok((project_id, persona_id))
    }

    #[tokio::test]
    async fn first_task_gets_df_1_and_queues_assignment_prompt() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let project_key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
            .await
            .unwrap();

        let (task, msg) = create_task(
            &pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("Fix login".into()),
                description: "Ensure OIDC callback validates state.".into(),
                issue_number: Some(42),
                created_by: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(task.key.to_string(), "DF-1");
        assert_eq!(task.number, 1);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.title, "Fix login");
        assert_eq!(task.issue_number, Some(42));

        assert_eq!(msg.task_id, task.id);
        assert_eq!(msg.author, Principal::system("task.assigned"));
        assert!(msg.delivered_at.is_none());
        assert!(msg.delivered_run_id.is_none());

        let undelivered = list_undelivered_messages(&pool, task.id).await.unwrap();
        assert_eq!(undelivered.len(), 1);
        assert_eq!(undelivered[0].id, msg.id);
    }

    #[tokio::test]
    async fn concurrent_task_creation_allocates_consecutive_gap_free_keys() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let project_key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
            .await
            .unwrap();

        let mut handles = Vec::new();
        for i in 0..8 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                create_task(
                    &pool,
                    NewTask {
                        project_id,
                        persona_id,
                        title: Some(format!("Concurrent task {i}")),
                        description: String::new(),
                        issue_number: None,
                        created_by: None,
                    },
                )
                .await
                .expect("concurrent create_task should succeed")
                .0
            }));
        }

        let mut keys = HashSet::new();
        let mut numbers = Vec::new();
        for h in handles {
            let t = h.await.unwrap();
            keys.insert(t.key.to_string());
            numbers.push(t.number);
        }
        numbers.sort_unstable();
        assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        for n in 1..=8 {
            assert!(keys.contains(&format!("DF-{n}")));
        }
    }

    #[tokio::test]
    async fn queueing_message_on_waiting_or_failed_or_rate_limited_task() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let notifier = Notifier::start(&pool).await.unwrap();
        let mut rx = notifier.subscribe();

        let project_key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
            .await
            .unwrap();

        let (task, initial_prompt) = create_task(
            &pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("Initial work".into()),
                description: "Desc".into(),
                issue_number: None,
                created_by: None,
            },
        )
        .await
        .unwrap();

        // Drain initial creation notifications
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();

        // Create a dummy run to mark the initial prompt delivered
        let run_id = RunId::new();
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
            .bind(run_id.as_uuid())
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            mark_prompt_delivered(&pool, initial_prompt.id, run_id)
                .await
                .unwrap()
        );
        assert!(
            list_undelivered_messages(&pool, task.id)
                .await
                .unwrap()
                .is_empty()
        );

        // Transition task to `waiting`, then queue a follow-up message
        update_task_status(&pool, task.id, TaskStatus::Waiting)
            .await
            .unwrap();
        let _ = rx.recv().await.unwrap(); // task_changed from update_task_status

        let author = Principal::human(UserId::new(), "Peter");
        let content = vec![ContentBlock {
            kind: Some(content_block::Kind::Text(TextContent {
                text: "also add a test for X".into(),
            })),
        }];
        let follow_up = queue_message(&pool, task.id, author.clone(), content)
            .await
            .unwrap();
        assert!(follow_up.delivered_at.is_none());

        let reloaded = get_task(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(reloaded.status, TaskStatus::Queued);

        let n1 = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n1, Notification::TaskMessageQueued { task_id: task.id });
        let _ = rx.recv().await.unwrap(); // task_changed from queue_message

        // Transition task to `failed`, then queue another message -> becomes `queued`
        update_task_status(&pool, task.id, TaskStatus::Failed)
            .await
            .unwrap();
        queue_message(&pool, task.id, author.clone(), vec![])
            .await
            .unwrap();
        let reloaded_after_failed = get_task(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(reloaded_after_failed.status, TaskStatus::Queued);

        // Per `iter1-run-orchestration` ("no new run starts until a message is queued")
        // and `iter1-demo-e2e` ("the task is `rate_limited`, and a new message starts a run
        // that continues the work"), queueing a new message on a `rate_limited` task moves
        // it to `queued`.
        update_task_status(&pool, task.id, TaskStatus::RateLimited)
            .await
            .unwrap();
        queue_message(&pool, task.id, author, vec![]).await.unwrap();
        let reloaded_after_rate_limited = get_task(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(reloaded_after_rate_limited.status, TaskStatus::Queued);

        // If `tasks.number` and `tasks.key` disagree in the row, `get_task` returns an error
        // rather than silently returning inconsistent data.
        sqlx::query("UPDATE tasks SET number = 99 WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let mismatched = get_task(&pool, task.id).await;
        assert!(matches!(mismatched, Err(DbError::InvalidData(_))));
    }

    #[tokio::test]
    async fn database_check_constraints_reject_invalid_project_keys_task_statuses_and_duplicate_active_runs()
     {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        // Invalid project keys `df` and `D` rejected by DB check constraint
        for bad_key in ["df", "D"] {
            let res = sqlx::query(
                "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, $2, 'Bad', 'https://example.test/r.git', 'img:latest')",
            )
            .bind(Uuid::now_v7())
            .bind(bad_key)
            .execute(&pool)
            .await;
            assert!(
                res.is_err(),
                "DB should reject invalid project key `{bad_key}`"
            );
        }

        let project_key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
            .await
            .unwrap();
        let (task, _) = create_task(
            &pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("Check constraints".into()),
                description: String::new(),
                issue_number: None,
                created_by: None,
            },
        )
        .await
        .unwrap();

        // Invalid task status rejected by DB check constraint
        let bad_status = sqlx::query("UPDATE tasks SET status = 'bogus_status' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await;
        assert!(bad_status.is_err(), "DB should reject invalid task status");

        // Partial unique index: at most one active run per task
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
            .bind(Uuid::now_v7())
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        let second_active = sqlx::query(
            "INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 2, 'pending')",
        )
        .bind(Uuid::now_v7())
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await;
        assert!(
            second_active.is_err(),
            "partial unique index should reject second active run on same task"
        );
    }

    fn new_task(project_id: ProjectId, persona_id: PersonaId, title: Option<&str>) -> NewTask {
        NewTask {
            project_id,
            persona_id,
            title: title.map(str::to_string),
            description: "The nightly backup job sometimes runs twice. It started last week."
                .into(),
            issue_number: None,
            created_by: None,
        }
    }

    fn prompt_text(msg: &TaskMessageRow) -> String {
        match &msg.content[0].kind {
            Some(content_block::Kind::Text(t)) => t.text.clone(),
            other => panic!("unexpected content {other:?}"),
        }
    }

    #[tokio::test]
    async fn task_without_title_gets_a_placeholder_and_asks_for_a_title() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();

        let (task, msg) = create_task(&pool, new_task(project_id, persona_id, None))
            .await
            .unwrap();

        assert_eq!(task.title, "The nightly backup job sometimes runs twice.");
        assert_eq!(task.title_source, TitleSource::Placeholder);
        let text = prompt_text(&msg);
        assert!(text.contains("The nightly backup job sometimes runs twice. It started"));
        assert!(text.contains("set_task_title"));
    }

    #[tokio::test]
    async fn task_with_title_keeps_it_and_the_old_prompt() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();

        let (task, msg) = create_task(&pool, new_task(project_id, persona_id, Some("Fix backups")))
            .await
            .unwrap();

        assert_eq!(task.title, "Fix backups");
        assert_eq!(task.title_source, TitleSource::User);
        let text = prompt_text(&msg);
        assert!(text.starts_with("# Fix backups\n\n"));
        assert!(!text.contains("set_task_title"));
        assert_eq!(get_task(&pool, task.id).await.unwrap().unwrap(), task);
    }

    #[tokio::test]
    async fn title_precedence_holds_for_every_source_pair() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();

        for current in TitleSource::ALL {
            for new in TitleSource::ALL {
                let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
                    .await
                    .unwrap();
                sqlx::query("UPDATE tasks SET title = 'current', title_source = $2 WHERE id = $1")
                    .bind(task.id.as_uuid())
                    .bind(current.as_str())
                    .execute(&pool)
                    .await
                    .unwrap();

                let changed = set_task_title(&pool, task.id, "new", *new).await.unwrap();

                let stored = get_task(&pool, task.id).await.unwrap().unwrap();
                assert_eq!(changed, new.may_replace(*current), "{new} over {current}");
                if changed {
                    assert_eq!((stored.title.as_str(), stored.title_source), ("new", *new));
                } else {
                    assert_eq!(
                        (stored.title.as_str(), stored.title_source),
                        ("current", *current)
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn agent_title_is_sanitized_and_empty_is_rejected() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();
        let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
            .await
            .unwrap();

        assert!(
            set_task_title(&pool, task.id, " \n ", TitleSource::Agent)
                .await
                .is_err()
        );
        set_task_title(
            &pool,
            task.id,
            "Fix login\n\nand logout",
            TitleSource::Agent,
        )
        .await
        .unwrap();

        let stored = get_task(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(stored.title, "Fix login and logout");
    }

    #[tokio::test]
    async fn rename_rejects_a_title_over_the_limit() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();
        let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("Keep")))
            .await
            .unwrap();

        let long = "x".repeat(TITLE_MAX_CHARS + 1);
        let result = rename_task(
            &pool,
            task.id,
            Principal::human(UserId::new(), "Ada"),
            &long,
        )
        .await;

        assert!(matches!(result, Err(DbError::InvalidData(_))));
        assert_eq!(
            get_task(&pool, task.id).await.unwrap().unwrap().title,
            "Keep"
        );
    }

    #[tokio::test]
    async fn rename_locks_the_title_and_is_audited() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();
        let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
            .await
            .unwrap();
        let notifier = Notifier::start(&pool).await.unwrap();
        let mut events = notifier.subscribe();

        rename_task(
            &pool,
            task.id,
            Principal::human(UserId::new(), "Ada"),
            "Stop duplicate nightly backups",
        )
        .await
        .unwrap();
        let agent_changed = set_task_title(&pool, task.id, "Agent title", TitleSource::Agent)
            .await
            .unwrap();

        let stored = get_task(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(stored.title, "Stop duplicate nightly backups");
        assert_eq!(stored.title_source, TitleSource::User);
        assert!(!agent_changed);
        let (action, details): (String, serde_json::Value) = sqlx::query_as(
            "SELECT action, details FROM audit_log WHERE resource_id = $1 AND action = 'task.rename'",
        )
        .bind(task.id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(action, "task.rename");
        assert_eq!(
            details["old_title"],
            "The nightly backup job sometimes runs twice."
        );
        assert_eq!(details["new_title"], "Stop duplicate nightly backups");
        let changed = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(changed, Notification::TaskChanged { task_id: task.id });
    }
    async fn insert_run(pool: &PgPool, task: TaskId) -> RunId {
        let run_id = RunId::new();
        sqlx::query(
            "INSERT INTO runs (id, task_id, number, state)
             SELECT $1, $2, COALESCE(max(number), 0) + 1, 'succeeded' FROM runs WHERE task_id = $2",
        )
        .bind(run_id.as_uuid())
        .bind(task.as_uuid())
        .execute(pool)
        .await
        .unwrap();
        run_id
    }

    fn text(t: &str) -> Vec<ContentBlock> {
        vec![ContentBlock {
            kind: Some(content_block::Kind::Text(TextContent { text: t.into() })),
        }]
    }

    fn human() -> Principal {
        Principal::human(UserId::new(), "Ada")
    }

    /// A `waiting` task with one human message queued (status `queued`).
    async fn task_with_queued_message(pool: &PgPool) -> (TaskRow, TaskMessageRow) {
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(pool, &key, "Linus").await.unwrap();
        let (task, assignment) = create_task(pool, new_task(project_id, persona_id, Some("T")))
            .await
            .unwrap();
        mark_prompt_delivered(pool, assignment.id, insert_run(pool, task.id).await)
            .await
            .unwrap();
        update_task_status(pool, task.id, TaskStatus::Waiting)
            .await
            .unwrap();
        let msg = queue_message(pool, task.id, human(), text("first"))
            .await
            .unwrap();
        (task, msg)
    }

    async fn status_of(pool: &PgPool, task: TaskId) -> TaskStatus {
        get_task(pool, task).await.unwrap().unwrap().status
    }

    async fn audit_count(pool: &PgPool, action: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
            .bind(action)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn claim_marks_held_back_messages_forwarded_once_in_queue_order() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, first) = task_with_queued_message(&pool).await;
        let second = queue_message(&pool, task.id, human(), text("second"))
            .await
            .unwrap();

        let claimed = claim_held_back_messages(&pool, task.id).await.unwrap();
        let ids: Vec<_> = claimed.iter().map(|m| m.id).collect();
        let again = claim_held_back_messages(&pool, task.id).await.unwrap();

        assert_eq!(ids, vec![first.id, second.id]);
        assert!(claimed.iter().all(|m| m.forwarded_at.is_some()));
        assert!(again.is_empty());
        // Forwarded messages stay undelivered until their event is persisted.
        assert_eq!(
            list_undelivered_messages(&pool, task.id)
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn claim_skips_delivered_messages() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        mark_prompt_delivered(&pool, msg.id, insert_run(&pool, task.id).await)
            .await
            .unwrap();

        assert!(
            claim_held_back_messages(&pool, task.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn message_undelivered_before_the_migration_is_not_editable() {
        let pool = df_test_postgres::empty_pool().await;
        let before = crate::MIGRATOR
            .iter()
            .map(|m| m.version)
            .rev()
            .nth(1)
            .unwrap();
        crate::MIGRATOR.run_to(before, &pool).await.unwrap();
        let (task, msg) = task_with_queued_message_pre_migration(&pool).await;

        crate::MIGRATOR.run(&pool).await.unwrap();

        let err = edit_queued_message(&pool, task, msg, "new", human()).await;
        assert!(matches!(err, Err(DbError::Conflict(_))));
    }

    /// Same as [`task_with_queued_message`] for a schema without `forwarded_at`.
    async fn task_with_queued_message_pre_migration(pool: &PgPool) -> (TaskId, PromptId) {
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(pool, &key, "Linus").await.unwrap();
        let (task, _) = create_task(pool, new_task(project_id, persona_id, Some("T")))
            .await
            .unwrap();
        let msg = queue_message(pool, task.id, human(), text("old"))
            .await
            .unwrap();
        (task.id, msg.id)
    }

    #[tokio::test]
    async fn edit_replaces_text_keeps_id_and_audits() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        let notifier = Notifier::start(&pool).await.unwrap();
        let mut events = notifier.subscribe();

        edit_queued_message(&pool, task.id, msg.id, "also check logout", human())
            .await
            .unwrap();

        let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, msg.id);
        assert_eq!(stored[0].queued_at, msg.queued_at);
        assert_eq!(prompt_text(&stored[0]), "also check logout");
        assert_eq!(audit_count(&pool, "message.edit").await, 1);
        let details: serde_json::Value =
            sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'message.edit'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(details["old_text"], "first");
        assert_eq!(details["new_text"], "also check logout");
        let changed = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            changed,
            Notification::TaskMessageChanged { task_id: task.id }
        );
    }

    #[tokio::test]
    async fn edit_and_remove_are_refused_after_forwarding() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        claim_held_back_messages(&pool, task.id).await.unwrap();

        let edit = edit_queued_message(&pool, task.id, msg.id, "late", human()).await;
        let remove = remove_queued_message(&pool, task.id, msg.id, human()).await;

        assert!(matches!(edit, Err(DbError::Conflict(_))));
        assert!(matches!(remove, Err(DbError::Conflict(_))));
        let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
        assert_eq!(prompt_text(&stored[0]), "first");
        assert_eq!(audit_count(&pool, "message.edit").await, 0);
    }

    #[tokio::test]
    async fn empty_edit_is_refused() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;

        let err = edit_queued_message(&pool, task.id, msg.id, "  \n ", human()).await;

        assert!(matches!(err, Err(DbError::InvalidData(_))));
        let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
        assert_eq!(prompt_text(&stored[0]), "first");
    }

    #[tokio::test]
    async fn assignment_prompt_cannot_be_removed() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
            .await
            .unwrap();
        let (task, assignment) = create_task(&pool, new_task(project_id, persona_id, Some("T")))
            .await
            .unwrap();

        let err = remove_queued_message(&pool, task.id, assignment.id, human()).await;

        assert!(matches!(err, Err(DbError::Conflict(_))));
        assert_eq!(
            list_undelivered_messages(&pool, task.id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn remove_deletes_the_message_and_audits() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        queue_message(&pool, task.id, human(), text("second"))
            .await
            .unwrap();

        remove_queued_message(&pool, task.id, msg.id, human())
            .await
            .unwrap();

        let left = list_undelivered_messages(&pool, task.id).await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(prompt_text(&left[0]), "second");
        assert_eq!(audit_count(&pool, "message.remove").await, 1);
        // Other messages still queued.
        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn removing_the_last_message_returns_the_task_to_waiting() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);

        remove_queued_message(&pool, task.id, msg.id, human())
            .await
            .unwrap();

        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Waiting);
    }

    #[tokio::test]
    async fn removing_a_message_while_another_is_queued_keeps_the_task_queued() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, msg) = task_with_queued_message(&pool).await;
        // A concurrent `queue_message`: it holds the task row lock, its message not committed yet.
        let mut queueing = pool.begin().await.unwrap();
        sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&mut *queueing)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO task_messages (id, task_id, author, content) VALUES ($1, $2, $3, $4)",
        )
        .bind(PromptId::new().as_uuid())
        .bind(task.id.as_uuid())
        .bind(serde_json::to_value(human()).unwrap())
        .bind(serde_json::to_value(text("second")).unwrap())
        .execute(&mut *queueing)
        .await
        .unwrap();

        let remove = tokio::spawn({
            let pool = pool.clone();
            async move { remove_queued_message(&pool, task.id, msg.id, human()).await }
        });
        // Wait until the removal waits for the row lock, then let the queueing commit.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                if waiting > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        queueing.commit().await.unwrap();
        remove.await.unwrap().unwrap();

        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn sql_and_rust_agree_on_which_messages_are_editable() {
        // Edit and remove each get fresh rows, so they cannot interfere.
        for remove in [false, true] {
            let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
            let (task, held) = task_with_queued_message(&pool).await;
            let forwarded = queue_message(&pool, task.id, human(), text("forwarded"))
                .await
                .unwrap();
            sqlx::query("UPDATE task_messages SET forwarded_at = now() WHERE id = $1")
                .bind(forwarded.id.as_uuid())
                .execute(&pool)
                .await
                .unwrap();
            let delivered = queue_message(&pool, task.id, human(), text("delivered"))
                .await
                .unwrap();
            let run = insert_run(&pool, task.id).await;
            mark_prompt_delivered(&pool, delivered.id, run)
                .await
                .unwrap();
            let system = queue_message(&pool, task.id, Principal::system("x"), text("system"))
                .await
                .unwrap();

            for (message, editable) in [
                (held, true),
                (forwarded, false),
                (delivered, false),
                (system, false),
            ] {
                let row: TaskMessageRow = sqlx::query_as!(
                    MessageRecord,
                    r#"SELECT id, task_id, author, content, queued_at, delivered_run_id,
                              delivered_at, forwarded_at
                       FROM task_messages WHERE id = $1"#,
                    message.id.as_uuid()
                )
                .fetch_one(&pool)
                .await
                .unwrap()
                .try_into()
                .unwrap();
                assert_eq!(row.is_editable(), editable, "rust: {row:?}");
                let result = if remove {
                    remove_queued_message(&pool, task.id, message.id, human()).await
                } else {
                    edit_queued_message(&pool, task.id, message.id, "x", human()).await
                };
                assert_eq!(
                    result.is_ok(),
                    editable,
                    "sql (remove: {remove}): {message:?}"
                );
            }
        }
    }
}
