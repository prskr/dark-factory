//! Task volume retention (`iter2-task-volume-lifecycle`, design D1): once per hour the leader
//! deletes the volumes of closed tasks after `volumes.retainAfterCloseSeconds`, of idle tasks
//! after `volumes.idleTtlSeconds` and of tasks with a merged PR at once, and marks them in
//! `tasks.volume_deleted_at`.

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use df_core::TaskId;
use df_db::lifecycle_events::VolumeDestroyedReason;
use df_db::volumes::{self, DELETE_TIMEOUT};
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::ChannelContext;
use crate::error::ChannelError;
use crate::runs::terminal_states;

/// Interval between two scans.
pub const SCAN_INTERVAL: Duration = Duration::from_hours(1);

/// Tasks whose volume is due for deletion (alias `t`). `$1` is the cutoff of closed tasks, `$2`
/// the one of every task (`NULL`: never); a task with a merged PR is due at once (`infinity`,
/// `iter3-webhooks` design D4). `GREATEST` ignores `NULL`, so the later applicable cutoff wins.
/// A task without runs never had a volume: its last run end is `NULL`, it never matches.
macro_rules! due {
    () => {
        concat!(
            "t.volume_deleted_at IS NULL
             AND NOT EXISTS (
                 SELECT 1 FROM runs r WHERE r.task_id = t.id AND r.state NOT IN ",
            terminal_states!(),
            ")
             AND (SELECT MAX(COALESCE(r.ended_at, r.started_at)) FROM runs r WHERE r.task_id = t.id)
                 < GREATEST(
                     CASE WHEN t.status IN ('completed', 'cancelled') THEN $1::timestamptz END,
                     $2::timestamptz,
                     CASE WHEN t.pr_merged_at IS NOT NULL THEN 'infinity'::timestamptz END
                 )"
        )
    };
}

/// The retention scan of one control plane replica; only the leader runs it.
pub struct Retention {
    ctx: ChannelContext,
    delete_timeout: Duration,
}

impl Retention {
    pub fn new(ctx: ChannelContext) -> Self {
        Self {
            ctx,
            delete_timeout: DELETE_TIMEOUT,
        }
    }

    /// Scans once per hour while `is_leader` is `true`, until `stop` turns `true` (or its sender
    /// is gone) or the leader election has ended. The stop is observed between scans and between
    /// the volumes of a scan: a deletion in progress finishes and commits first.
    pub async fn run(&self, is_leader: watch::Receiver<bool>, stop: watch::Receiver<bool>) {
        let stopping = || *stop.borrow();
        let ended = every_period_while_leader(SCAN_INTERVAL, is_leader, stop.clone(), || async {
            if let Err(e) = self.scan_until(Utc::now(), stopping).await {
                tracing::error!(error = %e, "volume retention scan failed");
            }
        })
        .await;
        if ended == LoopEnd::ElectionEnded {
            tracing::warn!("leader election ended; stopping volume retention");
        }
    }

    /// Deletes the volumes that are due at `now`, each in its own transaction; a failed deletion
    /// is logged and the scan goes on. Returns the number of deleted volumes.
    pub async fn scan(&self, now: DateTime<Utc>) -> Result<usize, ChannelError> {
        self.scan_until(now, || false).await
    }

    /// Like [`Self::scan`], but stops before the next volume once `stopping` returns `true`.
    async fn scan_until(
        &self,
        now: DateTime<Utc>,
        stopping: impl Fn() -> bool,
    ) -> Result<usize, ChannelError> {
        let cfg = &self.ctx.config.volumes;
        let cutoffs = (
            before(now, cfg.retain_after_close_seconds),
            before(now, cfg.idle_ttl_seconds),
        );
        let candidates: Vec<(uuid::Uuid, String)> = sqlx::query_as(concat!(
            "SELECT t.id, t.key FROM tasks t WHERE ",
            due!(),
            " ORDER BY t.created_at"
        ))
        .bind(cutoffs.0)
        .bind(cutoffs.1)
        .fetch_all(&self.ctx.pool)
        .await?;

        let mut deleted = 0;
        for (task_id, key) in candidates {
            if stopping() {
                break;
            }
            match self.delete_volume(task_id, &key, cutoffs).await {
                Ok(true) => deleted += 1,
                Ok(false) => {}
                Err(e) => {
                    tracing::error!(error = %e, task_key = %key, "failed to delete a task volume");
                }
            }
        }
        Ok(deleted)
    }

    /// Locks the task row (the row the scheduler locks to start a run), checks that the volume
    /// is still due, deletes it and sets the marker. Returns whether it deleted the volume; on
    /// an error the transaction rolls back.
    async fn delete_volume(
        &self,
        task_id: uuid::Uuid,
        key: &str,
        cutoffs: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    ) -> Result<bool, ChannelError> {
        let mut tx = self.ctx.pool.begin().await?;
        let task_key = volumes::lock_task(&mut tx, TaskId::from_uuid(task_id)).await?;
        let still_due: bool = sqlx::query_scalar(concat!(
            "SELECT EXISTS (SELECT 1 FROM tasks t WHERE t.id = $3 AND ",
            due!(),
            ")"
        ))
        .bind(cutoffs.0)
        .bind(cutoffs.1)
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        if !still_due {
            return Ok(false);
        }

        let deletion = volumes::Deletion {
            reason: VolumeDestroyedReason::Retention,
            events: &self.ctx.config.events,
            timeout: self.delete_timeout,
        };
        let volume = volumes::delete_volume(
            &mut tx,
            self.ctx.provider.as_ref(),
            TaskId::from_uuid(task_id),
            task_key,
            deletion,
        )
        .await?;
        tx.commit().await?;
        tracing::info!(task_key = %key, volume = %volume.volume_name, "deleted the volume of an expired task");
        Ok(true)
    }
}

/// Why [`every_period_while_leader`] returned.
#[derive(Debug, PartialEq, Eq)]
enum LoopEnd {
    Stopped,
    ElectionEnded,
}

/// Calls `scan` at the start of every `period` in which `is_leader` is `true`. Returns once `stop`
/// turns `true` or its sender is gone (checked between scans), or once the sender of `is_leader`
/// is gone.
async fn every_period_while_leader<F, Fut>(
    period: Duration,
    mut is_leader: watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
    mut scan: F,
) -> LoopEnd
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        // Both branches are cancel-safe; no scan runs inside the `select!`.
        tokio::select! {
            _ = interval.tick() => {}
            _ = stop.wait_for(|stop| *stop) => return LoopEnd::Stopped,
        }
        if is_leader.has_changed().is_err() {
            return LoopEnd::ElectionEnded;
        }
        if *is_leader.borrow_and_update() {
            scan().await;
        }
    }
}

/// `now` minus `seconds`; `None` (never) if that is not representable.
fn before(now: DateTime<Utc>, seconds: u64) -> Option<DateTime<Utc>> {
    let delta = TimeDelta::try_seconds(i64::try_from(seconds).ok()?)?;
    now.checked_sub_signed(delta)
}

#[cfg(test)]
mod tests;
