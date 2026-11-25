//! Run scheduling: starts runs for queued tasks, provisions sandboxes, follows provider events
//! and enforces connect and wall-clock timeouts (design D2).
//!
//! Every replica runs a scheduler. Nothing here needs a single writer: runs are claimed with
//! `FOR UPDATE SKIP LOCKED`, and every state change is a guarded, idempotent update.

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use df_core::{
    CAPACITY_DETAIL, ProjectKey, ProviderError, RunId, RunOutcome, RunSandboxSpec, RunState,
    SandboxEvent, TaskVolumeSpec,
};
use df_db::Notification;
use futures::StreamExt;
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinSet;

use crate::OrchestratorContext;
use crate::base_branch::ensure_base_branch;
use crate::error::OrchestratorError;
use crate::run_context::load_run_context;
use crate::runs::{finish_overdue, finish_run, teardown};
use df_core::run_transition::RunEnd;
use df_db::persona_keys::sync_signing_key;
use df_db::runs::{
    RunRow, begin_provisioning, get_run, list_active_runs, list_overdue_runs,
    list_unprovisioned_runs, mark_started, start_queued_runs,
};

/// Age of a run without a sandbox after which the tick provisions it again (design D4).
const REPROVISION_AFTER: SignedDuration = SignedDuration::from_secs(60);
/// Runs started per tick; the next tick (or notification) picks up the rest.
const START_BATCH: i64 = 10;
/// Time granted to in-flight provisioning when the scheduler stops.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(10);
/// Detail of a run the startup sweep ends because its sandbox is gone (design D4).
const LOST_WHILE_DOWN: &str = "sandbox pod no longer present after a control plane restart";
/// Passes of the startup sweep; a failed listing or check is retried after a tick interval.
const SWEEP_ATTEMPTS: u32 = 3;
/// Existence checks the startup sweep runs at once.
const SWEEP_CONCURRENCY: usize = 8;

/// Scheduler of one control plane replica.
#[derive(Clone)]
pub struct Scheduler {
    ctx: OrchestratorContext,
    /// Runs this replica is provisioning; a tick starts no second attempt for them. Another
    /// replica may still retry such a run (design D2); creation is idempotent.
    in_flight: Arc<Mutex<HashSet<RunId>>>,
}

impl Scheduler {
    pub fn new(ctx: OrchestratorContext) -> Self {
        Self {
            ctx,
            in_flight: Arc::default(),
        }
    }

