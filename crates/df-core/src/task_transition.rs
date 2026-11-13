//! The task status machine as one pure function (ADR-0055).
//!
//! [`transition`] takes what the rows of a task show ([`TaskSnapshot`]) and what happened
//! ([`TaskEvent`]) and returns the new status, resume time and resume count plus the
//! [`Effect`]s the caller executes in its transaction. `df_db::apply_task_event` loads the
//! snapshot, calls it and executes the result; no other code writes `tasks.status`.
//!
//! The status diagram and the events are in `crates/df-core/docs/task-transitions.md`.

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;

use crate::domain::{CAPACITY_DETAIL, Principal, RunOutcome, TaskStatus, system_reason};
use crate::ids::RunId;

/// Resume time of a task whose run reported a rate limit without a retry time.
const UNKNOWN_RETRY_AFTER: TimeDelta = TimeDelta::hours(1);
/// What the task's own rows show when an event is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub status: TaskStatus,
    pub resume_at: Option<DateTime<Utc>>,
    pub auto_resumes: i32,
    pub active_run: ActiveRun,
    /// The task has had a run. Dependencies gate only the first start.
    pub ever_ran: bool,
    /// Dependencies whose target has not satisfied them yet.
    pub unsatisfied_dependencies: u32,
    /// A message is undelivered. For the end of a run that was stopped, only messages queued
    /// after the stop count: the earlier ones wait for the next message.
    pub undelivered_messages: bool,
    pub pr: PrState,
}

impl TaskSnapshot {
    /// The snapshot of a task that is being created and has no row yet.
    pub const fn new_task() -> Self {
        Self {
            status: TaskStatus::Queued,
            resume_at: None,
            auto_resumes: 0,
            active_run: ActiveRun::None,
            ever_ran: false,
            unsatisfied_dependencies: 0,
            undelivered_messages: false,
            pr: PrState::None,
        }
    }
}

/// The task's run, as far as the status rules care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveRun {
    /// No run that has not ended.
    None,
    /// A run that takes new messages: not finalizing and not stopped.
    Connected,
    /// A run that is ending (`complete_task`, release).
    Finalizing,
    /// A board member stopped the run.
    StopRequested,
}

/// The task's pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    None,
    Unmerged,
    Merged,
}

/// Who marks a task done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoneBy {
    /// A board member. Rejected while a run works or is about to start.
    Board,
    /// The merge of the task's pull request by `login`; applies in every status but the final
    /// two.
    Merge { login: String },
}

/// The facts of a run end the status rules read. `now` and the limits are fields so the
/// function stays pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEnded {
    pub run_id: RunId,
    pub outcome: RunOutcome,
    pub detail: String,
    pub retry_after: Option<DateTime<Utc>>,
    /// The runtime pushed the task branch.
    pub task_branch_pushed: bool,
    /// When a board member stopped the run, if one did. Messages queued before it wait for the
    /// next message.
    pub stop_requested_at: Option<DateTime<Utc>>,
    /// `runs.maxAutoResumes`.
    pub max_auto_resumes: u32,
    /// `scheduling.resumeBackoffSeconds`: the backoff per resume attempt after an infrastructure
    /// failure, and the wait after a capacity rejection.
    pub resume_backoff: TimeDelta,
    pub now: DateTime<Utc>,
}

/// How a child task ended, as its parent hears about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildEnd {
    /// The agent called `complete_task` and the run ended.
    Completed,
    /// A board member marked the task done, or its pull request was merged.
    Done,
    Failed,
    Cancelled,
}

/// What a parent is told when a child ends. The applier fills it from the child's rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEnded {
    pub key: String,
    pub title: String,
    pub how: ChildEnd,
    /// The failure reason of the child's last run.
    pub detail: String,
    /// The completion summary of the child's last run, written by its agent.
    pub summary: String,
    pub pr_url: Option<String>,
    /// The child's artifacts as `<key>/<name>`, newest first. Those of its descendants are not
    /// listed.
    pub artifacts: Vec<String>,
    /// Keys of the tasks that still await a dependency on the child.
    pub waiting_dependents: Vec<String>,
}

