//! The run state machine as one pure function (ADR-0061).
//!
//! [`run_transition`] takes what the row of a run shows ([`RunSnapshot`]) and what happened
//! ([`RunEvent`]) and returns the new state, the fields to write and the [`RunEffect`]s the
//! caller executes in its transaction. `df_db::apply_run_event` loads the snapshot, calls it and
//! executes the result; no other code writes `runs.state`. [`overdue`] is the pure decision of the
//! scheduler's timeout enforcement.
//!
//! The state diagram and the events are in `crates/df-core/docs/run-transitions.md`.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, TimeDelta, Utc};

use crate::config::RunsConfig;
use crate::domain::{ParseEnumError, RunOutcome, RunState};
use crate::ids::RunId;
use crate::task_transition::StopScope;

/// Time a stopped run gets to report before its sandbox is torn down (design D1).
pub const STOP_TIMEOUT_SECONDS: u64 = 120;

/// What the run's row shows when an event is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,
    /// When a board member stopped the run.
    pub stop_requested_at: Option<DateTime<Utc>>,
    /// First `Hello` of the run's runtime.
    pub connected_at: Option<DateTime<Utc>>,
    /// The scheduler began to provision the sandbox.
    pub provisioning_began: bool,
    pub finalize_reason: Option<FinalizeReason>,
    /// A message of the task is undelivered. Read for [`FinalizeReason::Idle`] only.
    pub undelivered_messages: bool,
    pub started_at: DateTime<Utc>,
}

impl RunSnapshot {
    /// Start of the wall-clock budget: when the runtime connected, else when the run was created.
    pub fn budget_start(&self) -> DateTime<Utc> {
        self.connected_at.unwrap_or(self.started_at)
    }
}

/// Why a run is `finalizing` (`runs.finalize_reason`). It decides the command a session sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FinalizeReason {
    /// The idle deadline passed.
    Idle,
    /// The wall-clock budget is used up.
    Budget,
    /// A board member stopped the run (or cancelled the task).
    Stop,
    /// A board member marked the task done.
    Done,
}

impl FinalizeReason {
    pub const ALL: &'static [Self] = &[Self::Idle, Self::Budget, Self::Stop, Self::Done];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Budget => "budget",
            Self::Stop => "stop",
            Self::Done => "done",
        }
    }

    /// The command a session sends to the runtime of a run that is `finalizing` for this reason.
    pub const fn command(self) -> RunCommand {
        match self {
            Self::Idle => RunCommand::Release,
            Self::Budget => RunCommand::Cancel("wall-clock budget exceeded"),
            Self::Stop => RunCommand::Cancel("run stopped by a board member"),
            Self::Done => RunCommand::Cancel("task marked done"),
        }
    }
}

impl fmt::Display for FinalizeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FinalizeReason {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|reason| reason.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "FinalizeReason",
                value: s.to_string(),
            })
    }
}

/// A command for the runtime of a `finalizing` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCommand {
    /// End gracefully after the idle deadline.
    Release,
    /// Stop the run, with the reason the runtime reports.
    Cancel(&'static str),
}

/// Final state of a run, from a `RunReport` or from the control plane's own failure handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEnd {
    pub run_id: RunId,
    pub outcome: RunOutcome,
    pub detail: String,
    pub retry_after: Option<DateTime<Utc>>,
    pub harness_session_id: String,
    pub task_branch_sha: String,
    pub wip_sha: String,
    pub diverged: bool,
    /// The runtime pushed the task branch (`WorkspaceState`).
    pub task_branch_pushed: bool,
}

impl RunEnd {
    /// An end caused by the control plane (timeout, lost or failed sandbox).
    pub fn failure(run_id: RunId, outcome: RunOutcome, detail: impl Into<String>) -> Self {
        Self {
            run_id,
            outcome,
            detail: detail.into(),
            retry_after: None,
            harness_session_id: String::new(),
            task_branch_sha: String::new(),
            wip_sha: String::new(),
            diverged: false,
            task_branch_pushed: false,
        }
    }
}

/// Something that happened to a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// The scheduler picked the run.
    ProvisioningStarted,
    /// The sandbox is up.
    SandboxStarted {
        pod_ip: String,
    },
    /// The runtime opened its stream.
    Connected,
    /// The run left its idle phase for a turn.
    TurnStarted,
    /// The run is idle: its turn ended.
    TurnEnded,
    /// The control plane ends the run on its own account.
    Finalize(FinalizeReason),
    /// A board member stopped the run.
    StopRequested(StopScope),
    Ended(RunEnd),
    /// Becomes `Ended` if [`overdue`] says so on the locked snapshot.
    Overdue {
        now: DateTime<Utc>,
        limits: RunsConfig,
    },
}

/// A command the run's state does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("a {0} run cannot be stopped")]
    Stop(RunState),
}