    /// Runs until `shutdown` resolves. New runs start when a message is queued (and on every
    /// tick as a safety net); provisioning runs in its own tasks so slow provider calls never
    /// delay events, timeouts or shutdown. Timeouts are checked on the tick. On start, runs whose
    /// sandbox disappeared while no replica watched are ended in the background.
    pub async fn run(&self, shutdown: impl Future<Output = ()>) {
        let mut notifications = self.ctx.notifier.subscribe();
        let mut events = self.ctx.provider.events();
        // Resume times and freed slots are noticed on the tick (`scheduling.tickMillis`, 5 s by
        // default), the safety net besides notifications.
        let tick = self.ctx.config.scheduling.tick();
        let mut interval = tokio::time::interval(tick);
        // Provisioning and the startup sweep run here, off the event loop.
        let mut background = JoinSet::new();
        let scheduler = self.clone();
        background.spawn(async move { scheduler.sweep_lost_sandboxes(tick).await });
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                _ = interval.tick() => {
                    if let Err(e) = self.tick(&mut background, Timestamp::now()).await {
                        tracing::error!(error = %e, "failed to enforce run timeouts");
                    }
                }
                note = notifications.recv() => match note {
                    Ok(Notification::TaskMessageQueued { .. } | Notification::Reconnected)
                        | Err(RecvError::Lagged(_)) => self.start_in_background(&mut background).await,
                    // A stop of a run without a connection ends it here, not on the tick.
                    Ok(Notification::RunChanged { .. }) => {
                        if let Err(e) = self.enforce_timeouts(Timestamp::now()).await {
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
                Some(_) = background.join_next() => {}
            }
        }
        // Unfinished work is picked up again by the next replica: unprovisioned runs by the
        // stale-run retry, unchecked runs by its startup sweep.
        let drain = async { while background.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_DRAIN, drain).await.is_err() {
            tracing::warn!("background work still in flight at shutdown; abandoning it");
        }
    }

    /// One pass of the safety net: provisions runs whose provisioning was interrupted, starts due
    /// tasks and enforces the timeouts.
    async fn tick(
        &self,
        background: &mut JoinSet<()>,
        now: Timestamp,
    ) -> Result<(), OrchestratorError> {
        self.reprovision_stale(background, now).await;
        self.start_in_background(background).await;
        self.enforce_timeouts(now).await
    }

    /// Provisions runs again that stayed `pending`/`provisioning` for [`REPROVISION_AFTER`], for
    /// example because the replica that created them stopped before the sandbox existed. Volume
    /// and sandbox creation are idempotent; a run that has a sandbox only costs one existence check.
    async fn reprovision_stale(&self, background: &mut JoinSet<()>, now: Timestamp) {
        let Some(before) = now.checked_sub(REPROVISION_AFTER).ok() else {
            return;
        };
        match list_unprovisioned_runs(&self.ctx.pool, before).await {
            Ok(runs) => {
                for run in runs {
                    let scheduler = self.clone();
                    self.spawn_provisioning(background, run.id, async move {
                        scheduler.retry_provision(run).await;
                    });
                }
            }
            Err(e) => tracing::error!(error = %e, "failed to list runs to provision again"),
        }
    }

    /// Spawns `work` provisioning `run_id` unless an earlier attempt of this replica is still
    /// in flight.
    fn spawn_provisioning(
        &self,
        background: &mut JoinSet<()>,
        run_id: RunId,
        work: impl Future<Output = ()> + Send + 'static,
    ) {
        let Some(claim) = InFlight::claim(&self.in_flight, run_id) else {
            return;
        };
        background.spawn(async move {
            let _claim = claim;
            work.await;
        });
    }

    /// Ends every run in `connecting` or later whose sandbox no longer exists as `sandbox_lost`
    /// (design D4): the event stream only reports deletions it sees, not those that happened
    /// while no replica ran. `pending`/`provisioning` runs are left to [`Self::reprovision_stale`].
    /// A failed listing or check is retried after `retry_after`, [`SWEEP_ATTEMPTS`] passes in
    /// all; a run still unchecked then ends on its timeouts if the sandbox is really gone.
    async fn sweep_lost_sandboxes(&self, retry_after: Duration) {
        for attempt in 1..=SWEEP_ATTEMPTS {
            if self.sweep_once().await {
                return;
            }
            if attempt < SWEEP_ATTEMPTS {
                tokio::time::sleep(retry_after).await;
            }
        }
        tracing::error!(
            attempts = SWEEP_ATTEMPTS,
            "gave up checking runs for lost sandboxes after a restart; unchecked runs end on their timeouts"
        );
    }

    /// One pass of [`Self::sweep_lost_sandboxes`]; `false` if the listing or a check failed.
    async fn sweep_once(&self) -> bool {
        let runs = match list_active_runs(&self.ctx.pool).await {
            Ok(runs) => runs,
            Err(e) => {
                tracing::error!(error = %e, "failed to list runs to check for lost sandboxes");
                return false;
            }
        };
        let started = runs
            .into_iter()
            .filter(|run| !matches!(run.state, RunState::Pending | RunState::Provisioning));
        futures::stream::iter(started)
            .map(|run| self.end_if_sandbox_lost(run.id))
            .buffer_unordered(SWEEP_CONCURRENCY)
            .fold(true, |all, checked| async move { all && checked })
            .await
    }

    /// Ends the run as `sandbox_lost` if its sandbox is gone; `false` if that could not be
    /// determined or recorded.
    async fn end_if_sandbox_lost(&self, run_id: RunId) -> bool {
        match self.ctx.provider.sandbox_exists(run_id).await {
            Ok(true) => true,
            Ok(false) => {
                let end = RunEnd::failure(run_id, RunOutcome::SandboxLost, LOST_WHILE_DOWN);
                if let Err(e) = finish_run(&self.ctx, &end).await {
                    tracing::error!(error = %e, run_id = %run_id, "failed to end a run whose sandbox is gone");
                    return false;
                }
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, run_id = %run_id, "failed to check for the sandbox of a run after a restart");
                false
            }
        }
    }