/// Something that happened to a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    Created,
    MessageQueued,
    /// A queued message was deleted.
    MessageWithdrawn,
    RunStarted,
    /// The run left its idle phase for a turn.
    TurnStarted,
    /// The run is idle: its turn ended.
    TurnEnded,
    RunEnded(RunEnded),
    MarkedDone(DoneBy),
    Cancelled,
    /// A task this one depends on completed or became done (an effect of that task's event).
    DependencySatisfied,
    /// A child of this task ended (an effect of the child's event).
    ChildEnded(ChildEnded),
}

/// A command the task's status does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("a {0} task cannot be marked done")]
    MarkDone(TaskStatus),
    #[error("a {0} task cannot be cancelled")]
    Cancel(TaskStatus),
}

/// Which runs a [`Effect::StopRun`] marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopScope {
    /// A `finalizing` run is left alone.
    ActiveOnly,
    /// A `finalizing` run is marked as well (cancelling the task).
    IncludingFinalizing,
}

/// A change notification for the transitioned task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    TaskChanged,
    TaskMessageQueued,
    TaskMessageChanged,
}

/// The first time a task completed or became done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum First {
    Completion,
    Done,
}

/// Something the applier does in the transaction of the event, in the order of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Enqueue `pr.sync`. `ready` is stamped with the database clock under the task lock.
    EnqueuePrSync {
        run_id: Option<RunId>,
        open: bool,
        ready: bool,
    },
    /// Enqueue `pr.mark_draft`.
    EnqueueMarkDraft,
    /// An idle run (`awaiting_input`) becomes `finalizing`.
    FinalizeIdleRun,
    StopRun(StopScope),
    /// Discard the undelivered messages.
    DiscardMessages,
    /// Close the pending agent requests with `reason`.
    CloseRequests {
        reason: &'static str,
    },
    Audit {
        action: &'static str,
        details: serde_json::Value,
    },
    Publish(Notice),
    /// Set `first_completed_at` or `first_done_at` if it is null.
    RecordFirst(First),
    /// Raise `DependencySatisfied` on every task that depends on this one.
    NotifyDependents,
    /// Raise `ChildEnded` on the parent, if there is one.
    NotifyParent(ChildEnd),
    /// Queue a message on the task and apply `MessageQueued` for it.
    QueueMessage {
        author: Principal,
        text: String,
    },
}

/// The result of an event: the task's new status data and the effects. `status`, `resume_at` and
/// `auto_resumes` equal the snapshot's when the event changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub status: TaskStatus,
    pub resume_at: Option<DateTime<Utc>>,
    pub auto_resumes: i32,
    /// Write the row (and so bump `updated_at`, which orders the scheduler's queue) even when
    /// status, resume time and count stay.
    pub touch: bool,
    pub effects: Vec<Effect>,
}

impl Transition {
    /// An event that changes nothing.
    pub const fn unchanged(task: &TaskSnapshot) -> Self {
        Self {
            status: task.status,
            resume_at: task.resume_at,
            auto_resumes: task.auto_resumes,
            touch: false,
            effects: Vec::new(),
        }
    }

    fn to(task: &TaskSnapshot, status: TaskStatus) -> Self {
        Self {
            status,
            ..Self::unchanged(task)
        }
    }

