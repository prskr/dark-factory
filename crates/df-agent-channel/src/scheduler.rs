//! Run scheduling: starts runs for queued tasks, provisions sandboxes, follows provider events
//! and enforces connect and wall-clock timeouts (design D2).
//!
//! Every replica runs a scheduler. Nothing here needs a single writer: runs are claimed with
//! `FOR UPDATE SKIP LOCKED`, and every state change is a guarded, idempotent update.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use df_core::config::RunsConfig;
use df_core::{
    CAPACITY_DETAIL, ProjectKey, ProviderError, RunOutcome, RunSandboxSpec, RunState, SandboxEvent,
    TaskVolumeSpec,
};
use df_db::Notification;
use futures::StreamExt;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinSet;

use crate::ChannelContext;
use crate::assign::load_run_context;
use crate::error::ChannelError;
use crate::runs::{
    RunEnd, RunRow, finish_run, get_run, list_active_runs, list_unprovisioned_runs, mark_started,
    start_queued_runs, teardown,
};

/// Interval of the tick: resume times and freed slots are noticed on it (a due task starts
/// within 5 seconds), and it is the safety net besides notifications.
const TICK_INTERVAL: Duration = Duration::from_secs(5);
/// Age of a run without a sandbox after which the tick provisions it again (design D4).
const REPROVISION_AFTER: chrono::TimeDelta = chrono::TimeDelta::seconds(60);
/// Runs started per tick; the next tick (or notification) picks up the rest.
const START_BATCH: i64 = 10;
/// Time a stopped run gets to report before its sandbox is torn down (design D1).
const STOP_TIMEOUT_SECONDS: u64 = 120;
/// Time granted to in-flight provisioning when the scheduler stops.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(10);

/// Scheduler of one control plane replica.
#[derive(Clone)]
pub struct Scheduler {
    ctx: ChannelContext,
}

impl Scheduler {
    pub fn new(ctx: ChannelContext) -> Self {
        Self { ctx }
    }

