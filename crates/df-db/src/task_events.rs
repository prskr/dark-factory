//! Applies task events (ADR-0055): the one place that writes `tasks.status`.
//!
//! [`apply_task_event`] locks the task, loads its [`TaskSnapshot`], asks
//! [`df_core::task_transition::transition`] what the event means, writes the result and executes
//! the effects, all in the caller's transaction. The rules live in `df-core`; this module only
//! reads, writes and locks.

use chrono::{DateTime, Utc};
use std::collections::VecDeque;

use df_core::task_transition::{
    ActiveRun, ChildEnd, ChildEnded, Effect, First, Notice, PrState, TaskEvent, TaskSnapshot,
    Transition, transition,
};
use df_core::{Principal, RunId, TaskId, is_satisfied};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::tasks::{
    MARK_DRAFT_KIND, MarkDraftJob, PR_SYNC_KIND, PrSyncJob, clock_under_task_lock,
    lock_runs_then_task, request_stop_tx,
};

/// Work that an effect leaves for another task, applied after the current task's writes.
pub(crate) enum Followup {
    Event(TaskId, TaskEvent),
    /// The parent hears about a child. The message is built when the parent's turn comes, after
    /// the child's dependents were released.
    ChildEnded {
        child: TaskId,
        parent: TaskId,
        how: ChildEnd,
    },
}

/// Applies `event` to the task and returns what changed. `actor` is the principal of the audit
/// entry and of the requests the transition closes.
///
/// Locks the task row, after its run rows for events a board member or the merge raises
/// (`MarkedDone`, `Cancelled`); every other event locks the task row only, which the caller may
/// hold already. A command the status does not allow is [`DbError::Conflict`].
///
/// Effects on other tasks (dependents, then the parent) are applied in the same transaction from
/// a work queue. They lock only the other task's row, never its run rows (ADR-0055).
pub async fn apply_task_event(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    event: TaskEvent,
    actor: &Principal,
) -> Result<Transition, DbError> {
    let (first, followups) = apply_one(tx, task_id, event, actor).await?;
    let mut queue = VecDeque::from(followups);
    let cascade = Principal::system("task.cascade");
    while let Some(followup) = queue.pop_front() {
        let (target, event) = match followup {
            Followup::Event(target, event) => (target, event),
            Followup::ChildEnded { child, parent, how } => (
                parent,
                TaskEvent::ChildEnded(load_child_ended(tx, child, how).await?),
            ),
        };
        let (_, more) = apply_one(tx, target, event, &cascade).await?;
        queue.extend(more);
    }
    Ok(first)
}

async fn apply_one(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    event: TaskEvent,
    actor: &Principal,
) -> Result<(Transition, Vec<Followup>), DbError> {
    if matches!(event, TaskEvent::MarkedDone(_) | TaskEvent::Cancelled) {
        lock_runs_then_task(tx, task_id).await?;
    }
    let after = match &event {
        TaskEvent::RunEnded(end) => end.stop_requested_at,
        _ => None,
    };
    let snapshot = load_snapshot(tx, task_id, after).await?;
    let next =
        transition(&snapshot, event).map_err(|rejected| DbError::Conflict(rejected.to_string()))?;
    if next.touch || next.changes(&snapshot) {
        write_status(tx, task_id, &next).await?;
    }
    let followups = execute_effects(tx, task_id, &next, actor).await?;
    Ok((next, followups))
}

/// Executes the effects of a transition for the task `task_id`, in order. The caller holds the
/// task row lock. Returns the work for other tasks.
pub(crate) async fn execute_effects(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    next: &Transition,
    actor: &Principal,
) -> Result<Vec<Followup>, DbError> {
    let mut discarded = None;
    let mut followups = Vec::new();
    for effect in &next.effects {
        match effect {
            Effect::EnqueuePrSync {
                run_id,
                open,
                ready,
            } => {
                let ready = if *ready {
                    Some(clock_under_task_lock(tx).await?)
                } else {
                    None
                };
                let job = PrSyncJob {
                    task_id,
                    run_id: *run_id,
                    open: *open,
                    ready,
                };
                crate::outbox::enqueue(tx, PR_SYNC_KIND, &serde_json::to_value(job)?).await?;
            }
            Effect::EnqueueMarkDraft => {
                let job = serde_json::to_value(MarkDraftJob { task_id })?;
                crate::outbox::enqueue(tx, MARK_DRAFT_KIND, &job).await?;
            }
            Effect::FinalizeIdleRun => finalize_idle_run(tx, task_id).await?,
            Effect::StopRun(scope) => {
                request_stop_tx(tx, task_id, *scope).await?;
            }
            Effect::DiscardMessages => discarded = Some(discard_messages(tx, task_id).await?),
            Effect::CloseRequests { reason } => {
                crate::requests::close_pending_for_task(tx, task_id, reason, actor).await?;
            }
            Effect::Audit { action, details } => {
                let mut details = details.clone();
                if let (Some(count), Some(object)) = (discarded, details.as_object_mut()) {
                    object.insert("discarded_messages".into(), count.into());
                }
                let entry = NewAuditEntry::new(
                    actor.clone(),
                    Vec::new(),
                    *action,
                    "task",
                    task_id.to_string(),
                )
                .with_details(&details)?;
                write_audit_entry(&mut **tx, &entry).await?;
            }
            Effect::Publish(notice) => {
                publish(&mut **tx, &notification(*notice, task_id)).await?;
            }
            Effect::RecordFirst(first) => record_first(tx, task_id, *first).await?,
            Effect::NotifyDependents => {
                for dependent in dependents(tx, task_id).await? {
                    followups.push(Followup::Event(dependent, TaskEvent::DependencySatisfied));
                }
            }
            Effect::NotifyParent(how) => {
                let parent: Option<Uuid> =
                    sqlx::query_scalar("SELECT parent_id FROM tasks WHERE id = $1")
                        .bind(task_id.as_uuid())
                        .fetch_one(&mut **tx)
                        .await?;
                if let Some(parent) = parent {
                    followups.push(Followup::ChildEnded {
                        child: task_id,
                        parent: TaskId::from_uuid(parent),
                        how: *how,
                    });
                }
            }
            Effect::QueueMessage { author, text } => {
                let content = vec![crate::tasks::text_block(text.clone(), false)];
                // The message arm sets the status: async recursion needs the box.
                Box::pin(crate::tasks::queue_message_tx(
                    tx,
                    task_id,
                    author.clone(),
                    content,
                ))
                .await?;
            }
        }
    }
    Ok(followups)
}