    fn with(mut self, effect: Effect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Whether status, resume time or resume count differ from the snapshot's.
    pub fn changes(&self, task: &TaskSnapshot) -> bool {
        (self.status, self.resume_at, self.auto_resumes)
            != (task.status, task.resume_at, task.auto_resumes)
    }
}

/// The new state of `task` after `event`, or the reason the status does not allow it.
pub fn transition(task: &TaskSnapshot, event: TaskEvent) -> Result<Transition, Rejected> {
    Ok(match event {
        TaskEvent::Created => created(task),
        TaskEvent::MessageQueued => message_queued(task),
        TaskEvent::MessageWithdrawn => message_withdrawn(task),
        TaskEvent::RunStarted => run_started(task),
        TaskEvent::TurnStarted => turn_started(task),
        TaskEvent::TurnEnded => turn_ended(task),
        TaskEvent::RunEnded(end) => run_ended(task, &end),
        TaskEvent::MarkedDone(by) => return marked_done(task, by),
        TaskEvent::Cancelled => return cancelled(task),
        TaskEvent::DependencySatisfied => dependency_satisfied(task),
        TaskEvent::ChildEnded(child) => child_ended(task, &child),
    })
}

/// Task status for a finished run, before the "undelivered prompts" override. `completed` is
/// reserved for a human (mark done), so the agent's `completed` outcome puts the task in review.
pub const fn task_status_for(outcome: RunOutcome) -> TaskStatus {
    match outcome {
        RunOutcome::Completed => TaskStatus::InReview,
        RunOutcome::EndTurn | RunOutcome::Cancelled => TaskStatus::Waiting,
        RunOutcome::Blocked => TaskStatus::Blocked,
        RunOutcome::LimitReached => TaskStatus::LimitReached,
        RunOutcome::RateLimited => TaskStatus::RateLimited,
        RunOutcome::Failed
        | RunOutcome::ProvisioningFailed
        | RunOutcome::ConnectTimeout
        | RunOutcome::SandboxLost => TaskStatus::Failed,
    }
}

/// Outcomes after which an undelivered message does not requeue the task: the failure or the
/// rate limit has to be handled first.
pub const fn keeps_status_on_pending_prompts(outcome: RunOutcome) -> bool {
    matches!(
        outcome,
        RunOutcome::Failed
            | RunOutcome::ProvisioningFailed
            | RunOutcome::ConnectTimeout
            | RunOutcome::SandboxLost
            | RunOutcome::RateLimited
    )
}

const fn is_infrastructure_failure(outcome: RunOutcome) -> bool {
    matches!(
        outcome,
        RunOutcome::ProvisioningFailed | RunOutcome::ConnectTimeout | RunOutcome::SandboxLost
    )
}

/// A task with an unsatisfied dependency waits for it; every other new task is queued.
fn created(task: &TaskSnapshot) -> Transition {
    if task.unsatisfied_dependencies > 0 {
        return Transition::to(task, TaskStatus::AwaitingDependencies)
            .with(Effect::Publish(Notice::TaskChanged));
    }
    Transition::to(task, TaskStatus::Queued)
        .with(Effect::Publish(Notice::TaskChanged))
        .with(Effect::Publish(Notice::TaskMessageQueued))
}

/// A message goes to the connected run; otherwise the scheduler starts a run for it, unless the
/// task never ran and still waits for a dependency. A message
/// that moves the task out of `in_review` or `completed` with an unmerged pull request marks the
/// pull request as draft again.
fn message_queued(task: &TaskSnapshot) -> Transition {
    let status = if !task.ever_ran && task.unsatisfied_dependencies > 0 {
        // Dependencies gate the first start; the message is delivered in the first run.
        TaskStatus::AwaitingDependencies
    } else if task.active_run == ActiveRun::Connected {
        TaskStatus::Running
    } else {
        TaskStatus::Queued
    };
    let mut next = Transition {
        status,
        resume_at: None,
        auto_resumes: 0,
        touch: true,
        effects: Vec::new(),
    };
    if matches!(task.status, TaskStatus::InReview | TaskStatus::Completed)
        && task.pr == PrState::Unmerged
    {
        next = next.with(Effect::EnqueueMarkDraft);
    }
    next.with(Effect::Publish(Notice::TaskMessageQueued))
        .with(Effect::Publish(Notice::TaskChanged))
}

/// A `queued` task left without an undelivered message returns to `waiting`.
fn message_withdrawn(task: &TaskSnapshot) -> Transition {
    if task.status == TaskStatus::Queued && !task.undelivered_messages {
        return Transition::to(task, TaskStatus::Waiting)
            .with(Effect::Publish(Notice::TaskChanged));
    }
    Transition::unchanged(task)
}

/// The scheduler only starts due tasks (`queued`, `rate_limited`), so any status becomes
/// `running` here.
fn run_started(task: &TaskSnapshot) -> Transition {
    Transition {
        status: TaskStatus::Running,
        resume_at: None,
        touch: true,
        ..Transition::unchanged(task)
    }
    .with(Effect::Publish(Notice::TaskChanged))
}

/// `waiting` → `running`: the idle task is woken by the next turn.
fn turn_started(task: &TaskSnapshot) -> Transition {
    let next = if task.status == TaskStatus::Waiting {
        Transition::to(task, TaskStatus::Running)
    } else {
        Transition::unchanged(task)
    };
    next.with(Effect::Publish(Notice::TaskChanged))
}

/// `running` → `waiting`, unless a message is undelivered: then the task stays `running` and the
/// session forwards it.
fn turn_ended(task: &TaskSnapshot) -> Transition {
    let next = if task.status == TaskStatus::Running && !task.undelivered_messages {
        Transition::to(task, TaskStatus::Waiting)
    } else {
        Transition::unchanged(task)
    };
    next.with(Effect::Publish(Notice::TaskChanged))
}

/// The outcome picks the status. A cancelled task stays cancelled, a task marked done stays done,
/// and a message that arrived meanwhile (or reopened the task after a stop) keeps it `queued`.
fn run_ended(task: &TaskSnapshot, end: &RunEnded) -> Transition {
    let overridden = if task.status == TaskStatus::Cancelled {
        Some(TaskStatus::Cancelled)
    } else if task.undelivered_messages && !keeps_status_on_pending_prompts(end.outcome) {
        Some(TaskStatus::Queued)
    } else if task.status == TaskStatus::Completed {
        Some(TaskStatus::Completed)
    } else if task.status == TaskStatus::Queued && end.stop_requested_at.is_some() {
        Some(TaskStatus::Queued)
    } else {
        None
    };
    let mut next = plan_resume(end, overridden, task.auto_resumes);
    next.touch = true;
    let completed = end.outcome == RunOutcome::Completed;
    // A cancelled task keeps its PR as it is.
    if task.status != TaskStatus::Cancelled && (end.task_branch_pushed || completed) {
        next = next.with(Effect::EnqueuePrSync {
            run_id: Some(end.run_id),
            open: end.task_branch_pushed,
            ready: completed,
        });
    }
    // A cancelled task did not complete; a task already done told its parent when it got done.
    if completed && task.status != TaskStatus::Cancelled {
        next = next
            .with(Effect::RecordFirst(First::Completion))
            .with(Effect::NotifyDependents);
        if task.status != TaskStatus::Completed {
            next = next.with(Effect::NotifyParent(ChildEnd::Completed));
        }
    } else if next.status == TaskStatus::Failed {
        next = next.with(Effect::NotifyParent(ChildEnd::Failed));
    }
    let queued = next.status == TaskStatus::Queued;
    next = next.with(Effect::Publish(Notice::TaskChanged));
    if queued {
        next = next.with(Effect::Publish(Notice::TaskMessageQueued));
    }
    next
}

/// Status, resume time and resume count after a run end. `overridden` is a status the other
/// rules already chose; it keeps its status and gets no resume time. The count is reset by every
/// end except an infrastructure failure, which keeps it (or increments it when the task
/// resumes).
fn plan_resume(end: &RunEnded, overridden: Option<TaskStatus>, auto_resumes: i32) -> Transition {
    let plan = |status, resume_at, auto_resumes| Transition {
        status,
        resume_at,
        auto_resumes,
        touch: false,
        effects: Vec::new(),
    };
    if let Some(status) = overridden {
        let kept = if is_infrastructure_failure(end.outcome) {
            auto_resumes
        } else {
            0
        };
        return plan(status, None, kept);
    }
    let later = |delta: TimeDelta| end.now.checked_add_signed(delta).unwrap_or(end.now);
    if end.outcome == RunOutcome::RateLimited {
        let at = end
            .retry_after
            .unwrap_or_else(|| later(UNKNOWN_RETRY_AFTER));
        return plan(TaskStatus::RateLimited, Some(at), 0);
    }
    if !is_infrastructure_failure(end.outcome) {
        return plan(task_status_for(end.outcome), None, 0);
    }
    if end.outcome == RunOutcome::ProvisioningFailed && end.detail == CAPACITY_DETAIL {
        return plan(
            TaskStatus::Queued,
            Some(later(end.resume_backoff)),
            auto_resumes,
        );
    }
    if auto_resumes < i32::try_from(end.max_auto_resumes).unwrap_or(i32::MAX) {
        return plan(
            TaskStatus::Queued,
            Some(later(end.resume_backoff * (auto_resumes + 1))),
            auto_resumes + 1,
        );
    }
    plan(TaskStatus::Failed, None, auto_resumes)
}

fn marked_done(task: &TaskSnapshot, by: DoneBy) -> Result<Transition, Rejected> {
    let mut next = Transition {
        status: TaskStatus::Completed,
        resume_at: None,
        ..Transition::unchanged(task)
    };
    match by {
        DoneBy::Board => {
            if !task.status.can_mark_done() {
                return Err(Rejected::MarkDone(task.status));
            }
            if task.pr != PrState::None {
                next = next.with(Effect::EnqueuePrSync {
                    run_id: None,
                    open: false,
                    ready: true,
                });
            }
            Ok(next
                .with(Effect::FinalizeIdleRun)
                .with(Effect::CloseRequests {
                    reason: "task-completed",
                })
                .with(Effect::Audit {
                    action: "task.done",
                    details: json!({ "old_status": task.status }),
                })
                .with(Effect::Publish(Notice::TaskChanged))
                .with(Effect::RecordFirst(First::Done))
                .with(Effect::NotifyDependents)
                .with(Effect::NotifyParent(ChildEnd::Done)))
        }
        DoneBy::Merge { login } => {
            if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled) {
                return Ok(Transition::unchanged(task));
            }
            Ok(next
                .with(Effect::StopRun(StopScope::ActiveOnly))
                .with(Effect::CloseRequests {
                    reason: "task-completed",
                })
                .with(Effect::Audit {
                    action: "task.done",
                    details: json!({ "old_status": task.status, "login": login }),
                })
                .with(Effect::Publish(Notice::TaskChanged))
                .with(Effect::RecordFirst(First::Done))
                .with(Effect::NotifyDependents)
                .with(Effect::NotifyParent(ChildEnd::Done)))
        }
    }
}

