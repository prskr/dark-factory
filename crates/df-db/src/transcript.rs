//! Transcript events written by the control plane (forge activity, ADR-0046).
//!
//! Runtime events are stored by `df-agent-channel`, keyed by `(run, seq)`. Events of the control
//! plane have no `seq`; they are keyed by `(run, source_key)` and share the task-wide `id` order.

use df_core::{RunId, TaskId};
use df_proto::transcript::v1::{Event, FactoryActivity, ForgeActivity, event};
use sqlx::{Postgres, Transaction};

use crate::notify::{Notification, publish};
use crate::pool::DbError;

/// The `transcript_events.kind` of forge activity.
const KIND_FORGE_ACTIVITY: &str = "forge_activity";

/// The `transcript_events.kind` of factory activity.
const KIND_FACTORY_ACTIVITY: &str = "factory_activity";

/// Schema version of the events [`forge_event`] and [`factory_event`] build and of the stored row.
const SCHEMA_VERSION: i32 = 1;

/// Wraps `activity` into a transcript event (schema version 1, no turn).
pub fn forge_event(activity: ForgeActivity) -> Event {
    control_event(event::Kind::ForgeActivity(activity))
}

/// Wraps `activity` into a transcript event (schema version 1, no turn).
pub fn factory_event(activity: FactoryActivity) -> Event {
    control_event(event::Kind::FactoryActivity(activity))
}

fn control_event(kind: event::Kind) -> Event {
    Event {
        schema_version: SCHEMA_VERSION.unsigned_abs(),
        occurred_at: None,
        turn_id: String::new(),
        kind: Some(kind),
    }
}

/// Proof that a transaction holds the run row and then the task row (see [`lock_run_and_task`]).
#[derive(Debug)]
pub struct RunTaskLock {
    run_id: RunId,
    task_id: TaskId,
}

/// Locks the run row, then the task row, until `tx` ends.
///
/// Lock order: run row first, then task row. Event ids are the cursor of the live transcript
/// (ADR-0042) and must become visible in id order, so a transaction that writes transcript events
/// holds the task row; one that also locks a run row takes that first, otherwise two of them can
/// deadlock. A caller that updates the task row itself locks before that update.
pub async fn lock_run_and_task(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    task_id: TaskId,
) -> Result<RunTaskLock, DbError> {
    sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(RunTaskLock { run_id, task_id })
}

/// Stores a forge or factory activity `event` once per `(run, source_key)` of the locked run and
/// publishes `transcript_event` with its id (delivered when `tx` commits). Returns the task-wide
/// id, `None` for a duplicate.
pub async fn append_control_event(
    tx: &mut Transaction<'_, Postgres>,
    lock: &RunTaskLock,
    source_key: &str,
    event: &Event,
) -> Result<Option<i64>, DbError> {
    let kind = match event.kind {
        Some(event::Kind::ForgeActivity(_)) => KIND_FORGE_ACTIVITY,
        Some(event::Kind::FactoryActivity(_)) => KIND_FACTORY_ACTIVITY,
        _ => {
            return Err(DbError::InvalidData(
                "only forge and factory activity are control-plane transcript events".into(),
            ));
        }
    };
    let payload = serde_json::to_value(event)?;
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO transcript_events
             (task_id, run_id, source_key, schema_version, occurred_at, kind, payload)
         VALUES ($1, $2, $3, $4, now(), $5, $6)
         ON CONFLICT (run_id, source_key) DO NOTHING
         RETURNING id",
    )
    .bind(lock.task_id.as_uuid())
    .bind(lock.run_id.as_uuid())
    .bind(source_key)
    .bind(SCHEMA_VERSION)
    .bind(kind)
    .bind(payload)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = id {
        let task_id = lock.task_id;
        publish(&mut **tx, &Notification::TranscriptEvent { task_id, id }).await?;
    }
    Ok(id)
}

#[cfg(test)]
mod tests;
