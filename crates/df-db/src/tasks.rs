//! Task management persistence operations (`specs/task-management/spec.md`).

use chrono::{DateTime, Utc};
use df_core::{
    PersonaId, Principal, ProjectId, ProjectKey, PromptId, RunId, TaskId, TaskKey, TaskStatus,
    UserId,
};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::notify::{Notification, publish};
use crate::pool::DbError;

/// Parameters for creating a new task.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    pub title: String,
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
}

/// Formats the initial assignment prompt text from a task's title and description.
pub fn format_assignment_prompt(title: &str, description: &str) -> Vec<ContentBlock> {
    let text = if description.trim().is_empty() {
        format!("# {title}")
    } else {
        format!("# {title}\n\n{description}")
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

    let task_id = TaskId::new();
    let status = TaskStatus::Queued;
    let key_str = task_key.to_string();
    let created_by_uuid = input.created_by.map(|u| u.as_uuid());

    let row = sqlx::query!(
        r#"
        INSERT INTO tasks (
            id, project_id, number, key, title, description,
            persona_id, issue_number, status, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING
            id, project_id, number, key, title, description,
            persona_id, issue_number, status, status_line,
            pr_number, pr_url, pr_title, pr_body,
            diverged, harness_session_id, created_by, created_at, updated_at
        "#,
        task_id.as_uuid(),
        input.project_id.as_uuid(),
        project_alloc.allocated_number,
        key_str,
        input.title,
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
    let content = format_assignment_prompt(&row.title, &row.description);
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
            id, project_id, number, key, title, description,
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

/// Lists all undelivered prompts for `task_id` in queue order.
pub async fn list_undelivered_messages(
    pool: &PgPool,
    task_id: TaskId,
) -> Result<Vec<TaskMessageRow>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, task_id, author, content, queued_at, delivered_run_id, delivered_at
        FROM task_messages
        WHERE task_id = $1 AND delivered_at IS NULL
        ORDER BY queued_at ASC, id ASC
        "#,
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            let author: Principal = serde_json::from_value(r.author)?;
            let content: Vec<ContentBlock> = serde_json::from_value(r.content)?;
            Ok(TaskMessageRow {
                id: PromptId::from_uuid(r.id),
                task_id: TaskId::from_uuid(r.task_id),
                author,
                content,
                queued_at: r.queued_at,
                delivered_run_id: r.delivered_run_id.map(RunId::from_uuid),
                delivered_at: r.delivered_at,
            })
        })
        .collect()
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
                title: "Fix login".into(),
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
                        title: format!("Concurrent task {i}"),
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
                title: "Initial work".into(),
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
                title: "Check constraints".into(),
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
}
