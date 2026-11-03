//! Lifecycle events (`lifecycle-events` design D1): the recorded envelope and one outbox job per
//! receiver that subscribed to the event.

use chrono::{DateTime, SecondsFormat, Utc};
use df_core::lifecycle::matches_filter;
use df_core::{EventType, InstanceSubscription, LifecycleEventsConfig, RunId, RunOutcome, TaskId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::outbox;
use crate::pool::DbError;

/// Outbox job kind that delivers one event to one receiver.
pub const DELIVER_KIND: &str = "lifecycle_event.deliver";

/// Where events go: off without a signing key, else the instance subscriptions plus the project
/// subscriptions in the database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventSinks {
    enabled: bool,
    instance: Vec<InstanceSubscription>,
}

impl EventSinks {
    pub fn new(config: &LifecycleEventsConfig) -> Self {
        Self {
            enabled: config.enabled(),
            instance: config.subscriptions.clone(),
        }
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Why a task's volume was deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeDestroyedReason {
    Retention,
    Reset,
}

/// A state change that emits a lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    VolumeProvisioned {
        run_id: RunId,
    },
    VolumeDestroyed {
        task_id: TaskId,
        reason: VolumeDestroyedReason,
    },
    SandboxProvisioning {
        run_id: RunId,
    },
    SandboxProvisioned {
        run_id: RunId,
    },
    SandboxProvisioningFailed {
        run_id: RunId,
        error: String,
    },
    SandboxDestroyed {
        run_id: RunId,
        outcome: RunOutcome,
        started_at: Option<DateTime<Utc>>,
        ended_at: DateTime<Utc>,
    },
}

impl Event {
    pub const fn event_type(&self) -> EventType {
        match self {
            Self::VolumeProvisioned { .. } => EventType::VolumeProvisioned,
            Self::VolumeDestroyed { .. } => EventType::VolumeDestroyed,
            Self::SandboxProvisioning { .. } => EventType::SandboxProvisioning,
            Self::SandboxProvisioned { .. } => EventType::SandboxProvisioned,
            Self::SandboxProvisioningFailed { .. } => EventType::SandboxProvisioningFailed,
            Self::SandboxDestroyed { .. } => EventType::SandboxDestroyed,
        }
    }

    const fn run_id(&self) -> Option<RunId> {
        match self {
            Self::VolumeDestroyed { .. } => None,
            Self::VolumeProvisioned { run_id }
            | Self::SandboxProvisioning { run_id }
            | Self::SandboxProvisioned { run_id }
            | Self::SandboxProvisioningFailed { run_id, .. }
            | Self::SandboxDestroyed { run_id, .. } => Some(*run_id),
        }
    }

    const fn task_id(&self) -> Option<TaskId> {
        match self {
            Self::VolumeDestroyed { task_id, .. } => Some(*task_id),
            _ => None,
        }
    }

    /// Event-specific fields of `data`.
    fn fields(&self) -> Value {
        match self {
            Self::VolumeDestroyed { reason, .. } => json!({ "reason": reason }),
            Self::SandboxProvisioningFailed { error, .. } => json!({ "error": error }),
            Self::SandboxDestroyed {
                outcome,
                started_at,
                ended_at,
                ..
            } => json!({
                "outcome": outcome,
                "started_at": started_at.as_ref().map(rfc3339),
                "ended_at": rfc3339(ended_at),
            }),
            Self::VolumeProvisioned { .. }
            | Self::SandboxProvisioning { .. }
            | Self::SandboxProvisioned { .. } => json!({}),
        }
    }
}

/// Payload of a [`DELIVER_KIND`] job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliverJob {
    pub event_id: Uuid,
    pub target: Target,
}

/// Receiver of one delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Subscription { subscription_id: Uuid },
    Instance { instance_url: String },
}

/// What one delivery attempt sends, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    /// The envelope, sent as the body.
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
    pub url: String,
}

/// Project, task, persona and run an event is about, as of the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub project_id: Uuid,
    pub project_key: String,
    pub repo_url: String,
    pub task_id: Uuid,
    pub task_key: String,
    pub persona_id: Uuid,
    pub persona_name: String,
    /// `(id, number)` of the run; `None` for events without a run.
    pub run: Option<(Uuid, i64)>,
}

