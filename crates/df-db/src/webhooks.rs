//! Applying forge webhook events to tasks (`iter3-webhooks` design D3, D4, D8): a message per
//! event, or the completion of the task of a merged pull request.

use df_core::seams::ReviewVerdict;
use df_core::system_reason::WEBHOOK;
use df_core::task_transition::{DoneBy, TaskEvent};
use df_core::webhooks::{Target, WebhookEvent, WebhookWake, render_message};
use df_core::{Principal, ProjectId, StatusGroup, StatusSet, TaskId, TaskStatus};
use df_proto::transcript::v1::{
    WebhookCiFailed, WebhookCommentCreated, WebhookPullRequestClosed, WebhookReceived,
    WebhookReviewSubmitted, WebhookReviewVerdict, webhook_received,
};
use sqlx::{PgPool, Postgres, Transaction};

use crate::pool::DbError;
use crate::task_events::apply_task_event;
use crate::task_views::{status_list, statuses_of};
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
                 ORDER BY (status <> ALL($3)) DESC, created_at DESC, id DESC
                 LIMIT 1",
                project_id.as_uuid(),
                branch,
                &status_list(TaskStatus::DONE),
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
    complete_on_merge_as_tx(tx, task_id, login, &Principal::system(MERGED_REASON)).await
}

/// [`complete_on_merge_tx`] for a merge that Dark Factory requested itself
/// (`merge_subtask_pr`): `actor`, the agent of the parent task, is the actor of the audit entry.
pub async fn complete_on_merge_as_tx(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    login: &str,
    actor: &Principal,
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
    let done = TaskEvent::MarkedDone(DoneBy::Merge {
        login: login.to_string(),
    });
    apply_task_event(tx, task_id, done, actor).await?;
    Ok(true)
}

/// Queues the message for `event` on the task in the caller's transaction, unless the task is
/// `completed`, `cancelled` or `failed`. Returns whether a message was queued.
pub async fn queue_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    event: &WebhookEvent,
) -> Result<bool, DbError> {
    let task = sqlx::query!(
        "SELECT status, pr_number, pr_url FROM tasks WHERE id = $1 FOR UPDATE",
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(task) = task else {
        return Ok(false);
    };
    let status: TaskStatus = task.status.parse()?;
    if status.group() == StatusGroup::Done {
        tracing::debug!(task_id = %task_id, %status, "webhook event of a finished task dropped");
        return Ok(false);
    }
    // Hidden: the agent gets the text, the transcript shows the stored event once it is delivered.
    let content = vec![text_block(render_message(event), true)];
    let task_pr = task
        .pr_number
        .and_then(|number| u64::try_from(number).ok())
        .zip(task.pr_url.as_deref());
    let received = webhook_received(event, task_pr);
    queue_message_tx(tx, task_id, Principal::system(WEBHOOK), content, received).await?;
    Ok(true)
}

/// The display fields of `event` for the transcript row. The forge events carry no URL of their
/// own except a CI run's, so the URL of `task_pr`, the task's pull request as `(number, url)`, is
/// the link of reviews, pull request comments and closes, but only for an event about that same
/// number: an event routed by branch can name an older pull request of the task. `None` for a
/// merge, which completes the task and queues nothing.
pub fn webhook_received(
    event: &WebhookEvent,
    task_pr: Option<(u64, &str)>,
) -> Option<WebhookReceived> {
    let pr_link = |number: u64| {
        task_pr
            .filter(|(task_number, _)| *task_number == number)
            .map_or_else(String::new, |(_, url)| url.to_owned())
    };
    let kind = match event {
        WebhookEvent::ReviewSubmitted {
            number,
            actor,
            verdict,
            ..
        } => webhook_received::Kind::ReviewSubmitted(WebhookReviewSubmitted {
            number: *number,
            actor: actor.clone(),
            verdict: match verdict {
                ReviewVerdict::Approved => WebhookReviewVerdict::Approved,
                ReviewVerdict::ChangesRequested => WebhookReviewVerdict::ChangesRequested,
                ReviewVerdict::Commented => WebhookReviewVerdict::Commented,
            }
            .into(),
            url: pr_link(*number),
        }),
        WebhookEvent::CommentCreated {
            number,
            is_pull_request,
            actor,
            ..
        } => webhook_received::Kind::CommentCreated(WebhookCommentCreated {
            number: *number,
            is_pull_request: *is_pull_request,
            actor: actor.clone(),
            url: if *is_pull_request {
                pr_link(*number)
            } else {
                String::new()
            },
        }),
        WebhookEvent::PullRequestClosed { merged: true, .. } => return None,
        WebhookEvent::PullRequestClosed { number, actor, .. } => {
            webhook_received::Kind::PullRequestClosed(WebhookPullRequestClosed {
                number: *number,
                actor: actor.clone(),
                url: pr_link(*number),
            })
        }
        WebhookEvent::CiFailed { head, check, url } => {
            webhook_received::Kind::CiFailed(WebhookCiFailed {
                head: head.clone(),
                check: check.clone(),
                url: url.clone(),
            })
        }
    };
    Some(WebhookReceived { kind: Some(kind) })
}

#[cfg(test)]
mod tests;