fn cancelled(task: &TaskSnapshot) -> Result<Transition, Rejected> {
    if !task.status.can_cancel() {
        return Err(Rejected::Cancel(task.status));
    }
    Ok(Transition {
        status: TaskStatus::Cancelled,
        resume_at: None,
        ..Transition::unchanged(task)
    }
    .with(Effect::DiscardMessages)
    .with(Effect::StopRun(StopScope::IncludingFinalizing))
    .with(Effect::CloseRequests {
        reason: "task-cancelled",
    })
    .with(Effect::Audit {
        action: "task.cancel",
        details: json!({ "old_status": task.status }),
    })
    .with(Effect::Publish(Notice::TaskMessageChanged))
    .with(Effect::Publish(Notice::TaskChanged))
    .with(Effect::NotifyParent(ChildEnd::Cancelled)))
}

/// The last dependency of a task that never started releases it; its assignment prompt is queued
/// already. The dependency list changes either way, so the UI is told.
fn dependency_satisfied(task: &TaskSnapshot) -> Transition {
    if task.status == TaskStatus::AwaitingDependencies && task.unsatisfied_dependencies == 0 {
        return Transition::to(task, TaskStatus::Queued)
            .with(Effect::Publish(Notice::TaskChanged))
            .with(Effect::Publish(Notice::TaskMessageQueued));
    }
    Transition::unchanged(task).with(Effect::Publish(Notice::TaskChanged))
}

