//! The outbox dispatcher (`lifecycle-events` design D3).
//!
//! While its replica leads, the dispatcher claims up to [`MAX_IN_FLIGHT`] due jobs of the kinds it
//! has a handler for, leases them, runs them at once and writes each result back. A job whose kind
//! has no handler stays untouched (a rolling update may add a kind). No transaction is open while
//! a handler runs: a crash leaves the lease, and the job runs again once it expired, so delivery is
//! at least once. A job claimed after its policy's `give_up_after` is given up without running.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Meter};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::{broadcast, watch};
use tokio::time::MissedTickBehavior;

use crate::notify::{Notification, Notifier};
use crate::pool::DbError;

/// Jobs claimed at once and run at the same time, across all kinds. One wave per claim keeps
/// every job of a batch within its lease.
pub const MAX_IN_FLIGHT: usize = 8;
/// Wake-up without an `outbox` notification, for example after a backoff elapsed.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Interval of [`Dispatcher::cleanup`].
pub const CLEANUP_INTERVAL: Duration = Duration::from_hours(1);
/// How long finished and given-up jobs are kept.
pub const RETAIN_FINISHED: Duration = Duration::from_hours(7 * 24);

/// When a kind's failed jobs run again and when they are given up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Delay after the first failed attempt; it doubles with every further one.
    pub base: Duration,
    /// Longest delay.
    pub cap: Duration,
    /// A job claimed this long after it was enqueued is given up without running.
    pub give_up_after: Duration,
    /// How long a claimed job is reserved: the handler's timeout plus a margin.
    pub lease: Duration,
}

impl RetryPolicy {
    /// Delay after the `attempts`-th failed attempt: `base · 2^(attempts - 1)`, at most `cap`.
    pub fn backoff(&self, attempts: u32) -> Duration {
        let doublings = attempts.saturating_sub(1).min(31);
        self.base.saturating_mul(1 << doublings).min(self.cap)
    }
}

/// A failed attempt.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// Run the job again after the backoff.
    #[error("{0}")]
    Retry(String),
    /// The job can never succeed.
    #[error("{0}")]
    GiveUp(String),
}

/// Runs the jobs of one kind. Must tolerate running a job more than once.
#[async_trait]
pub trait OutboxHandler: Send + Sync {
    /// The `outbox.kind` this handler runs.
    fn kind(&self) -> &'static str;

    fn retry_policy(&self) -> RetryPolicy;

    async fn handle(&self, payload: &Value) -> Result<(), JobError>;

    /// Runs after the periodic removal of finished jobs, for example to delete data that no job
    /// references any more.
    async fn cleanup(&self, _pool: &PgPool) -> Result<(), DbError> {
        Ok(())
    }
}

/// What became of an attempt; the `result` attribute of `df.outbox.jobs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobResult {
    Done,
    Retry,
    GaveUp,
}

impl JobResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Retry => "retry",
            Self::GaveUp => "gave_up",
        }
    }
}

/// Claims due jobs and runs their handlers; see the module documentation.
pub struct Dispatcher {
    pool: PgPool,
    notifier: Arc<Notifier>,
    handlers: HashMap<&'static str, Arc<dyn OutboxHandler>>,
    /// The longest lease of the handlers; one claim covers every kind.
    lease: Duration,
    jobs: Counter<u64>,
}

impl Dispatcher {
    pub fn new(
        pool: PgPool,
        notifier: Arc<Notifier>,
        handlers: Vec<Arc<dyn OutboxHandler>>,
        meter: &Meter,
    ) -> Self {
        let lease = handlers
            .iter()
            .map(|h| h.retry_policy().lease)
            .max()
            .unwrap_or_default();
        Self {
            pool,
            notifier,
            handlers: handlers.into_iter().map(|h| (h.kind(), h)).collect(),
            lease,
            jobs: meter
                .u64_counter("df.outbox.jobs")
                .with_description("Outbox job attempts by result")
                .build(),
        }
    }