    /// Provisions a stale run again unless its sandbox exists. A transient failure is only
    /// logged: the run is tried again on the next tick or ends on the connect timeout, so it never
    /// ends a run that may be starting up fine. A rejection the provider won't change its mind on
    /// (`Capacity`, `InvalidSpec`) ends the run as `provisioning_failed`, unless the sandbox
    /// exists by now (a concurrent attempt's pod may be what exceeded the quota).
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
        let Err(e) = self.provision(run).await else {
            return;
        };
        let permanent = matches!(
            e,
            OrchestratorError::Provider(ProviderError::Capacity(_) | ProviderError::InvalidSpec(_))
        );
        if permanent && matches!(self.ctx.provider.sandbox_exists(run_id).await, Ok(false)) {
            self.fail_provisioning(run_id, &e).await;
            return;
        }
        tracing::warn!(error = %e, run_id = %run_id, "provisioning a stale run again failed; retrying on a later tick");
    }

    /// Creates `pending` runs for due tasks.
    async fn start_runs(&self) -> Result<Vec<RunRow>, OrchestratorError> {
        Ok(start_queued_runs(
            &self.ctx.pool,
            START_BATCH,
            self.ctx.config.runs.max_concurrent,
            self.ctx.config.runs.max_concurrent_chats,
        )
        .await?)
    }

    pub(crate) async fn start_in_background(&self, background: &mut JoinSet<()>) {
        match self.start_runs().await {
            Ok(runs) => {
                for run in runs {
                    let scheduler = self.clone();
                    self.spawn_provisioning(background, run.id, async move {
                        scheduler.provision_or_fail(run).await;
                    });
                }
            }
            Err(e) => tracing::error!(error = %e, "failed to start runs"),
        }
    }

    /// Provisions the run; anything that goes wrong ends it as `provisioning_failed` instead of
    /// leaving it `pending` until the connect timeout. A temporarily unavailable provider leaves
    /// the run `provisioning` for `reprovision_stale` to retry.
    async fn provision_or_fail(&self, run: RunRow) {
        let run_id = run.id;
        if let Err(e) = self.provision(run).await {
            if let OrchestratorError::Provider(ProviderError::Unavailable(_)) = &e {
                // ponytail: design D2 – no backoff. Once the run is older than 60 s,
                // `reprovision_stale` retries it every tick, costing one `sandbox_exists` list
                // plus up to two lookups per rule every 5 s, until the pod exists or
                // `enforce_timeouts` ends the run with `connect_timeout`.
                tracing::warn!(
                    error = %e,
                    run_id = %run_id,
                    "sandbox provider temporarily unavailable, run stays provisioning"
                );
                return;
            }
            tracing::error!(error = %e, run_id = %run_id, "provisioning failed");
            self.fail_provisioning(run_id, &e).await;
        }
    }

    /// Ends the run as `provisioning_failed`; a capacity rejection requeues the task without
    /// counting a resume.
    async fn fail_provisioning(&self, run_id: RunId, error: &OrchestratorError) {
        let detail = match error {
            OrchestratorError::Provider(ProviderError::Capacity(_)) => CAPACITY_DETAIL.to_string(),
            _ => error.to_string(),
        };
        // A refused base branch is no infrastructure failure: a resume would fail the same way.
        let outcome = match error {
            OrchestratorError::BaseBranch(_) => RunOutcome::Failed,
            _ => RunOutcome::ProvisioningFailed,
        };
        let end = RunEnd::failure(run_id, outcome, detail);
        if let Err(e) = finish_run(&self.ctx, &end).await {
            tracing::error!(error = %e, run_id = %run_id, "failed to record provisioning failure");
        }
    }

    /// Moves the run to `provisioning`, ensures the task volume and creates the sandbox. Errors
    /// are recorded by [`Self::provision_or_fail`].
    async fn provision(&self, run: RunRow) -> Result<(), OrchestratorError> {
        let run_id = run.id;
        let rc = load_run_context(&self.ctx.pool, run).await?;
        let provider = &self.ctx.provider;

        let events = &self.ctx.config.events;
        // The run may have ended meanwhile (sandbox lost, connect timeout on another replica):
        // nothing would tear a sandbox created now down again.
        let moved = begin_provisioning(&self.ctx.pool, events, run_id).await?;
        // Already `provisioning` is the retry of an interrupted run; anything else is over.
        if !moved
            && get_run(&self.ctx.pool, run_id)
                .await?
                .is_none_or(|run| run.state != RunState::Provisioning)
        {
            tracing::info!(run_id = %run_id, "run ended before its sandbox was created; skipping provisioning");
            return Ok(());
        }

        // A persona from before the signing keys gets its key now: the runtime needs it to sign.
        sync_signing_key(
            &self.ctx.pool,
            self.ctx.secrets.as_ref(),
            rc.persona.id.as_uuid(),
        )
        .await?;
        ensure_base_branch(&self.ctx, &rc).await?;

        let volume = TaskVolumeSpec::for_task(rc.task.key.clone());
        let volume_name = volume.volume_name.clone();
        provider.ensure_task_volume(&volume).await?;
        df_db::volumes::volume_ensured(&self.ctx.pool, events, rc.task.id, run_id).await?;

        let project_key = ProjectKey::new(rc.project.key.clone())
            .map_err(|e| OrchestratorError::InvalidData(e.to_string()))?;
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

    /// Applies a sandbox lifecycle event: `Started` → `connecting`, `Gone` → `sandbox_lost`.
    pub async fn handle_event(&self, event: SandboxEvent) -> Result<(), OrchestratorError> {
        match event {
            SandboxEvent::Started { run_id, pod_ip } => {
                mark_started(
                    &self.ctx.pool,
                    &self.ctx.config.events,
                    run_id,
                    &pod_ip.to_string(),
                )
                .await?;
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
    pub async fn enforce_timeouts(&self, now: Timestamp) -> Result<(), OrchestratorError> {
        let mut first_error = None;
        // The query selects a superset of the runs that are overdue; `overdue` decides, on the
        // locked row (`df_core::run_transition::overdue`).
        for run in list_overdue_runs(&self.ctx.pool, now, &self.ctx.config.runs).await? {
            // One run failing must not stop the enforcement for the others.
            if let Err(e) = finish_overdue(&self.ctx, run.id, now).await {
                tracing::error!(error = %e, run_id = %run.id, "failed to end a timed out run");
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// A run in [`Scheduler::in_flight`]; dropping the claim (attempt finished or aborted) releases it.
struct InFlight {
    runs: Arc<Mutex<HashSet<RunId>>>,
    run_id: RunId,
}

impl InFlight {
    /// `None` if the run is claimed already.
    fn claim(runs: &Arc<Mutex<HashSet<RunId>>>, run_id: RunId) -> Option<Self> {
        let claimed = runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run_id);
        claimed.then(|| Self {
            runs: runs.clone(),
            run_id,
        })
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.run_id);
    }
}

#[cfg(test)]
mod base_branch_tests;
#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod tests;