/// The columns an event writes besides `state`. A stamp sets the column to the database clock if
/// it is null (`stop_requested_at`: the clock of the transaction's statement).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFields {
    pub pod_ip: Option<String>,
    pub stamp_provisioning_started: bool,
    pub stamp_sandbox_started: bool,
    pub stamp_connected: bool,
    pub stamp_stop_requested: bool,
    /// Written with `finalizing`.
    pub finalize_reason: Option<FinalizeReason>,
    /// The outcome columns and `ended_at`.
    pub end: Option<RunEnd>,
}

/// Something the applier does in the transaction of the event, in the order of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEffect {
    /// Publish `RunChanged`.
    Publish,
    /// Enqueue `sandbox.provisioning`.
    SandboxProvisioning,
    /// Enqueue `sandbox.provisioned`.
    SandboxProvisioned,
    /// Enqueue `sandbox.provisioning_failed` or `sandbox.destroyed` for the run's end.
    SandboxClosed,
    /// Apply an event to the task of the run.
    Task(TaskCue),
}

/// The task events a run raises. The applier fills them from the rows and the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCue {
    TurnStarted,
    TurnEnded,
    RunEnded,
}

/// The result of an event: the run's new state, the fields to write and the effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTransition {
    pub state: RunState,
    pub set: RunFields,
    pub effects: Vec<RunEffect>,
}

impl RunTransition {
    /// An event that changes nothing.
    pub fn unchanged(run: &RunSnapshot) -> Self {
        Self {
            state: run.state,
            set: RunFields::default(),
            effects: Vec::new(),
        }
    }

    /// Whether the event changes the row.
    pub fn changes(&self) -> bool {
        !self.effects.is_empty()
    }

    fn to(run: &RunSnapshot, state: RunState) -> Self {
        Self {
            state,
            ..Self::unchanged(run)
        }
    }

    fn with(mut self, effect: RunEffect) -> Self {
        self.effects.push(effect);
        self
    }
}

/// The new state of `run` after `event`, or the reason the state does not allow it.
pub fn run_transition(run: &RunSnapshot, event: RunEvent) -> Result<RunTransition, Rejected> {
    Ok(match event {
        RunEvent::ProvisioningStarted => provisioning_started(run),
        RunEvent::SandboxStarted { pod_ip } => sandbox_started(run, pod_ip),
        RunEvent::Connected => connected(run),
        RunEvent::TurnStarted => turn(run, RunState::Running, TaskCue::TurnStarted),
        RunEvent::TurnEnded => turn(run, RunState::AwaitingInput, TaskCue::TurnEnded),
        RunEvent::Finalize(reason) => finalize(run, reason),
        RunEvent::StopRequested(scope) => return stop_requested(run, scope),
        RunEvent::Ended(end) => ended(run, end),
        RunEvent::Overdue { now, limits } => match overdue(run, now, &limits) {
            Some(event) => return run_transition(run, event),
            None => RunTransition::unchanged(run),
        },
    })
}

