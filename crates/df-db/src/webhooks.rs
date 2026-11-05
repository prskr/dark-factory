//! Applying forge webhook events to tasks (`iter3-webhooks` design D3, D4, D8): a message per
//! event, or the completion of the task of a merged pull request.

use df_core::webhooks::{Target, WebhookEvent, WebhookWake, render_message};
use df_core::{Principal, ProjectId, StatusGroup, StatusSet, TaskId, TaskStatus};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::requests::close_pending_for_task;
use crate::task_views::statuses_of;
use crate::tasks::{StopScope, lock_runs_then_task, queue_message_tx, request_stop_tx, text_block};

/// `system` reason of the message.
pub const MESSAGE_REASON: &str = "webhook";
/// `system` reason of the `task.done` audit entry of a merged pull request.
pub const MERGED_REASON: &str = "pr.merged";

/// A project as webhook routing sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookProject {
    pub id: ProjectId,
    pub repo_url: String,
    pub wake: WebhookWake,
}

/// Every project; the caller matches the event's repository against `repo_url`.
pub async fn webhook_projects(pool: &PgPool) -> Result<Vec<WebhookProject>, DbError> {
    let rows = sqlx::query!("SELECT id, repo_url, webhook_wake FROM projects ORDER BY key")
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(WebhookProject {
                id: ProjectId::from_uuid(row.id),
                repo_url: row.repo_url,
                wake: row.webhook_wake.parse()?,
            })
        })
        .collect()
}

/// The project's tasks an event addresses: the task of a key, the tasks of a pull request, or
/// the open (not `completed`, `cancelled` or `failed`) tasks of an issue.
pub async fn target_tasks(
    pool: &PgPool,
    project_id: ProjectId,
    target: &Target,
) -> Result<Vec<TaskId>, DbError> {
    let ids = match target {
        Target::Task(key) => {
            sqlx::query_scalar!(
                "SELECT id FROM tasks WHERE project_id = $1 AND key = $2",
                project_id.as_uuid(),
                key.to_string(),
            )
            .fetch_all(pool)
            .await?
        }
        Target::PullRequest(number) => {
            let Ok(number) = i64::try_from(*number) else {
                return Ok(Vec::new());
            };
            sqlx::query_scalar!(
                "SELECT id FROM tasks WHERE project_id = $1 AND pr_number = $2",
                project_id.as_uuid(),
                number,
            )
            .fetch_all(pool)
            .await?
        }
        Target::Issue(number) => {
            let Ok(number) = i64::try_from(*number) else {
                return Ok(Vec::new());
            };
            sqlx::query_scalar!(
                "SELECT id FROM tasks WHERE project_id = $1 AND issue_number = $2
                   AND status <> ALL($3)",
                project_id.as_uuid(),
                number,
                &statuses_of(StatusSet::Done),
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(ids.into_iter().map(TaskId::from_uuid).collect())
}

/// Completes the task of a merged pull request (design D4) in the caller's transaction: records
/// the merge and, unless the task is `completed` or `cancelled`, sets `completed`, stops its
/// active run, closes its pending requests and audits `task.done` as [`MERGED_REASON`] with the
/// merging login. The pull request is left as it is. Returns `false` for a merge already seen
/// (a redelivery) or an unknown task.
pub async fn complete_on_merge_tx(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    login: &str,
) -> Result<bool, DbError> {
    lock_runs_then_task(tx, task_id).await?;
    let row = sqlx::query!(
        r#"SELECT status, pr_merged_at IS NOT NULL AS "merged!" FROM tasks WHERE id = $1"#,
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row.filter(|row| !row.merged) else {
        return Ok(false);
    };
    sqlx::query!(
        "UPDATE tasks SET pr_merged_at = now() WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    let old_status: TaskStatus = row.status.parse()?;
    if matches!(old_status, TaskStatus::Completed | TaskStatus::Cancelled) {
        return Ok(true);
    }
    sqlx::query!(
        "UPDATE tasks SET status = 'completed', resume_at = NULL, updated_at = now()
         WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    request_stop_tx(tx, task_id, StopScope::ActiveOnly).await?;
    let actor = Principal::system(MERGED_REASON);
    close_pending_for_task(tx, task_id, "task-completed", &actor).await?;
    let entry = NewAuditEntry::new(actor, Vec::new(), "task.done", "task", task_id.to_string())
        .with_details(&json!({ "old_status": old_status, "login": login }))?;
    write_audit_entry(&mut **tx, &entry).await?;
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
    Ok(true)
}

/// Queues the message for `event` on the task in the caller's transaction, unless the task is
/// `completed`, `cancelled` or `failed`. Returns whether a message was queued.
pub async fn queue_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    event: &WebhookEvent,
) -> Result<bool, DbError> {
    let status = sqlx::query_scalar!(
        "SELECT status FROM tasks WHERE id = $1 FOR UPDATE",
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(status) = status else {
        return Ok(false);
    };
    let status: TaskStatus = status.parse()?;
    if status.group() == StatusGroup::Done {
        tracing::debug!(task_id = %task_id, %status, "webhook event of a finished task dropped");
        return Ok(false);
    }
    let content = vec![text_block(render_message(event), false)];
    queue_message_tx(tx, task_id, Principal::system(MESSAGE_REASON), content).await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