/// Records `event` and enqueues one delivery per matching receiver, inside the caller's
/// transaction. Writes nothing when the feature is off or nobody subscribed to the event.
/// Returns the event ID if it was recorded.
pub async fn enqueue(
    conn: &mut PgConnection,
    sinks: &EventSinks,
    event: &Event,
) -> Result<Option<Uuid>, DbError> {
    if !sinks.enabled {
        return Ok(None);
    }
    let event_type = event.event_type();
    let (subject, occurred_at) = load_subject(conn, event).await?;

    let subscriptions = sqlx::query_scalar!(
        r#"
        SELECT id FROM event_subscriptions
        WHERE project_id = $1 AND (cardinality(events) = 0 OR $2 = ANY(events))
        ORDER BY created_at, id
        "#,
        subject.project_id,
        event_type.as_str(),
    )
    .fetch_all(&mut *conn)
    .await?;
    let targets: Vec<Target> = subscriptions
        .into_iter()
        .map(|subscription_id| Target::Subscription { subscription_id })
        .chain(
            sinks
                .instance
                .iter()
                .filter(|sub| matches_filter(&sub.events, event_type))
                .map(|sub| Target::Instance {
                    instance_url: sub.url.clone(),
                }),
        )
        .collect();
    if targets.is_empty() {
        return Ok(None);
    }

    let event_id = Uuid::now_v7();
    let payload = envelope(event_id, occurred_at, &subject, event);
    sqlx::query!(
        r#"
        INSERT INTO lifecycle_events (id, type, occurred_at, project_id, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        event_id,
        event_type.as_str(),
        occurred_at,
        subject.project_id,
        payload,
    )
    .execute(&mut *conn)
    .await?;
    for target in targets {
        let job = serde_json::to_value(DeliverJob { event_id, target })?;
        outbox::enqueue(conn, DELIVER_KIND, &job).await?;
    }
    Ok(Some(event_id))
}

/// The event as sent: `evt_<uuidv7>` ID, type, time and `data` (spec "Event envelope").
pub fn envelope(id: Uuid, occurred_at: DateTime<Utc>, subject: &Subject, event: &Event) -> Value {
    let mut data = Map::new();
    data.insert(
        "project".into(),
        json!({
            "id": subject.project_id,
            "key": subject.project_key,
            "repo_url": subject.repo_url,
        }),
    );
    data.insert(
        "task".into(),
        json!({ "id": subject.task_id, "key": subject.task_key }),
    );
    data.insert(
        "persona".into(),
        json!({ "id": subject.persona_id, "name": subject.persona_name }),
    );
    if let Some((run_id, number)) = subject.run {
        data.insert("run".into(), json!({ "id": run_id, "number": number }));
    }
    if let Value::Object(fields) = event.fields() {
        data.extend(fields);
    }
    json!({
        "id": format!("evt_{id}"),
        "type": event.event_type(),
        "timestamp": rfc3339(&occurred_at),
        "data": data,
    })
}

/// Loads the event and the receiver URL of `job`. `None` when the event or the project
/// subscription no longer exists.
pub async fn load_delivery(pool: &PgPool, job: &DeliverJob) -> Result<Option<Delivery>, DbError> {
    let Some(event) = sqlx::query!(
        "SELECT payload, occurred_at FROM lifecycle_events WHERE id = $1",
        job.event_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let url = match &job.target {
        Target::Instance { instance_url } => instance_url.clone(),
        Target::Subscription { subscription_id } => {
            let url = sqlx::query_scalar!(
                "SELECT url FROM event_subscriptions WHERE id = $1",
                subscription_id,
            )
            .fetch_optional(pool)
            .await?;
            let Some(url) = url else {
                return Ok(None);
            };
            url
        }
    };
    Ok(Some(Delivery {
        payload: event.payload,
        occurred_at: event.occurred_at,
        url,
    }))
}

/// Records the last delivery attempt of a project subscription: its HTTP status, or the error
/// when no response came back.
pub async fn record_attempt(
    pool: &PgPool,
    subscription_id: Uuid,
    status: Option<u16>,
    error: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query!(
        r#"
        UPDATE event_subscriptions
        SET last_attempt_at = now(), last_status = $2, last_error = $3
        WHERE id = $1
        "#,
        subscription_id,
        status.and_then(|status| i16::try_from(status).ok()),
        error,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes the events that no outbox job refers to any more (cleanup hook of the delivery
/// handler). Returns how many were deleted.
pub async fn delete_unreferenced(pool: &PgPool) -> Result<u64, DbError> {
    let deleted = sqlx::query!(
        r#"
        DELETE FROM lifecycle_events e
        WHERE NOT EXISTS (
            SELECT 1 FROM outbox o
            WHERE o.kind = $1 AND o.payload->>'event_id' = e.id::text
        )
        "#,
        DELIVER_KIND,
    )
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected())
}

/// Loads the subject of `event` and the transaction time as the event time.
async fn load_subject(
    conn: &mut PgConnection,
    event: &Event,
) -> Result<(Subject, DateTime<Utc>), DbError> {
    let run_id = event.run_id().map(|id| id.as_uuid());
    let row = sqlx::query!(
        r#"
        SELECT p.id AS project_id, p.key AS project_key, p.repo_url,
               t.id AS task_id, t.key AS task_key,
               pe.id AS persona_id, pe.name AS persona_name,
               r.id AS "run_id?", r.number AS "run_number?",
               now() AS "occurred_at!"
        FROM tasks t
        JOIN projects p ON p.id = t.project_id
        JOIN personas pe ON pe.id = t.persona_id
        LEFT JOIN runs r ON r.id = $2 AND r.task_id = t.id
        WHERE t.id = COALESCE($1, (SELECT task_id FROM runs WHERE id = $2))
        "#,
        event.task_id().map(|id| id.as_uuid()),
        run_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| DbError::InvalidData(format!("no task for lifecycle event {event:?}")))?;
    let run = match (row.run_id, row.run_number) {
        (Some(id), Some(number)) => Some((id, number)),
        _ => None,
    };
    if run_id.is_some() && run.is_none() {
        return Err(DbError::InvalidData(format!(
            "no run for lifecycle event {event:?}"
        )));
    }
    let subject = Subject {
        project_id: row.project_id,
        project_key: row.project_key,
        repo_url: row.repo_url,
        task_id: row.task_id,
        task_key: row.task_key,
        persona_id: row.persona_id,
        persona_name: row.persona_name,
        run,
    };
    Ok((subject, row.occurred_at))
}

fn rfc3339(time: &DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests;
