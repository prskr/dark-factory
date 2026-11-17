//! Applies run events (ADR-0061): the one place that writes `runs.state`.
//!
//! [`apply_run_event`] locks the run row, loads its [`RunSnapshot`], asks
//! [`df_core::run_transition::run_transition`] what the event means, writes the result and
//! executes the effects, all in the caller's transaction. The rules live in `df-core`; this
//! module only reads, writes and locks.

use chrono::{DateTime, TimeDelta, Utc};
use df_core::run_transition::{
    FinalizeReason, RunEffect, RunEnd, RunEvent, RunSnapshot, RunTransition, TaskCue,
    run_transition,
};
use df_core::task_transition::{RunEnded, TaskEvent, Transition};
use df_core::{Principal, RunId, RunOutcome, RunState, TaskId, TaskStatus};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::lifecycle_events::{self, Event, EventSinks};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::runs::has_undelivered_message;
use crate::task_events::apply_task_event;
use crate::tasks::record_run_report;

static NO_EVENTS: EventSinks = EventSinks::disabled();

/// What applying an event may need besides the rows: the sinks of the sandbox lifecycle events
/// and the resume policy of the task event for a run end.
#[derive(Debug, Clone, Copy)]
pub struct RunEnv<'a> {
    pub events: &'a EventSinks,
    /// `runs.maxAutoResumes`.
    pub max_auto_resumes: u32,
    /// `scheduling.resumeBackoffSeconds`.
    pub resume_backoff: TimeDelta,
}

impl<'a> RunEnv<'a> {
    /// The lifecycle sinks and the resume policy of a run end.
    pub const fn new(
        events: &'a EventSinks,
        max_auto_resumes: u32,
        resume_backoff: TimeDelta,
    ) -> Self {
        Self {
            events,
            max_auto_resumes,
            resume_backoff,
        }
    }

    /// For events that enqueue sandbox lifecycle events but end no run (provisioning started,
    /// sandbox started): no resume policy.
    pub const fn events_only(events: &'a EventSinks) -> Self {
        Self::new(events, 0, TimeDelta::zero())
    }
}

impl RunEnv<'static> {
    /// For events that neither provision a sandbox nor end a run (turns, finalize, stop,
    /// connect): they need no sinks and no policy.
    pub const fn none() -> Self {
        Self::events_only(&NO_EVENTS)
    }
}

/// What [`apply_run_event`] did to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedRun {
    pub task_id: TaskId,
    /// The state of the run after the event.
    pub state: RunState,
    /// The event changed the run.
    pub changed: bool,
    /// The task's status after the event, if the event raised a task event for the run end.
    pub task_status: Option<TaskStatus>,
}

struct Facts {
    task_id: Uuid,
    state: String,
    stop_requested_at: Option<DateTime<Utc>>,
    connected_at: Option<DateTime<Utc>>,
    started_at: DateTime<Utc>,
    provisioning_started_at: Option<DateTime<Utc>>,
    sandbox_started_at: Option<DateTime<Utc>>,
    finalize_reason: Option<String>,
}

/// Applies `event` to the run and returns what changed; `None` if the run does not exist. A
/// command the state does not allow is [`DbError::Conflict`].
///
/// Locks the run row, then (for the task events a turn or the end raises) the task row, the order
/// of ADR-0055. `Finalize(Idle)` reads the undelivered messages: the caller holds the task row
/// lock first ([`crate::transcript::lock_run_and_task`]), so a message queued concurrently is
/// either seen or sees the run `finalizing`.
pub async fn apply_run_event(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    event: RunEvent,
    env: &RunEnv<'_>,
) -> Result<Option<AppliedRun>, DbError> {
    let Some(facts) = load_facts(tx, run_id).await? else {
        return Ok(None);
    };
    let task_id = TaskId::from_uuid(facts.task_id);
    let undelivered_messages = matches!(event, RunEvent::Finalize(FinalizeReason::Idle))
        && has_undelivered_message(tx, task_id, None).await?;
    let snapshot = RunSnapshot {
        run_id,
        state: facts.state.parse()?,
        stop_requested_at: facts.stop_requested_at,
        connected_at: facts.connected_at,
        provisioning_began: facts.provisioning_started_at.is_some(),
        finalize_reason: facts
            .finalize_reason
            .as_deref()
            .map(str::parse)
            .transpose()?,
        undelivered_messages,
        started_at: facts.started_at,
    };
    let next = run_transition(&snapshot, event)
        .map_err(|rejected| DbError::Conflict(rejected.to_string()))?;
    let mut applied = AppliedRun {
        task_id,
        state: next.state,
        changed: next.changes(),
        task_status: None,
    };
    if !applied.changed {
        return Ok(Some(applied));
    }
    let ended_at = write(tx, run_id, &next).await?;
    for effect in &next.effects {
        match effect {
            RunEffect::Publish => {
                publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await?;
            }
            RunEffect::SandboxProvisioning => {
                let event = Event::SandboxProvisioning { run_id };
                lifecycle_events::enqueue(tx, env.events, &event).await?;
            }
            RunEffect::SandboxProvisioned => {
                let event = Event::SandboxProvisioned { run_id };
                lifecycle_events::enqueue(tx, env.events, &event).await?;
            }
            RunEffect::SandboxClosed => {
                if let Some(event) = sandbox_closed(&next, &facts, ended_at) {
                    lifecycle_events::enqueue(tx, env.events, &event).await?;
                }
            }
            RunEffect::Task(TaskCue::TurnStarted) => {
                let actor = Principal::system("run.turn");
                raise_task_event(tx, task_id, TaskEvent::TurnStarted, &actor).await?;
            }
            RunEffect::Task(TaskCue::TurnEnded) => {
                let actor = Principal::system("run.turn");
                raise_task_event(tx, task_id, TaskEvent::TurnEnded, &actor).await?;
            }
            RunEffect::Task(TaskCue::RunEnded) => {
                if let Some(end) = &next.set.end {
                    applied.task_status =
                        Some(raise_run_ended(tx, task_id, end, &snapshot, env).await?);
                }
            }
        }
    }
    Ok(Some(applied))
}