/// A parent that is done, cancelled or failed is not woken; any other parent gets the message.
fn child_ended(task: &TaskSnapshot, child: &ChildEnded) -> Transition {
    if matches!(
        task.status,
        TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed
    ) {
        return Transition::unchanged(task);
    }
    Transition::unchanged(task).with(Effect::QueueMessage {
        author: Principal::system(system_reason::CHILD_ENDED),
        text: child_ended_text(child),
    })
}

/// The text of the message a parent gets when a child ends. The child's own words are quoted
/// under a line naming their source: they are untrusted input of another agent.
pub fn child_ended_text(child: &ChildEnded) -> String {
    let ChildEnded { key, title, .. } = child;
    let ended = match child.how {
        ChildEnd::Completed => "completed",
        ChildEnd::Done => "is done",
        ChildEnd::Failed => "failed",
        ChildEnd::Cancelled => "was cancelled",
    };
    let mut lines = vec![format!("{key} \"{title}\" {ended}.")];
    if child.how == ChildEnd::Failed && !child.detail.is_empty() {
        lines.push(format!("Reason: {}", child.detail));
    }
    if let Some(url) = &child.pr_url {
        lines.push(format!("PR: {url}"));
    }
    lines.push(if child.artifacts.is_empty() {
        "Artifacts: none".to_string()
    } else {
        format!("Artifacts: {}", child.artifacts.join(", "))
    });
    if !child.waiting_dependents.is_empty() {
        let waiting = child.waiting_dependents.join(", ");
        lines.push(format!("Still waiting on {key}: {waiting}"));
    }
    if !child.summary.is_empty() {
        lines.push(format!("\nSummary from the agent of {key}:"));
        lines.extend(child.summary.lines().map(|line| format!("> {line}")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
