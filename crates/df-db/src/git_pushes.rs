//! Records of pushes through the gateway's Git proxy (iter2-git-proxy design D6).
//!
//! The verdict only moves forward: `forwarding → accepted | upstream-failed`, and `unknown` (set
//! by [`mark_stale_forwarding`] when the final call never came) is a placeholder that a late
//! `accepted` or `upstream-failed` still replaces. Repeating the stored verdict and moving it
//! backwards are no-ops, so retries are idempotent.
//!
//! One audit entry `git.push` is written per final verdict reported by the gateway (`accepted`,
//! `rejected`, `upstream-failed`), in the transaction that stores it. `forwarding` and `unknown`
//! only live in `git_pushes`.

use std::time::Duration;

use df_core::{PersonaId, Principal, RunId, TaskId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::pool::DbError;

/// Verdict of a push as stored in `git_pushes.verdict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PushVerdict {
    Forwarding,
    Accepted,
    Rejected,
    UpstreamFailed,
    Unknown,
}

impl PushVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forwarding => "forwarding",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::UpstreamFailed => "upstream-failed",
            Self::Unknown => "unknown",
        }
    }

    fn parse(value: &str) -> Result<Self, DbError> {
        Ok(match value {
            "forwarding" => Self::Forwarding,
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            "upstream-failed" => Self::UpstreamFailed,
            "unknown" => Self::Unknown,
            other => {
                return Err(DbError::InvalidData(format!(
                    "unknown push verdict `{other}`"
                )));
            }
        })
    }

    /// A verdict the gateway reports at the end of a push; it gets an audit entry.
    fn is_final(self) -> bool {
        matches!(self, Self::Accepted | Self::Rejected | Self::UpstreamFailed)
    }

    /// Whether a stored `self` may be replaced by `next`.
    fn advances_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Forwarding | Self::Unknown,
                Self::Accepted | Self::UpstreamFailed
            )
        )
    }
}

/// One command of a push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefUpdate {
    pub name: String,
    pub old_sha: String,
    pub new_sha: String,
    pub verdict: PushVerdict,
    pub reason: String,
}

/// A `RecordPush` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRecord {
    pub push_id: Uuid,
    pub run_id: RunId,
    pub gateway_pod: String,
    pub refs: Vec<RefUpdate>,
    pub verdict: PushVerdict,
    pub bytes: u64,
    pub duration: Duration,
}

/// What [`record_push`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Inserted,
    Advanced,
    /// Same verdict as stored, or a step backwards.
    Unchanged,
}

/// Details of the `git.push` audit entry.
#[derive(Serialize)]
struct PushAuditDetails<'a> {
    push_id: Uuid,
    run_id: RunId,
    verdict: PushVerdict,
    gateway_pod: &'a str,
    refs: &'a [RefUpdate],
}

/// Stores or advances a push record, and writes its audit entry, in one transaction.
pub async fn record_push(pool: &PgPool, record: &PushRecord) -> Result<RecordOutcome, DbError> {
    let mut tx = pool.begin().await?;
    let actor = run_agent(&mut tx, record.run_id).await?;
    let refs = serde_json::to_value(&record.refs)?;
    let bytes = i64::try_from(record.bytes).unwrap_or(i64::MAX);
    let duration_ms = i64::try_from(record.duration.as_millis()).unwrap_or(i64::MAX);

    let inserted = sqlx::query_scalar!(
        r#"
        INSERT INTO git_pushes (push_id, run_id, gateway_pod, refs, verdict, bytes, duration_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (push_id) DO NOTHING
        RETURNING push_id
        "#,
        record.push_id,
        record.run_id.as_uuid(),
        record.gateway_pod,
        refs,
        record.verdict.as_str(),
        bytes,
        duration_ms,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let outcome = if inserted.is_some() {
        RecordOutcome::Inserted
    } else {
        let stored = sqlx::query!(
            "SELECT run_id, verdict FROM git_pushes WHERE push_id = $1 FOR UPDATE",
            record.push_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if stored.run_id != record.run_id.as_uuid() {
            return Err(DbError::InvalidData(format!(
                "push {} belongs to another run",
                record.push_id
            )));
        }
        if !PushVerdict::parse(&stored.verdict)?.advances_to(record.verdict) {
            return Ok(RecordOutcome::Unchanged);
        }
        sqlx::query!(
            r#"
            UPDATE git_pushes
            SET refs = $2, verdict = $3, bytes = $4, duration_ms = $5, gateway_pod = $6,
                updated_at = now()
            WHERE push_id = $1
            "#,
            record.push_id,
            refs,
            record.verdict.as_str(),
            bytes,
            duration_ms,
            record.gateway_pod,
        )
        .execute(&mut *tx)
        .await?;
        RecordOutcome::Advanced
    };

    if record.verdict.is_final() {
        let entry = NewAuditEntry::new(
            actor,
            Vec::new(),
            "git.push",
            "run",
            record.run_id.to_string(),
        )
        .with_details(&PushAuditDetails {
            push_id: record.push_id,
            run_id: record.run_id,
            verdict: record.verdict,
            gateway_pod: &record.gateway_pod,
            refs: &record.refs,
        })?;
        write_audit_entry(&mut *tx, &entry).await?;
    }
    tx.commit().await?;
    Ok(outcome)
}

/// Marks `forwarding` records older than `older_than` as `unknown`; returns how many.
pub async fn mark_stale_forwarding(pool: &PgPool, older_than: Duration) -> Result<u64, DbError> {
    let seconds = older_than.as_secs_f64();
    let done = sqlx::query!(
        r#"
        UPDATE git_pushes
        SET verdict = 'unknown', updated_at = now()
        WHERE verdict = 'forwarding' AND created_at < now() - make_interval(secs => $1)
        "#,
        seconds
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// The agent principal of a run: its task's persona.
async fn run_agent(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: RunId,
) -> Result<Principal, DbError> {
    let row = sqlx::query!(
        r#"
        SELECT r.task_id, t.persona_id, p.name
        FROM runs r
        JOIN tasks t ON t.id = r.task_id
        JOIN personas p ON p.id = t.persona_id
        WHERE r.id = $1
        "#,
        run_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("run {run_id}")))?;
    Ok(Principal::agent(
        PersonaId::from_uuid(row.persona_id),
        row.name,
        Some(run_id),
        TaskId::from_uuid(row.task_id),
    ))
}

#[cfg(test)]
mod tests;
