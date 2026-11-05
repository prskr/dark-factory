//! Pull request lifecycle: draft PR on the first push ahead of base, ready for review on
//! completion, divergence flag (`specs/pull-request-lifecycle/spec.md`).
//!
//! Nothing here calls the forge. The triggers (a push report, the end of a run, a board member
//! marking the task done) enqueue a [`PR_SYNC_KIND`] outbox job in the transaction that records
//! them; [`sync::PrSyncHandler`] does the forge work and retries it with backoff
//! (`pr-forge-outbox` design D1, D2).

use df_core::{RepoRef, RunId, RunOutcome, TaskId};
use df_db::tasks::{PR_SYNC_KIND, PrSyncJob, clock_under_task_lock};
use sqlx::{Postgres, Transaction};

use crate::error::ChannelError;
use crate::runs::RunEnd;

pub mod mark_draft;
pub mod sync;

/// Enqueues `pr.sync { open, run_id }` for a task-branch push ahead of base, unless the task has
/// a PR recorded. Returns whether a job was enqueued.
pub(crate) async fn enqueue_open_after_push(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    run_id: RunId,
) -> Result<bool, ChannelError> {
    let has_pr: Option<bool> =
        sqlx::query_scalar("SELECT pr_number IS NOT NULL FROM tasks WHERE id = $1")
            .bind(task_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
    if has_pr != Some(false) {
        return Ok(false);
    }
    let job = PrSyncJob {
        task_id,
        run_id: Some(run_id),
        open: true,
        ready: None,
    };
    enqueue(tx, job).await?;
    Ok(true)
}

/// Enqueues the `pr.sync` job of a run end: open the PR if the branch was pushed, mark it ready
/// if the run completed. No job for a run that did neither.
///
/// The caller holds the task row lock. The ready time is read from the database clock under it,
/// the same clock `task_messages.queued_at` uses, so a message that waited for the lock to
/// reopen the task is queued after it and one pending at completion before it (design D3).
pub(crate) async fn enqueue_run_end(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    end: &RunEnd,
) -> Result<(), ChannelError> {
    let completed = end.outcome == RunOutcome::Completed;
    if !end.task_branch_pushed && !completed {
        return Ok(());
    }
    let ready = if completed {
        Some(clock_under_task_lock(tx).await?)
    } else {
        None
    };
    let job = PrSyncJob {
        task_id,
        run_id: Some(end.run_id),
        open: end.task_branch_pushed,
        ready,
    };
    enqueue(tx, job).await
}

async fn enqueue(tx: &mut Transaction<'_, Postgres>, job: PrSyncJob) -> Result<(), ChannelError> {
    df_db::outbox::enqueue(tx, PR_SYNC_KIND, &serde_json::to_value(job)?).await?;
    Ok(())
}

/// Takes the per-task lock of the PR forge handlers (`task-pr:<id>`), also across control-plane
/// replicas. It is released when the returned transaction ends, on every path including a
/// dropped future.
///
/// ponytail: only outbox jobs take it, and the dispatcher runs at most `MAX_IN_FLIGHT` (8) of
/// them, so at most 8 pooled connections wait or hold it across forge calls (design D5). Add a
/// separate lock connection pool if the dispatcher's concurrency grows.
pub(crate) async fn lock_task_pr(
    pool: &sqlx::PgPool,
    task_id: TaskId,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("task-pr:{task_id}"))
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Owner and name from the last two path segments of a repository URL.
pub fn repo_ref(repo_url: &str) -> Result<RepoRef, ChannelError> {
    let path = repo_url.split_once("://").map_or(repo_url, |(_, rest)| {
        rest.split_once('/').map_or("", |(_, p)| p)
    });
    let mut segments = path.trim_matches('/').rsplit('/');
    let name = segments.next().map(|n| n.strip_suffix(".git").unwrap_or(n));
    match (segments.next(), name) {
        (Some(owner), Some(name)) if !owner.is_empty() && !name.is_empty() => Ok(RepoRef {
            owner: owner.to_string(),
            name: name.to_string(),
        }),
        _ => Err(ChannelError::InvalidData(format!(
            "repository URL `{repo_url}` has no owner/name path"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_ref_takes_owner_and_name() {
        let r = repo_ref("https://git.example.org/peter/dark-factory.git").unwrap();
        assert_eq!(
            (r.owner.as_str(), r.name.as_str()),
            ("peter", "dark-factory")
        );
        assert!(repo_ref("https://git.example.org/").is_err());
    }
}
