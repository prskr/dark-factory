//! Deleting task volumes under the task row lock (`iter2-task-volume-lifecycle`, design D1 and
//! D2). The scheduler locks the same row (`FOR UPDATE SKIP LOCKED`) to start a run, so no run
//! starts on a volume while it is being deleted.

use std::time::Duration;

use df_core::{Principal, ProviderError, SandboxProvider, TaskId, TaskKey, TaskVolumeSpec};
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
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

/// Deletes the volume of task `key` within `timeout` and sets `tasks.volume_deleted_at`. Call it
/// while holding the row lock of [`lock_task`].
pub async fn delete_volume(
    tx: &mut Transaction<'_, Postgres>,
    provider: &dyn SandboxProvider,
    task_id: TaskId,
    key: TaskKey,
    timeout: Duration,
) -> Result<TaskVolumeSpec, VolumeError> {
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
    sqlx::query!(
        "UPDATE tasks SET volume_deleted_at = now() WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    Ok(volume)
}

/// A board member resets the workspace of a task (design D2): deletes its volume under the row
/// lock and audits `task.reset_workspace`. A task with a non-terminal run is rejected with
/// [`DbError::Conflict`] and keeps its volume.
pub async fn reset_workspace(
    pool: &PgPool,
    provider: &dyn SandboxProvider,
    task_id: TaskId,
    actor: Principal,
    timeout: Duration,
) -> Result<(), VolumeError> {
    let mut tx = pool.begin().await?;
    let key = lock_task(&mut tx, task_id).await?;
    if crate::tasks::has_active_run(&mut tx, task_id).await? {
        return Err(DbError::Conflict("the task has an active run".into()).into());
    }
    let volume = delete_volume(&mut tx, provider, task_id, key, timeout).await?;
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