/// The end an overdue run gets at `now`, if any: a stopped run that has no connection to send
/// `Cancel` to, a stop that was not obeyed within [`STOP_TIMEOUT_SECONDS`], a sandbox that did not
/// connect, a run that outlived budget plus two grace periods (the first one is for `Cancel`).
pub fn overdue(run: &RunSnapshot, now: DateTime<Utc>, cfg: &RunsConfig) -> Option<RunEvent> {
    if run.state.is_terminal() {
        return None;
    }
    let unconnected = run.state.is_unconnected();
    let connected = run.state.is_runtime_up() || run.state == RunState::Finalizing;
    let stop_timeout = run
        .stop_requested_at
        .and_then(|at| after(at, STOP_TIMEOUT_SECONDS));
    let end = if unconnected && run.stop_requested_at.is_some() {
        // A human stopped a run that has no connection to send `Cancel` to.
        RunEnd::failure(run.run_id, RunOutcome::Cancelled, "stopped")
    } else if is_before(stop_timeout, now) {
        RunEnd::failure(run.run_id, RunOutcome::Cancelled, "stop-timeout")
    } else if unconnected && is_before(after(run.started_at, cfg.connect_timeout_seconds), now) {
        RunEnd::failure(
            run.run_id,
            RunOutcome::ConnectTimeout,
            format!(
                "the sandbox did not connect within {} s",
                cfg.connect_timeout_seconds
            ),
        )
    } else if connected && is_before(hard_stop(run.budget_start(), cfg), now) {
        RunEnd::failure(
            run.run_id,
            RunOutcome::Failed,
            "the run did not stop after the wall-clock budget and was torn down",
        )
    } else {
        return None;
    };
    Some(RunEvent::Ended(end))
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

/// `start` plus `seconds`; `None` if that is not representable, which configuration treats as
/// "never".
fn after(start: DateTime<Utc>, seconds: u64) -> Option<DateTime<Utc>> {
    let delta = TimeDelta::try_seconds(i64::try_from(seconds).ok()?)?;
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

fn provisioning_started(run: &RunSnapshot) -> RunTransition {
    if run.state != RunState::Pending {
        return RunTransition::unchanged(run);
    }
    let mut next = RunTransition::to(run, RunState::Provisioning);
    next.set.stamp_provisioning_started = true;
    next.with(RunEffect::SandboxProvisioning)
        .with(RunEffect::Publish)
}

fn sandbox_started(run: &RunSnapshot, pod_ip: String) -> RunTransition {
    if !matches!(run.state, RunState::Pending | RunState::Provisioning) {
        return RunTransition::unchanged(run);
    }
    let mut next = RunTransition::to(run, RunState::Connecting);
    next.set.pod_ip = Some(pod_ip);
    next.set.stamp_sandbox_started = true;
    if run.provisioning_began {
        next = next.with(RunEffect::SandboxProvisioned);
    }
    next.with(RunEffect::Publish)
}

/// The first connect moves an early run to `preparing` (to `finalizing` for a stop when one is
/// pending); a later one changes nothing.
fn connected(run: &RunSnapshot) -> RunTransition {
    if run.state.is_terminal() {
        return RunTransition::unchanged(run);
    }
    let early = run.state.is_unconnected();
    if !early && run.connected_at.is_some() {
        return RunTransition::unchanged(run);
    }
    // A stop accepted before the connection is carried out now, as `StopRequested` does for a
    // connected run.
    let stopped_early = early && run.stop_requested_at.is_some();
    let state = if stopped_early {
        RunState::Finalizing
    } else if early {
        RunState::Preparing
    } else {
        run.state
    };
    let mut next = RunTransition::to(run, state);
    next.set.finalize_reason = stopped_early.then_some(FinalizeReason::Stop);
    next.set.stamp_connected = run.connected_at.is_none();
    next.with(RunEffect::Publish)
}

/// A turn starts or ends. A `finalizing` or terminal run is never moved: once the control plane
/// decided to end it, the runtime cannot reopen it.
fn turn(run: &RunSnapshot, state: RunState, cue: TaskCue) -> RunTransition {
    if run.state.is_terminal() || run.state == RunState::Finalizing || run.state == state {
        return RunTransition::unchanged(run);
    }
    RunTransition::to(run, state)
        .with(RunEffect::Publish)
        .with(RunEffect::Task(cue))
}

/// `Finalize` moves a run to `finalizing` with the reason. The budget and a done task also take
/// over a run that is `finalizing` for `idle` (the release is under way, the `Cancel` follows); a
/// run `finalizing` for a stop, a done task or the budget keeps its reason.
fn finalize(run: &RunSnapshot, reason: FinalizeReason) -> RunTransition {
    let takes_over = matches!(reason, FinalizeReason::Budget | FinalizeReason::Done)
        && run.state == RunState::Finalizing
        && run.finalize_reason == Some(FinalizeReason::Idle);
    if takes_over {
        let mut next = RunTransition::unchanged(run);
        next.set.finalize_reason = Some(reason);
        return next.with(RunEffect::Publish);
    }
    let allowed = match reason {
        FinalizeReason::Idle => run.state == RunState::AwaitingInput && !run.undelivered_messages,
        FinalizeReason::Done => run.state == RunState::AwaitingInput,
        FinalizeReason::Budget => !run.state.is_terminal() && run.state != RunState::Finalizing,
        // A stop is `StopRequested`.
        FinalizeReason::Stop => false,
    };
    if !allowed {
        return RunTransition::unchanged(run);
    }
    let mut next = RunTransition::to(run, RunState::Finalizing);
    next.set.finalize_reason = Some(reason);
    next.with(RunEffect::Publish)
}

/// The stop marker goes on a run that has none. A connected run becomes `finalizing` with it; a
/// `finalizing` one (released, budget) is marked only for [`StopScope::IncludingFinalizing`]
/// (cancelling the task), and then its command is the stop `Cancel`.
fn stop_requested(run: &RunSnapshot, scope: StopScope) -> Result<RunTransition, Rejected> {
    if run.state.is_terminal() {
        return Err(Rejected::Stop(run.state));
    }
    let finalizing = run.state == RunState::Finalizing;
    if run.stop_requested_at.is_some() || (finalizing && scope == StopScope::ActiveOnly) {
        return Ok(RunTransition::unchanged(run));
    }
    let to_finalizing = finalizing || run.state.is_runtime_up();
    let state = if to_finalizing {
        RunState::Finalizing
    } else {
        run.state
    };
    let mut next = RunTransition::to(run, state);
    next.set.stamp_stop_requested = true;
    next.set.finalize_reason = to_finalizing.then_some(FinalizeReason::Stop);
    Ok(next.with(RunEffect::Publish))
}

fn ended(run: &RunSnapshot, end: RunEnd) -> RunTransition {
    if run.state.is_terminal() {
        return RunTransition::unchanged(run);
    }
    let mut next = RunTransition::to(run, end.outcome.run_state());
    if run.provisioning_began {
        next = next.with(RunEffect::SandboxClosed);
    }
    next.set.end = Some(end);
    next.with(RunEffect::Task(TaskCue::RunEnded))
        .with(RunEffect::Publish)
}

#[cfg(test)]
mod tests;
