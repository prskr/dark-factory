//! Task artifacts: storage and the "related task" rule (spec `task-artifacts`, design D1, D5).
//!
//! Callers validate name, type and size limits with `df_core::artifacts`; this module stores
//! what they detected and enforces the per-task count.

use chrono::{DateTime, Utc};
use df_core::artifacts::{ArtifactError, ArtifactKind, Sniffed};
use df_core::{RunId, TaskId};
use df_proto::transcript::v1::{self as pb, ArtifactWritten, FactoryActivity, factory_activity};
use sqlx::{FromRow, PgPool};
use thiserror::Error;

use crate::blobs::sha256_hex;
use crate::pool::DbError;
use crate::transcript::{append_control_event, factory_event, lock_run_and_task};

/// An artifact without its content.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ArtifactMeta {
    pub name: String,
    /// `text` or `image`.
    pub kind: String,
    pub mime_type: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub sha256: String,
    pub updated_at: DateTime<Utc>,
}

/// An artifact with its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub meta: ArtifactMeta,
    pub content: Vec<u8>,
}

/// A write to store.
#[derive(Debug, Clone, Copy)]
pub struct NewArtifact<'a> {
    pub task_id: TaskId,
    /// The run whose transcript gets the artifact-written event.
    pub run_id: RunId,
    /// The tool call id; the event is stored once per call.
    pub call_id: &'a str,
    pub name: &'a str,
    pub sniffed: &'a Sniffed,
    pub content: &'a [u8],
    pub max_per_task: u32,
}

#[derive(Debug, Error)]
pub enum WriteArtifactError {
    /// The task already holds `max_per_task` artifacts and the name is new.
    #[error(transparent)]
    Rejected(#[from] ArtifactError),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<sqlx::Error> for WriteArtifactError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error.into())
    }
}

/// Stores `new` on its task, replacing an artifact of the same name, and appends an
/// artifact-written event to the run's transcript in the same transaction (ADR-0057). The run and
/// task rows are locked, so concurrent writes of one task cannot exceed the count.
pub async fn write_artifact(
    pool: &PgPool,
    new: NewArtifact<'_>,
) -> Result<ArtifactMeta, WriteArtifactError> {
    let mut tx = pool.begin().await?;
    let lock = lock_run_and_task(&mut tx, new.run_id, new.task_id).await?;
    let (count, exists): (i64, bool) = sqlx::query_as(
        "SELECT count(*), COALESCE(bool_or(name = $2), false) FROM task_artifacts WHERE task_id = $1",
    )
    .bind(new.task_id.as_uuid())
    .bind(new.name)
    .fetch_one(&mut *tx)
    .await?;
    if !exists && count >= i64::from(new.max_per_task) {
        return Err(ArtifactError::TooMany(new.max_per_task).into());
    }
    let (width, height) = match new.sniffed.dimensions {
        Some((w, h)) => (i32::try_from(w).ok(), i32::try_from(h).ok()),
        None => (None, None),
    };
    let meta = sqlx::query_as::<_, ArtifactMeta>(
        "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, width, height, content, sha256)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (task_id, name) DO UPDATE SET
             kind = EXCLUDED.kind, mime_type = EXCLUDED.mime_type, size = EXCLUDED.size,
             width = EXCLUDED.width, height = EXCLUDED.height, content = EXCLUDED.content,
             sha256 = EXCLUDED.sha256, updated_at = now()
         RETURNING name, kind, mime_type, size, width, height, sha256, updated_at",
    )
    .bind(new.task_id.as_uuid())
    .bind(new.name)
    .bind(new.sniffed.kind.as_str())
    .bind(new.sniffed.mime_type)
    .bind(i64::try_from(new.content.len()).unwrap_or(i64::MAX))
    .bind(width)
    .bind(height)
    .bind(new.content)
    .bind(sha256_hex(new.content))
    .fetch_one(&mut *tx)
    .await?;
    let written = factory_event(FactoryActivity {
        kind: Some(factory_activity::Kind::ArtifactWritten(ArtifactWritten {
            name: meta.name.clone(),
            kind: match new.sniffed.kind {
                ArtifactKind::Text => pb::ArtifactKind::Text,
                ArtifactKind::Image => pb::ArtifactKind::Image,
            }
            .into(),
            mime_type: meta.mime_type.clone(),
            size: meta.size.unsigned_abs(),
        })),
    });
    append_control_event(
        &mut tx,
        &lock,
        &format!("artifact:{}", new.call_id),
        &written,
    )
    .await?;
    tx.commit().await?;
    Ok(meta)
}

/// The artifacts of a task, newest update first.
pub async fn list_artifacts(pool: &PgPool, task_id: TaskId) -> Result<Vec<ArtifactMeta>, DbError> {
    Ok(sqlx::query_as::<_, ArtifactMeta>(
        "SELECT name, kind, mime_type, size, width, height, sha256, updated_at FROM task_artifacts WHERE task_id = $1
         ORDER BY updated_at DESC, name",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?)
}

/// The first bytes of every text artifact of a task, by name, for the cards. They may end inside
/// a character.
pub async fn text_previews(
    pool: &PgPool,
    task_id: TaskId,
) -> Result<std::collections::HashMap<String, Vec<u8>>, DbError> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT name, substring(content FROM 1 FOR 640) FROM task_artifacts
         WHERE task_id = $1 AND kind = 'text'",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// One artifact with its content, or `None` if the task has none of that name.
pub async fn read_artifact_content(
    pool: &PgPool,
    task_id: TaskId,
    name: &str,
) -> Result<Option<Artifact>, DbError> {
    let row = sqlx::query_as::<_, ArtifactMeta>(
        "SELECT name, kind, mime_type, size, width, height, sha256, updated_at FROM task_artifacts WHERE task_id = $1 AND name = $2",
    )
    .bind(task_id.as_uuid())
    .bind(name)
    .fetch_optional(pool)
    .await?;
    let Some(meta) = row else { return Ok(None) };
    let content: Vec<u8> =
        sqlx::query_scalar("SELECT content FROM task_artifacts WHERE task_id = $1 AND name = $2")
            .bind(task_id.as_uuid())
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(Some(Artifact { meta, content }))
}

/// The task with `key` if `reader` may read its artifacts: the reader itself, an ancestor, a
/// descendant, a task the reader depends on. `None` for an unknown key and for an unrelated task
/// alike.
pub async fn readable_task(
    pool: &PgPool,
    reader: TaskId,
    key: &str,
) -> Result<Option<TaskId>, DbError> {
    let id: Option<uuid::Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE
             ancestors(id) AS (
                 SELECT parent_id FROM tasks WHERE id = $1 AND parent_id IS NOT NULL
                 UNION
                 SELECT t.parent_id FROM tasks t JOIN ancestors a ON t.id = a.id
                 WHERE t.parent_id IS NOT NULL
             ),
             descendants(id) AS (
                 SELECT id FROM tasks WHERE parent_id = $1
                 UNION
                 SELECT t.id FROM tasks t JOIN descendants d ON t.parent_id = d.id
             )
         SELECT t.id FROM tasks t
         WHERE t.key = $2 AND (
             t.id = $1
             OR t.id IN (SELECT id FROM ancestors)
             OR t.id IN (SELECT id FROM descendants)
             OR t.id IN (SELECT depends_on_task_id FROM task_dependencies WHERE task_id = $1)
         )",
    )
    .bind(reader.as_uuid())
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(id.map(TaskId::from_uuid))
}

#[cfg(test)]
mod tests;