async fn load_facts(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
) -> Result<Option<Facts>, DbError> {
    Ok(sqlx::query_as!(
        Facts,
        r#"
        SELECT task_id, state, stop_requested_at, connected_at, started_at,
               provisioning_started_at, sandbox_started_at, finalize_reason
        FROM runs WHERE id = $1 FOR UPDATE
        "#,
        run_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?)
}

/// Writes the state and the fields of the transition; returns `ended_at` for an end.
async fn write(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    next: &RunTransition,
) -> Result<Option<DateTime<Utc>>, DbError> {
    let set = &next.set;
    let end = set.end.as_ref();
    let row = sqlx::query!(
        r#"
        UPDATE runs SET
            state = $2,
            pod_ip = COALESCE($3, pod_ip),
            provisioning_started_at =
                CASE WHEN $4 THEN COALESCE(provisioning_started_at, now())
                     ELSE provisioning_started_at END,
            sandbox_started_at =
                CASE WHEN $5 THEN COALESCE(sandbox_started_at, now())
                     ELSE sandbox_started_at END,
            connected_at =
                CASE WHEN $6 THEN COALESCE(connected_at, now()) ELSE connected_at END,
            stop_requested_at =
                CASE WHEN $7 THEN COALESCE(stop_requested_at, clock_timestamp())
                     ELSE stop_requested_at END,
            finalize_reason = COALESCE($8, finalize_reason),
            outcome = COALESCE($9, outcome),
            detail = CASE WHEN $9::text IS NULL THEN detail ELSE $10 END,
            retry_after = CASE WHEN $9::text IS NULL THEN retry_after ELSE $11 END,
            harness_session_id = CASE WHEN $12 <> '' THEN $12 ELSE harness_session_id END,
            task_branch_sha = CASE WHEN $9::text IS NULL THEN task_branch_sha ELSE $13 END,
            wip_sha = CASE WHEN $9::text IS NULL THEN wip_sha ELSE $14 END,
            ended_at = CASE WHEN $9::text IS NULL THEN ended_at ELSE now() END
        WHERE id = $1
        RETURNING ended_at
        "#,
        run_id.as_uuid(),
        next.state.as_str(),
        set.pod_ip.as_deref(),
        set.stamp_provisioning_started,
        set.stamp_sandbox_started,
        set.stamp_connected,
        set.stamp_stop_requested,
        set.finalize_reason.map(FinalizeReason::as_str),
        end.map(|e| e.outcome.as_str()),
        end.map_or("", |e| e.detail.as_str()),
        end.and_then(|e| e.retry_after),
        end.map_or("", |e| e.harness_session_id.as_str()),
        end.map_or("", |e| e.task_branch_sha.as_str()),
        end.map_or("", |e| e.wip_sha.as_str()),
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.ended_at)
}

/// The closing sandbox event of a run whose provisioning began (design D2 pairing).
fn sandbox_closed(
    next: &RunTransition,
    facts: &Facts,
    ended_at: Option<DateTime<Utc>>,
) -> Option<Event> {
    let end = next.set.end.as_ref()?;
    if end.outcome == RunOutcome::ProvisioningFailed {
        return Some(Event::SandboxProvisioningFailed {
            run_id: end.run_id,
            error: end.detail.clone(),
        });
    }
    Some(Event::SandboxDestroyed {
        run_id: end.run_id,
        outcome: end.outcome,
        started_at: facts.sandbox_started_at,
        ended_at: ended_at.unwrap_or_else(Utc::now),
    })
}

/// Applies a task event for the run. Boxed: the task applier reaches back here through its
/// effects `FinalizeIdleRun` and `StopRun`, and a recursive `async fn` needs the indirection.
async fn raise_task_event(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    event: TaskEvent,
    actor: &Principal,
) -> Result<Transition, DbError> {
    Box::pin(apply_task_event(tx, task_id, event, actor)).await
}

/// Raises `RunEnded` on the task and records the session id and divergence the run reported.
/// The applier locks the task row, as `queue_message` does before it inserts: a message queued
/// concurrently is visible to the rules, instead of the task becoming `waiting` with it
/// stranded.
async fn raise_run_ended(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    end: &RunEnd,
    run: &RunSnapshot,
    env: &RunEnv<'_>,
) -> Result<TaskStatus, DbError> {
    let event = TaskEvent::RunEnded(RunEnded {
        run_id: end.run_id,
        outcome: end.outcome,
        detail: end.detail.clone(),
        retry_after: end.retry_after,
        task_branch_pushed: end.task_branch_pushed,
        stop_requested_at: run.stop_requested_at,
        max_auto_resumes: env.max_auto_resumes,
        resume_backoff: env.resume_backoff,
        now: Utc::now(),
    });
    let next = raise_task_event(tx, task_id, event, &Principal::system("run.ended")).await?;
    record_run_report(&mut **tx, task_id, &end.harness_session_id, end.diverged).await?;
    Ok(next.status)
}

#[cfg(test)]
mod tests;
