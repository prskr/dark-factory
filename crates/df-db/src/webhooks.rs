//! Applying forge webhook events to tasks (`iter3-webhooks` design D3, D4, D8): a message per
//! event, or the completion of the task of a merged pull request.

use df_core::system_reason::WEBHOOK;
use df_core::task_transition::{DoneBy, TaskEvent};
use df_core::webhooks::{Target, WebhookEvent, WebhookWake, render_message};
use df_core::{Principal, ProjectId, StatusGroup, StatusSet, TaskId, TaskStatus};
use sqlx::{PgPool, Postgres, Transaction};

use crate::pool::DbError;
use crate::task_events::apply_task_event;
use crate::task_views::statuses_of;
use crate::tasks::{lock_runs_then_task, queue_message_tx, text_block};

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

/// The project's tasks an event addresses: the task of a branch, the tasks of a pull request, or
/// the open (not `completed`, `cancelled` or `failed`) tasks of an issue.
pub async fn target_tasks(
    pool: &PgPool,
    project_id: ProjectId,
    target: &Target,
) -> Result<Vec<TaskId>, DbError> {
    let ids = match target {
        // The open holder of the branch first; without one, the latest task that had it.
        Target::Branch(branch) => {
            sqlx::query_scalar!(
                "SELECT id FROM tasks WHERE project_id = $1 AND branch = $2
                 ORDER BY (status NOT IN ('completed', 'failed', 'cancelled')) DESC,
                          created_at DESC, id DESC
                 LIMIT 1",
                project_id.as_uuid(),
                branch,
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
    let merged: Option<bool> =
        sqlx::query_scalar("SELECT pr_merged_at IS NOT NULL FROM tasks WHERE id = $1")
            .bind(task_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
    if merged != Some(false) {
        return Ok(false);
    }
    sqlx::query!(
        "UPDATE tasks SET pr_merged_at = now() WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    let actor = Principal::system(MERGED_REASON);
    let done = TaskEvent::MarkedDone(DoneBy::Merge {
        login: login.to_string(),
    });
    apply_task_event(tx, task_id, done, &actor).await?;
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
    queue_message_tx(tx, task_id, Principal::system(WEBHOOK), content).await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