    /// Dispatches while `is_leader` is `true`, until `stop` turns `true` (or its sender is gone)
    /// or the leader election ended. Woken by `outbox` notifications, every [`POLL_INTERVAL`]
    /// and every [`CLEANUP_INTERVAL`] for [`Self::cleanup`].
    pub async fn run(&self, mut is_leader: watch::Receiver<bool>, mut stop: watch::Receiver<bool>) {
        loop {
            // Both branches are cancel-safe.
            tokio::select! {
                leading = is_leader.wait_for(|leading| *leading) => if leading.is_err() {
                    tracing::warn!("leader election ended; stopping the outbox dispatcher");
                    return;
                },
                _ = stop.wait_for(|stop| *stop) => return,
            }
            tracing::info!("leading: dispatching outbox jobs");
            if self.lead(&mut is_leader, &mut stop).await == LeadEnd::Stopped {
                return;
            }
            tracing::info!("lost leadership: no longer dispatching outbox jobs");
        }
    }

    /// Claims one batch of due jobs, runs them and records the results. Returns the number of
    /// claimed jobs.
    pub async fn run_due(&self) -> Result<usize, DbError> {
        let kinds = self.kinds();
        let limit = i64::try_from(MAX_IN_FLIGHT).unwrap_or(i64::MAX);
        let jobs = sqlx::query_as!(
            Job,
            r#"UPDATE outbox SET lease_until = now() + make_interval(secs => $2), attempts = attempts + 1
               WHERE id IN (
                   SELECT id FROM outbox
                   WHERE done_at IS NULL AND gave_up_at IS NULL AND next_attempt_at <= now()
                     AND (lease_until IS NULL OR lease_until < now()) AND kind = ANY($1)
                   ORDER BY next_attempt_at
                   LIMIT $3
                   FOR UPDATE SKIP LOCKED
               )
               RETURNING id, kind, payload, attempts, last_error,
                         EXTRACT(EPOCH FROM now() - created_at)::float8 AS "age_secs!""#,
            &kinds,
            self.lease.as_secs_f64(),
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        let claimed = jobs.len();
        futures::stream::iter(jobs)
            .for_each_concurrent(MAX_IN_FLIGHT, |job| self.run_job(job))
            .await;
        Ok(claimed)
    }

    /// Deletes jobs that finished or were given up more than [`RETAIN_FINISHED`] ago, runs the
    /// handlers' cleanup hooks and warns about waiting jobs of kinds without a handler. Returns
    /// the number of deleted jobs.
    pub async fn cleanup(&self) -> Result<u64, DbError> {
        let deleted = sqlx::query!(
            "DELETE FROM outbox
             WHERE done_at < now() - make_interval(secs => $1)
                OR gave_up_at < now() - make_interval(secs => $1)",
            RETAIN_FINISHED.as_secs_f64(),
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        for handler in self.handlers.values() {
            handler.cleanup(&self.pool).await?;
        }
        let unknown = sqlx::query_scalar!(
            "SELECT DISTINCT kind FROM outbox
             WHERE done_at IS NULL AND gave_up_at IS NULL AND kind <> ALL($1)",
            &self.kinds(),
        )
        .fetch_all(&self.pool)
        .await?;
        if !unknown.is_empty() {
            tracing::warn!(kinds = ?unknown, "outbox jobs of kinds without a handler are waiting");
        }
        Ok(deleted)
    }

    /// Dispatches until leadership is lost or `stop` is set.
    async fn lead(
        &self,
        is_leader: &mut watch::Receiver<bool>,
        stop: &mut watch::Receiver<bool>,
    ) -> LeadEnd {
        let mut wake = self.notifier.subscribe();
        let mut cleanup = tokio::time::interval(CLEANUP_INTERVAL);
        cleanup.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            self.drain(|| *stop.borrow() || !*is_leader.borrow()).await;
            // All branches are cancel-safe; the work runs after the `select!`.
            let clean_up = tokio::select! {
                () = outbox_notified(&mut wake) => false,
                () = tokio::time::sleep(POLL_INTERVAL) => false,
                _ = cleanup.tick() => true,
                _ = stop.wait_for(|stop| *stop) => return LeadEnd::Stopped,
                lost = is_leader.wait_for(|leading| !*leading) => {
                    if lost.is_err() {
                        tracing::warn!("leader election ended; stopping the outbox dispatcher");
                        return LeadEnd::Stopped;
                    }
                    return LeadEnd::Lost;
                }
            };
            if clean_up && let Err(e) = self.cleanup().await {
                tracing::error!(error = %e, "outbox cleanup failed");
            }
        }
    }

    /// Runs batches until one is not full or `stopping` returns `true`.
    async fn drain(&self, stopping: impl Fn() -> bool) {
        while !stopping() {
            match self.run_due().await {
                Ok(claimed) if claimed == MAX_IN_FLIGHT => {}
                Ok(_) => return,
                Err(e) => {
                    tracing::error!(error = %e, "failed to claim outbox jobs");
                    return;
                }
            }
        }
    }

    async fn run_job(&self, job: Job) {
        let Some(handler) = self.handlers.get(job.kind.as_str()) else {
            return;
        };
        let policy = handler.retry_policy();
        let outcome = if job.age() >= policy.give_up_after {
            Err(JobError::GiveUp(job.last_error.clone().unwrap_or_else(
                || "expired before the first attempt".into(),
            )))
        } else {
            handler.handle(&job.payload).await
        };
        let error = outcome.as_ref().err().map(ToString::to_string);
        let recorded = match outcome {
            Ok(()) => self.finish(job.id).await.map(|()| JobResult::Done),
            Err(JobError::GiveUp(error)) => self
                .give_up(job.id, &error)
                .await
                .map(|()| JobResult::GaveUp),
            Err(JobError::Retry(error)) => self
                .retry(&job, &policy, &error)
                .await
                .map(|()| JobResult::Retry),
        };
        match recorded {
            Ok(result) => {
                if result == JobResult::GaveUp {
                    tracing::warn!(
                        job_id = job.id, kind = %job.kind, error = error.unwrap_or_default(),
                        "gave up an outbox job"
                    );
                }
                self.jobs.add(
                    1,
                    &[
                        KeyValue::new("kind", job.kind),
                        KeyValue::new("result", result.as_str()),
                    ],
                );
            }
            // The lease expires and the job runs again.
            Err(e) => tracing::error!(
                error = %e, job_id = job.id, kind = %job.kind,
                "failed to record an outbox job result"
            ),
        }
    }

    async fn finish(&self, id: i64) -> Result<(), DbError> {
        sqlx::query!(
            "UPDATE outbox SET done_at = now(), lease_until = NULL, last_error = NULL WHERE id = $1",
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn give_up(&self, id: i64, error: &str) -> Result<(), DbError> {
        sqlx::query!(
            "UPDATE outbox SET gave_up_at = now(), lease_until = NULL, last_error = $2 WHERE id = $1",
            id,
            error
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Schedules the next attempt; a job past the policy's age is given up when it is claimed.
    async fn retry(&self, job: &Job, policy: &RetryPolicy, error: &str) -> Result<(), DbError> {
        let attempts = u32::try_from(job.attempts).unwrap_or_default();
        sqlx::query!(
            "UPDATE outbox SET lease_until = NULL, last_error = $2,
                 next_attempt_at = now() + make_interval(secs => $3)
             WHERE id = $1",
            job.id,
            error,
            policy.backoff(attempts).as_secs_f64(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn kinds(&self) -> Vec<String> {
        self.handlers
            .keys()
            .map(|kind| (*kind).to_owned())
            .collect()
    }
}

/// A claimed job.
struct Job {
    id: i64,
    kind: String,
    payload: Value,
    attempts: i32,
    /// Error of the previous attempt.
    last_error: Option<String>,
    /// Seconds since the job was enqueued, by the database clock.
    age_secs: f64,
}

impl Job {
    fn age(&self) -> Duration {
        Duration::try_from_secs_f64(self.age_secs).unwrap_or_default()
    }
}

/// Why [`Dispatcher::lead`] returned.
#[derive(Debug, PartialEq, Eq)]
enum LeadEnd {
    Lost,
    Stopped,
}

/// Resolves on an `outbox` notification or when notifications were lost; never once the notifier
/// is gone (the poll interval takes over).
async fn outbox_notified(rx: &mut broadcast::Receiver<Notification>) {
    loop {
        match rx.recv().await {
            Ok(Notification::Outbox) | Err(broadcast::error::RecvError::Lagged(_)) => return,
            Ok(_) => {}
            Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests;
