//! The task tree (`task-tree`): the effective base branch, open subtasks, base task resolution
//! and the delegation limits.

use df_core::config::TasksConfig;
use df_core::task_transition::TaskEvent;
use df_core::{Principal, TaskId, TaskKey, TaskStatus};
use sqlx::{PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use super::PR_SYNC_KIND;
use crate::pool::DbError;
use crate::task_events::apply_task_event;
use crate::task_views::status_list;

/// Which delegation limit a creation hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DelegationLimit {
    #[error(
        "the limit `tasks.maxDepth` ({0}) is reached: a task may have at most {0} ancestors, so this task cannot create subtasks"
    )]
    Depth(u32),
    #[error(
        "the limit `tasks.maxOpenSubtasks` ({0}) is reached: this task has {0} open subtasks; wait for some to finish"
    )]
    OpenSubtasks(u32),
}

/// A subtask that holds back its parent's completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSubtask {
    pub key: TaskKey,
    pub pr_url: Option<String>,
}

/// The effective base branch of the task (`task-tree` "Effective base branch"): the branch of its
/// base task, or, once the base task is Done (`completed`), that task's own base branch, applied
/// recursively. Without a base task: the base of the PR the task is attached to
/// (`tasks.base_branch`), else the project's.
pub async fn base_branch(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
) -> Result<String, DbError> {
    // The recursion follows Done base tasks only, so the last row's base task is open or absent.
    let branch = sqlx::query_scalar!(
        r#"WITH RECURSIVE chain AS (
             SELECT id, project_id, base_task_id, base_branch, 0 AS depth
             FROM tasks WHERE id = $1
             UNION ALL
             SELECT b.id, b.project_id, b.base_task_id, b.base_branch, c.depth + 1
             FROM chain c JOIN tasks b ON b.id = c.base_task_id
             WHERE b.status = 'completed'
         )
         SELECT COALESCE(open_base.branch, c.base_branch, p.base_branch) AS "branch!"
         FROM (SELECT * FROM chain ORDER BY depth DESC LIMIT 1) c
         JOIN projects p ON p.id = c.project_id
         LEFT JOIN tasks open_base ON open_base.id = c.base_task_id"#,
        task_id.as_uuid()
    )
    .fetch_optional(executor)
    .await?;
    branch.ok_or_else(|| DbError::NotFound(format!("task `{task_id}` not found")))
}

/// The direct subtasks of `parent` that hold back its completion (`task-tree` "Open subtasks"),
/// by task number. A subtask based on its parent also counts while its pull request is unmerged,
/// and while the job that opens it is still pending: the run end wakes the parent before that
/// job ran, so without the second rule `complete_task` could pass in that window.
pub async fn open_subtasks(
    executor: impl PgExecutor<'_>,
    parent: TaskId,
) -> Result<Vec<OpenSubtask>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT t.key, t.pr_url FROM tasks t
         WHERE t.parent_id = $1
           AND (t.status <> ALL($2)
                OR (t.base_task_id = t.parent_id AND t.status <> ALL($3)
                    AND (t.pr_number IS NOT NULL
                         OR EXISTS (
                             SELECT 1 FROM outbox o
                             WHERE o.kind = $4 AND o.payload ->> 'task_id' = t.id::text
                               AND (o.payload ->> 'open')::boolean
                               AND o.done_at IS NULL AND o.gave_up_at IS NULL))))
         ORDER BY t.number"#,
        parent.as_uuid(),
        &status_list(TaskStatus::SUBTASK_FINISHED),
        &status_list(TaskStatus::CLOSED),
        PR_SYNC_KIND
    )
    .fetch_all(executor)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(OpenSubtask {
                key: row.key.parse()?,
                pr_url: row.pr_url,
            })
        })
        .collect()
}

/// Cancels `root` and each of its descendants that is not `completed`, `failed` or `cancelled`
/// in the caller's transaction (`task-tree` "Cancelling a subtree"), top-down: the run rows of the
/// whole subtree are locked first, then its task rows (the global order "runs, then task" of
/// every other transaction), and `Cancelled` is applied parent before child, so a child's
/// `NotifyParent` finds its parent cancelled and wakes nobody. Only the root's parent is woken.
pub(super) async fn cancel_subtree(
    tx: &mut Transaction<'_, Postgres>,
    root: TaskId,
    actor: &Principal,
) -> Result<(), DbError> {
    let candidates = subtree(tx, root).await?;
    let ids: Vec<Uuid> = candidates.iter().map(|task| task.id).collect();
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM runs WHERE task_id = ANY($1) ORDER BY id FOR UPDATE"#,
        &ids
    )
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM tasks WHERE id = ANY($1) ORDER BY id FOR UPDATE"#,
        &ids
    )
    .fetch_all(&mut **tx)
    .await?;
    // The statuses are read again under the locks: a descendant may have finished meanwhile.
    for task in subtree(tx, root).await? {
        let via = (task.depth > 0).then(|| task.root_key.clone());
        apply_task_event(
            tx,
            TaskId::from_uuid(task.id),
            TaskEvent::Cancelled { via },
            actor,
        )
        .await?;
    }
    Ok(())
}

struct SubtreeTask {
    id: Uuid,
    depth: i32,
    root_key: String,
}

