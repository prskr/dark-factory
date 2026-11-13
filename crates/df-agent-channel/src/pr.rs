//! Pull request lifecycle: draft PR on the first push ahead of base, ready for review on
//! completion, divergence flag (`specs/pull-request-lifecycle/spec.md`).
//!
//! Nothing here calls the forge. The triggers (a push report, the end of a run, a board member
//! marking the task done) enqueue a [`PR_SYNC_KIND`] outbox job in the transaction that records
//! them; [`sync::PrSyncHandler`] does the forge work and retries it with backoff
//! (`pr-forge-outbox` design D1, D2).

use std::time::Duration;

use df_core::{RepoRef, RunId, TaskId};
use df_db::outbox::RetryPolicy;
use df_db::tasks::{PR_SYNC_KIND, PrSyncJob};
use sqlx::{Postgres, Transaction};

use crate::error::ChannelError;

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

/// Retry policy of the pull request actions: the first retry after `base`
/// (`scheduling.forgeRetryBaseSeconds`, 30 s by default), doubling up to 10 minutes; after an hour
/// the dispatcher gives up with a warning. `lease` covers the forge calls of one attempt.
pub(crate) fn forge_retry_policy(base: Duration, lease: Duration) -> RetryPolicy {
    RetryPolicy {
        base,
        cap: Duration::from_mins(10),
        give_up_after: Duration::from_hours(1),
        lease,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retry_policy_starts_at_the_configured_base_and_keeps_cap_and_give_up_age() {
        let lease = Duration::from_secs(30);
        let policy = forge_retry_policy(Duration::from_secs(2), lease);

        assert_eq!(policy.backoff(1), Duration::from_secs(2));
        assert_eq!(policy.backoff(3), Duration::from_secs(8));
        assert_eq!(policy.cap, Duration::from_mins(10));
        assert_eq!(policy.give_up_after, Duration::from_hours(1));
        assert_eq!(policy.lease, lease);
        assert_eq!(
            forge_retry_policy(Duration::from_secs(30), lease).backoff(1),
            Duration::from_secs(30),
            "the default base"
        );
    }

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