async fn record_first(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    first: First,
) -> Result<(), DbError> {
    let sql = match first {
        First::Completion => {
            "UPDATE tasks SET first_completed_at = COALESCE(first_completed_at, now()) WHERE id = $1"
        }
        First::Done => {
            "UPDATE tasks SET first_done_at = COALESCE(first_done_at, now()) WHERE id = $1"
        }
    };
    sqlx::query(sql)
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn dependents(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<Vec<TaskId>, DbError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT task_id FROM task_dependencies WHERE depends_on_task_id = $1 ORDER BY task_id",
    )
    .bind(task_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    Ok(ids.into_iter().map(TaskId::from_uuid).collect())
}

#[derive(sqlx::FromRow)]
struct ChildFacts {
    key: String,
    title: String,
    pr_url: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LastRun {
    detail: String,
    completion_summary: String,
}

/// What the parent is told about the child, read from the child's rows as they are now.
async fn load_child_ended(
    tx: &mut Transaction<'_, Postgres>,
    child: TaskId,
    how: ChildEnd,
) -> Result<ChildEnded, DbError> {
    let facts: ChildFacts = sqlx::query_as("SELECT key, title, pr_url FROM tasks WHERE id = $1")
        .bind(child.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
    let last_run: Option<LastRun> = sqlx::query_as(
        "SELECT detail, completion_summary FROM runs WHERE task_id = $1
         ORDER BY number DESC LIMIT 1",
    )
    .bind(child.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let (detail, summary) =
        last_run.map_or_else(Default::default, |run| (run.detail, run.completion_summary));
    // Tasks whose dependency on the child is still unsatisfied.
    let (completed_at, done_at): (Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT first_completed_at, first_done_at FROM tasks WHERE id = $1")
            .bind(child.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    let dependents: Vec<(String, String)> = sqlx::query_as(
        "SELECT t.key, d.kind FROM task_dependencies d JOIN tasks t ON t.id = d.task_id
         WHERE d.depends_on_task_id = $1 AND t.status = 'awaiting_dependencies'
         ORDER BY t.number",
    )
    .bind(child.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    let mut waiting_dependents = Vec::new();
    for (key, kind) in dependents {
        if !dependency_satisfied(&kind, completed_at, done_at)? {
            waiting_dependents.push(key);
        }
    }
    Ok(ChildEnded {
        key: facts.key,
        title: facts.title,
        how,
        detail,
        summary,
        pr_url: facts.pr_url,
        waiting_dependents,
    })
}

/// Whether a dependency of the stored `kind` is satisfied by a target with these first-times.
fn dependency_satisfied(
    kind: &str,
    first_completed_at: Option<DateTime<Utc>>,
    first_done_at: Option<DateTime<Utc>>,
) -> Result<bool, DbError> {
    Ok(is_satisfied(
        kind.parse()?,
        first_completed_at,
        first_done_at,
    ))
}

fn notification(notice: Notice, task_id: TaskId) -> Notification {
    match notice {
        Notice::TaskChanged => Notification::TaskChanged { task_id },
        Notice::TaskMessageQueued => Notification::TaskMessageQueued { task_id },
        Notice::TaskMessageChanged => Notification::TaskMessageChanged { task_id },
    }
}

/// A dependency of a task with the times its target completed and got done.
#[derive(sqlx::FromRow)]
struct DependencyTimes {
    kind: String,
    first_completed_at: Option<DateTime<Utc>>,
    first_done_at: Option<DateTime<Utc>>,
}

/// What the snapshot reads from the task row.
#[derive(sqlx::FromRow)]
struct TaskRowFacts {
    status: String,
    resume_at: Option<DateTime<Utc>>,
    auto_resumes: i32,
    has_pr: bool,
    merged: bool,
}

/// Locks the task row and reads what the rules need. With `after`, only messages queued after
/// that time count as undelivered: for a run a board member stopped, the earlier ones wait for
/// the next message.
async fn load_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    after: Option<DateTime<Utc>>,
) -> Result<TaskSnapshot, DbError> {
    let row: Option<TaskRowFacts> = sqlx::query_as(
        "SELECT status, resume_at, auto_resumes, pr_number IS NOT NULL AS has_pr,
             pr_merged_at IS NOT NULL AS merged
         FROM tasks WHERE id = $1 FOR UPDATE",
    )
    .bind(task_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(TaskRowFacts {
        status,
        resume_at,
        auto_resumes,
        has_pr,
        merged,
    }) = row
    else {
        return Err(DbError::NotFound(format!("task `{task_id}` not found")));
    };
    let active_run: String = sqlx::query_scalar(
        "SELECT CASE
             WHEN EXISTS (SELECT 1 FROM runs WHERE task_id = $1 AND stop_requested_at IS NULL
                          AND state NOT IN ('finalizing', 'succeeded', 'failed', 'cancelled'))
                 THEN 'connected'
             WHEN EXISTS (SELECT 1 FROM runs WHERE task_id = $1 AND stop_requested_at IS NOT NULL
                          AND state NOT IN ('succeeded', 'failed', 'cancelled'))
                 THEN 'stop_requested'
             WHEN EXISTS (SELECT 1 FROM runs WHERE task_id = $1 AND state = 'finalizing')
                 THEN 'finalizing'
             ELSE 'none'
         END",
    )
    .bind(task_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let ever_ran: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM runs WHERE task_id = $1)")
            .bind(task_id.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    let dependencies: Vec<DependencyTimes> = sqlx::query_as(
        "SELECT d.kind, t.first_completed_at, t.first_done_at
             FROM task_dependencies d JOIN tasks t ON t.id = d.depends_on_task_id
             WHERE d.task_id = $1",
    )
    .bind(task_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    let mut unsatisfied_dependencies = 0_u32;
    for d in dependencies {
        if !dependency_satisfied(&d.kind, d.first_completed_at, d.first_done_at)? {
            unsatisfied_dependencies += 1;
        }
    }
    let undelivered_messages: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM task_messages
             WHERE task_id = $1 AND delivered_at IS NULL AND discarded_at IS NULL
               AND ($2::timestamptz IS NULL OR queued_at > $2))",
    )
    .bind(task_id.as_uuid())
    .bind(after)
    .fetch_one(&mut **tx)
    .await?;
    Ok(TaskSnapshot {
        status: status.parse()?,
        resume_at,
        auto_resumes,
        active_run: match active_run.as_str() {
            "connected" => ActiveRun::Connected,
            "stop_requested" => ActiveRun::StopRequested,
            "finalizing" => ActiveRun::Finalizing,
            "none" => ActiveRun::None,
            other => return Err(DbError::InvalidData(format!("active run `{other}`"))),
        },
        ever_ran,
        unsatisfied_dependencies,
        undelivered_messages,
        pr: match (has_pr, merged) {
            (false, _) => PrState::None,
            (true, false) => PrState::Unmerged,
            (true, true) => PrState::Merged,
        },
    })
}

async fn write_status(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    next: &Transition,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE tasks SET status = $2, resume_at = $3, auto_resumes = $4, updated_at = now()
         WHERE id = $1",
    )
    .bind(task_id.as_uuid())
    .bind(next.status.as_str())
    .bind(next.resume_at)
    .bind(next.auto_resumes)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// An idle run (`awaiting_input`) becomes `finalizing`, so the replica holding its stream
/// cancels it: no prompt reaches it from here on.
async fn finalize_idle_run(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<(), DbError> {
    let idle_run: Option<Uuid> = sqlx::query_scalar(
        "UPDATE runs SET state = 'finalizing' WHERE task_id = $1 AND state = 'awaiting_input'
         RETURNING id",
    )
    .bind(task_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(run_id) = idle_run {
        publish(
            &mut **tx,
            &Notification::RunChanged {
                run_id: RunId::from_uuid(run_id),
                task_id,
            },
        )
        .await?;
    }
    Ok(())
}

async fn discard_messages(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<u64, DbError> {
    Ok(sqlx::query(
        "UPDATE task_messages SET discarded_at = now()
         WHERE task_id = $1 AND delivered_at IS NULL AND discarded_at IS NULL",
    )
    .bind(task_id.as_uuid())
    .execute(&mut **tx)
    .await?
    .rows_affected())
}

/// Sets the status of a task with SQL and publishes `task_changed`, for tests that need a status
/// the events reach only through a long history.
#[cfg(test)]
pub(crate) async fn set_status_for_test(
    pool: &sqlx::PgPool,
    task_id: TaskId,
    status: df_core::TaskStatus,
) {
    sqlx::query("UPDATE tasks SET status = $2, updated_at = now() WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(status.as_str())
        .execute(pool)
        .await
        .unwrap();
    publish(pool, &Notification::TaskChanged { task_id })
        .await
        .unwrap();
}