/// `root` and its descendants (at any depth) that are not `completed`, `failed` or `cancelled`,
/// parents before children. The root is always listed: a final root is rejected by its event.
async fn subtree(conn: &mut sqlx::PgConnection, root: TaskId) -> Result<Vec<SubtreeTask>, DbError> {
    Ok(sqlx::query_as!(
        SubtreeTask,
        r#"WITH RECURSIVE sub AS (
             SELECT id, status, key AS root_key, 0 AS depth FROM tasks WHERE id = $1
             UNION ALL
             SELECT t.id, t.status, s.root_key, s.depth + 1
             FROM tasks t JOIN sub s ON t.parent_id = s.id
         )
         SELECT id AS "id!", depth AS "depth!", root_key AS "root_key!" FROM sub
         WHERE depth = 0 OR status <> ALL($2)
         ORDER BY depth, id"#,
        root.as_uuid(),
        &status_list(TaskStatus::DONE)
    )
    .fetch_all(conn)
    .await?)
}

/// What `merge_subtask_pr` checks about a subtask and its project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskToMerge {
    pub id: TaskId,
    pub parent_id: Option<TaskId>,
    pub base_task_id: Option<TaskId>,
    pub status: TaskStatus,
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
    /// The project's `subtask_merge` setting.
    pub policy: df_core::SubtaskMerge,
}

/// The task `key` of `project` with the facts the parent's merge of its PR depends on; `None` for
/// an unknown key.
pub async fn subtask_to_merge(
    executor: impl PgExecutor<'_>,
    project: df_core::ProjectId,
    key: &TaskKey,
) -> Result<Option<SubtaskToMerge>, DbError> {
    let row = sqlx::query!(
        "SELECT t.id, t.parent_id, t.base_task_id, t.status, t.pr_number, t.pr_url, p.subtask_merge
         FROM tasks t JOIN projects p ON p.id = t.project_id
         WHERE t.project_id = $1 AND t.key = $2",
        project.as_uuid(),
        key.to_string()
    )
    .fetch_optional(executor)
    .await?;
    row.map(|row| {
        let pr_number = row
            .pr_number
            .map(|n| {
                u64::try_from(n)
                    .map_err(|_negative| DbError::InvalidData(format!("pull request number {n}")))
            })
            .transpose()?;
        Ok(SubtaskToMerge {
            id: TaskId::from_uuid(row.id),
            parent_id: row.parent_id.map(TaskId::from_uuid),
            base_task_id: row.base_task_id.map(TaskId::from_uuid),
            status: row.status.parse()?,
            pr_number,
            pr_url: row.pr_url,
            policy: row.subtask_merge.parse()?,
        })
    })
    .transpose()
}

/// Locks the run rows and then the task row of `task` (the global order, design D6) and returns
/// its status; `None` for an unknown task.
pub async fn lock_task_status(
    tx: &mut Transaction<'_, Postgres>,
    task: TaskId,
) -> Result<Option<TaskStatus>, DbError> {
    super::lock_runs_then_task(tx, task).await?;
    let status = sqlx::query_scalar!("SELECT status FROM tasks WHERE id = $1", task.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;
    Ok(status.map(|s| s.parse()).transpose()?)
}

/// The id of the base task `key` of a new task in `project`: a task of the project that is not
/// Done, failed or cancelled. A chat has no branch to build on, so it is no base task.
pub(super) async fn resolve_base_task(
    tx: &mut Transaction<'_, Postgres>,
    project: df_core::ProjectId,
    key: &TaskKey,
) -> Result<TaskId, DbError> {
    if key.is_chat() {
        return Err(DbError::InvalidBaseTask(key.to_string()));
    }
    let id = sqlx::query_scalar!(
        "SELECT id FROM tasks WHERE project_id = $1 AND key = $2 AND status <> ALL($3)",
        project.as_uuid(),
        key.to_string(),
        &status_list(TaskStatus::DONE)
    )
    .fetch_optional(&mut **tx)
    .await?;
    id.map(TaskId::from_uuid)
        .ok_or_else(|| DbError::InvalidBaseTask(key.to_string()))
}

/// Checks `limits` for a task created by `parent`. The project row lock of the key allocation
/// serializes creations, so the counts cannot change under it.
pub(super) async fn check_limits(
    tx: &mut Transaction<'_, Postgres>,
    parent: TaskId,
    limits: TasksConfig,
) -> Result<(), DbError> {
    let ancestors = sqlx::query_scalar!(
        r#"WITH RECURSIVE chain AS (
             SELECT parent_id FROM tasks WHERE id = $1
             UNION ALL
             SELECT t.parent_id FROM chain c JOIN tasks t ON t.id = c.parent_id
         )
         SELECT count(*) AS "ancestors!" FROM chain WHERE parent_id IS NOT NULL"#,
        parent.as_uuid()
    )
    .fetch_one(&mut **tx)
    .await?;
    // The parent's ancestors plus the parent itself.
    if ancestors + 1 > i64::from(limits.max_depth) {
        return Err(DelegationLimit::Depth(limits.max_depth).into());
    }
    let open = open_subtasks(&mut **tx, parent).await?.len();
    if open >= limits.max_open_subtasks as usize {
        return Err(DelegationLimit::OpenSubtasks(limits.max_open_subtasks).into());
    }
    Ok(())
}