    /// Runs until `shutdown` resolves. New runs start when a message is queued (and on every
    /// tick as a safety net); provisioning runs in its own tasks so slow provider calls never
    /// delay events, timeouts or shutdown. Timeouts are checked on the tick.
    pub async fn run(&self, shutdown: impl Future<Output = ()>) {
        let mut notifications = self.ctx.notifier.subscribe();
        let mut events = self.ctx.provider.events();
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        let mut provisions = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                _ = interval.tick() => {
                    if let Err(e) = self.tick(&mut provisions, Utc::now()).await {
                        tracing::error!(error = %e, "failed to enforce run timeouts");
                    }
                }
                note = notifications.recv() => match note {
                    Ok(Notification::TaskMessageQueued { .. } | Notification::Reconnected)
                        | Err(RecvError::Lagged(_)) => self.start_in_background(&mut provisions).await,
                    // A stop of a run without a connection ends it here, not on the tick.
                    Ok(Notification::RunChanged { .. }) => {
                        if let Err(e) = self.enforce_timeouts(Utc::now()).await {
                            tracing::error!(error = %e, "failed to end stopped runs");
                        }
                    }
                    Ok(_) => {}
                    Err(RecvError::Closed) => break,
                },
                event = events.next() => if let Some(event) = event { self.handle_event_logged(event).await } else {
                    tracing::warn!("sandbox event stream ended; scheduler continues on ticks only");
                    events = futures::stream::pending().boxed();
                },
                Some(_) = provisions.join_next() => {}
            }
        }
        // Runs that were created but not provisioned yet would only end on the connect timeout.
        let drain = async { while provisions.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_DRAIN, drain).await.is_err() {
            tracing::warn!(
                "provisioning still in flight at shutdown; runs end on the connect timeout"
            );
        }
    }

    /// One pass of the safety net: provisions runs whose provisioning was interrupted, starts due
    /// tasks and enforces the timeouts.
    async fn tick(
        &self,
        provisions: &mut JoinSet<()>,
        now: DateTime<Utc>,
    ) -> Result<(), ChannelError> {
        self.reprovision_stale(provisions, now).await;
        self.start_in_background(provisions).await;
        self.enforce_timeouts(now).await
    }

    /// Provisions runs again that stayed `pending`/`provisioning` for [`REPROVISION_AFTER`], for
    /// example because the replica that created them stopped before the sandbox existed. Volume
    /// and sandbox creation are idempotent; a run that has a sandbox only costs one existence check.
    async fn reprovision_stale(&self, provisions: &mut JoinSet<()>, now: DateTime<Utc>) {
        let Some(before) = now.checked_sub_signed(REPROVISION_AFTER) else {
            return;
        };
        match list_unprovisioned_runs(&self.ctx.pool, before).await {
            Ok(runs) => {
                for run in runs {
                    let scheduler = self.clone();
                    provisions.spawn(async move { scheduler.retry_provision(run).await });
                }
            }
            Err(e) => tracing::error!(error = %e, "failed to list runs to provision again"),
        }
    }

    /// Provisions a stale run again unless its sandbox exists. A failure is only logged: the run
    /// is tried again on the next tick or ends on the connect timeout, so a transient API error
    /// never ends a run that may be starting up fine.
    async fn retry_provision(&self, run: RunRow) {
        let run_id = run.id;
        match self.ctx.provider.sandbox_exists(run_id).await {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(error = %e, run_id = %run_id, "failed to check for the sandbox of a stale run");
                return;
            }
        }
        if let Err(e) = self.provision(run).await {
            tracing::warn!(error = %e, run_id = %run_id, "provisioning a stale run again failed; retrying on a later tick");
        }
    }

    /// Creates `pending` runs for due tasks.
    async fn start_runs(&self) -> Result<Vec<RunRow>, ChannelError> {
        start_queued_runs(
            &self.ctx.pool,
            START_BATCH,
            self.ctx.config.runs.max_concurrent,
        )
        .await
    }

    pub(crate) async fn start_in_background(&self, provisions: &mut JoinSet<()>) {
        match self.start_runs().await {
            Ok(runs) => {
                for run in runs {
                    let scheduler = self.clone();
                    provisions.spawn(async move { scheduler.provision_or_fail(run).await });
                }
            }
            Err(e) => tracing::error!(error = %e, "failed to start runs"),
        }
    }

    /// Provisions the run; anything that goes wrong ends it as `provisioning_failed` instead of
    /// leaving it `pending` until the connect timeout.
    async fn provision_or_fail(&self, run: RunRow) {
        let run_id = run.id;
        if let Err(e) = self.provision(run).await {
            tracing::error!(error = %e, run_id = %run_id, "provisioning failed");
            // The error is logged above; the run keeps the marker the run end acts on.
            let detail = match &e {
                ChannelError::Provider(ProviderError::Capacity(_)) => CAPACITY_DETAIL.to_string(),
                _ => e.to_string(),
            };
            if let Err(e2) = self.provisioning_failed(run_id, &detail).await {
                tracing::error!(error = %e2, run_id = %run_id, "failed to record provisioning failure");
            }
        }
    }

    /// Ensures the task volume, creates the sandbox and moves the run to `provisioning`. Errors
    /// are recorded by [`Self::provision_or_fail`].
    async fn provision(&self, run: RunRow) -> Result<(), ChannelError> {
        let run_id = run.id;
        let rc = load_run_context(&self.ctx.pool, run).await?;
        let provider = &self.ctx.provider;

        let volume = TaskVolumeSpec::for_task(rc.task.key.clone());
        let volume_name = volume.volume_name.clone();
        provider.ensure_task_volume(&volume).await?;
        df_db::tasks::clear_volume_deleted(&self.ctx.pool, rc.task.id).await?;
        // The run may have ended meanwhile (sandbox lost, connect timeout on another replica):
        // nothing would tear a sandbox created now down again.
        let moved =
            crate::runs::set_run_state(&self.ctx.pool, run_id, RunState::Provisioning, None)
                .await?;
        // Already `provisioning` is the retry of an interrupted run; anything else is over.
        if !moved
            && get_run(&self.ctx.pool, run_id)
                .await?
                .is_none_or(|run| run.state != RunState::Provisioning)
        {
            tracing::info!(run_id = %run_id, "run ended before its sandbox was created; skipping provisioning");
            return Ok(());
        }

        let project_key = ProjectKey::new(rc.project.key.clone())
            .map_err(|e| ChannelError::InvalidData(e.to_string()))?;
        let spec = RunSandboxSpec {
            run_id,
            run_number: u64::try_from(rc.run.number).unwrap_or(1),
            task_key: rc.task.key.clone(),
            project_id: rc.project.id,
            project_key,
            persona_id: rc.persona.id,
            image: rc.project.image.clone(),
            harness: rc
                .project
                .mounts_harness()
                .then(|| rc.persona.harness.clone()),
            volume_name,
            labels: BTreeMap::from([
                ("dark-factory.io/run-id".to_string(), run_id.to_string()),
                (
                    "dark-factory.io/task-key".to_string(),
                    rc.task.key.to_string(),
                ),
                (
                    "dark-factory.io/project".to_string(),
                    rc.project.key.clone(),
                ),
                (
                    "dark-factory.io/persona".to_string(),
                    rc.persona.id.to_string(),
                ),
            ]),
        };
        provider.provision(&spec).await?;
        // A stop may have ended the run while the sandbox was being created: that teardown
        // found nothing to delete.
        if get_run(&self.ctx.pool, run_id)
            .await?
            .is_some_and(|run| run.state.is_terminal())
        {
            tracing::info!(run_id = %run_id, "run ended while its sandbox was created; tearing it down");
            teardown(&self.ctx, run_id).await;
        }
        Ok(())
    }

    async fn provisioning_failed(
        &self,
        run_id: df_core::RunId,
        detail: &str,
    ) -> Result<(), ChannelError> {
        let end = RunEnd::failure(run_id, RunOutcome::ProvisioningFailed, detail);
        finish_run(&self.ctx, &end).await?;
        Ok(())
    }

    /// Applies a sandbox lifecycle event: `Started` → `connecting`, `Gone` → `sandbox_lost`.
    pub async fn handle_event(&self, event: SandboxEvent) -> Result<(), ChannelError> {
        match event {
            SandboxEvent::Started { run_id, pod_ip } => {
                mark_started(&self.ctx.pool, run_id, &pod_ip.to_string()).await?;
            }
            SandboxEvent::Gone { run_id, reason } => {
                let end = RunEnd::failure(
                    run_id,
                    RunOutcome::SandboxLost,
                    format!("sandbox is gone: {reason}"),
                );
                finish_run(&self.ctx, &end).await?;
            }
        }
        Ok(())
    }

    async fn handle_event_logged(&self, event: SandboxEvent) {
        if let Err(e) = self.handle_event(event).await {
            tracing::error!(error = %e, "failed to apply sandbox event");
        }
    }

    /// Ends runs a board member stopped (at once without a connection, after the stop timeout
    /// otherwise), fails runs that never connected and runs that outlived budget plus two grace
    /// periods (the first one is for `Cancel`, sent by the session holding the stream).
    pub async fn enforce_timeouts(&self, now: DateTime<Utc>) -> Result<(), ChannelError> {
        let cfg = &self.ctx.config.runs;
        let mut first_error = None;
        for run in list_active_runs(&self.ctx.pool).await? {
            let end = match run.state {
                // A human stopped a run that has no connection to send `Cancel` to.
                RunState::Pending | RunState::Provisioning | RunState::Connecting
                    if run.stop_requested_at.is_some() =>
                {
                    RunEnd::failure(run.id, RunOutcome::Cancelled, "stopped")
                }
                _ if is_before(
                    run.stop_requested_at
                        .and_then(|at| after(at, STOP_TIMEOUT_SECONDS)),
                    now,
                ) =>
                {
                    RunEnd::failure(run.id, RunOutcome::Cancelled, "stop-timeout")
                }
                RunState::Pending | RunState::Provisioning | RunState::Connecting
                    if is_before(after(run.started_at, cfg.connect_timeout_seconds), now) =>
                {
                    RunEnd::failure(
                        run.id,
                        RunOutcome::ConnectTimeout,
                        format!(
                            "the sandbox did not connect within {} s",
                            cfg.connect_timeout_seconds
                        ),
                    )
                }
                RunState::Preparing
                | RunState::Running
                | RunState::AwaitingInput
                | RunState::Finalizing
                    if is_before(hard_stop(run.budget_start(), cfg), now) =>
                {
                    RunEnd::failure(
                        run.id,
                        RunOutcome::Failed,
                        "the run did not stop after the wall-clock budget and was torn down",
                    )
                }
                _ => continue,
            };
            // One run failing must not stop the enforcement for the others.
            if let Err(e) = finish_run(&self.ctx, &end).await {
                tracing::error!(error = %e, run_id = %end.run_id, "failed to end a timed out run");
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// `start` plus `seconds`; `None` if that is not representable, which configuration treats as
/// "never".
fn after(start: DateTime<Utc>, seconds: u64) -> Option<DateTime<Utc>> {
    let delta = chrono::TimeDelta::try_seconds(i64::try_from(seconds).ok()?)?;
    start.checked_add_signed(delta)
}

fn is_before(deadline: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    deadline.is_some_and(|d| d < now)
}

fn hard_stop(start: DateTime<Utc>, cfg: &RunsConfig) -> Option<DateTime<Utc>> {
    after(
        start,
        cfg.max_wall_clock_seconds
            .saturating_add(cfg.cancel_grace_seconds.saturating_mul(2)),
    )
}

/// When the session should send `Cancel`: wall-clock budget plus one grace period after the
/// runtime connected. `None` if the configured budget is too large to represent (no limit).
pub fn cancel_deadline(budget_start: DateTime<Utc>, cfg: &RunsConfig) -> Option<DateTime<Utc>> {
    after(
        budget_start,
        cfg.max_wall_clock_seconds
            .saturating_add(cfg.cancel_grace_seconds),
    )
}

#[cfg(test)]
mod tests;
