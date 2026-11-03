//! Deleting task volumes under the task row lock (`iter2-task-volume-lifecycle`, design D1 and
//! D2). The scheduler locks the same row (`FOR UPDATE SKIP LOCKED`) to start a run, so no run
//! starts on a volume while it is being deleted.

use std::time::Duration;

use df_core::{Principal, ProviderError, RunId, SandboxProvider, TaskId, TaskKey, TaskVolumeSpec};
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::lifecycle_events::{self, Event, EventSinks, VolumeDestroyedReason};
use crate::pool::DbError;

/// Upper bound of one volume deletion; the task row stays locked meanwhile.
pub const DELETE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why deleting a task volume failed; the transaction rolls back.
#[derive(Debug, thiserror::Error)]
pub enum VolumeError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

impl From<sqlx::Error> for VolumeError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error.into())
    }
}

/// Locks the task row for the rest of the transaction and returns the task key.
pub async fn lock_task(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<TaskKey, DbError> {
    let key = sqlx::query_scalar!(
        "SELECT key FROM tasks WHERE id = $1 FOR UPDATE",
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("task `{task_id}` not found")))?;
    key.parse()
        .map_err(|e| DbError::InvalidData(format!("task key `{key}`: {e}")))
}

/// What a volume deletion needs besides the task: why, where events go and how long it may take.
#[derive(Debug, Clone, Copy)]
pub struct Deletion<'a> {
    pub reason: VolumeDestroyedReason,
    pub events: &'a EventSinks,
    pub timeout: Duration,
}

/// Records that the volume of `task_id` exists after `run_id`'s scheduling ensured it: clears
/// `tasks.volume_deleted_at` and, the first time since the volume last did not exist, sets
/// `tasks.volume_provisioned_at` and emits `volume.provisioned` with that run.
pub async fn volume_ensured(
    pool: &PgPool,
    events: &EventSinks,
    task_id: TaskId,
    run_id: RunId,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "UPDATE tasks SET volume_deleted_at = NULL WHERE id = $1 AND volume_deleted_at IS NOT NULL",
        task_id.as_uuid()
    )
    .execute(&mut *tx)
    .await?;
    let first = sqlx::query!(
        "UPDATE tasks SET volume_provisioned_at = now() WHERE id = $1 AND volume_provisioned_at IS NULL",
        task_id.as_uuid()
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if first {
        lifecycle_events::enqueue(&mut tx, events, &Event::VolumeProvisioned { run_id }).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Deletes the volume of task `key` within `deletion.timeout`, sets `tasks.volume_deleted_at`
/// and, if the volume was provisioned, emits `volume.destroyed`. Call it while holding the row
/// lock of [`lock_task`].
pub async fn delete_volume(
    tx: &mut Transaction<'_, Postgres>,
    provider: &dyn SandboxProvider,
    task_id: TaskId,
    key: TaskKey,
    deletion: Deletion<'_>,
) -> Result<TaskVolumeSpec, VolumeError> {
    let timeout = deletion.timeout;
    let volume = TaskVolumeSpec::for_task(key);
    tokio::time::timeout(timeout, provider.delete_task_volume(&volume))
        .await
        .map_err(|_elapsed| {
            ProviderError::Backend(format!(
                "deleting volume `{}` timed out after {} s",
                volume.volume_name,
                timeout.as_secs()
            ))
        })??;
    let provisioned = sqlx::query_scalar!(
        "SELECT volume_provisioned_at IS NOT NULL AS \"provisioned!\" FROM tasks WHERE id = $1",
        task_id.as_uuid()
    )
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query!(
        "UPDATE tasks SET volume_deleted_at = now(), volume_provisioned_at = NULL WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    if provisioned {
        let event = Event::VolumeDestroyed {
            task_id,
            reason: deletion.reason,
        };
        lifecycle_events::enqueue(tx, deletion.events, &event).await?;
    }
    Ok(volume)
}

/// A board member resets the workspace of a task (design D2): deletes its volume under the row
/// lock and audits `task.reset_workspace`. A task with a non-terminal run is rejected with
/// [`DbError::Conflict`] and keeps its volume.
pub async fn reset_workspace(
    pool: &PgPool,
    provider: &dyn SandboxProvider,
    events: &EventSinks,
    task_id: TaskId,
    actor: Principal,
    timeout: Duration,
) -> Result<(), VolumeError> {
    let mut tx = pool.begin().await?;
    let key = lock_task(&mut tx, task_id).await?;
    if crate::tasks::has_active_run(&mut tx, task_id).await? {
        return Err(DbError::Conflict("the task has an active run".into()).into());
    }
    let deletion = Deletion {
        reason: VolumeDestroyedReason::Reset,
        events,
        timeout,
    };
    let volume = delete_volume(&mut tx, provider, task_id, key, deletion).await?;
    let entry = NewAuditEntry::new(
        actor,
        Vec::new(),
        "task.reset_workspace",
        "task",
        task_id.to_string(),
    )
    .with_details(&serde_json::json!({ "volume": volume.volume_name }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(())
}
