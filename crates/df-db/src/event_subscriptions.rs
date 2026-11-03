//! Project subscriptions of lifecycle events (`event-subscriptions` spec). Admins edit them in the
//! project settings; every change writes an `event_subscription.*` audit entry with the URL and
//! the event types in the same transaction.

use chrono::{DateTime, Utc};
use df_core::EventType;
use serde::Serialize;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::PgPool;
use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::Actor;
use crate::pool::DbError;

/// Fields of a project subscription. Empty `events` means every event type, including types
/// added later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubscriptionInput {
    pub url: String,
    pub events: Vec<EventType>,
}

/// A stored project subscription with its last delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub id: Uuid,
    pub url: String,
    /// Event types; empty means all.
    pub events: Vec<EventType>,
    pub created_at: DateTime<Utc>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// HTTP status of the last attempt that got a response.
    pub last_status: Option<u16>,
    /// Error of the last attempt that got no response (timeout, refused address).
    pub last_error: Option<String>,
}

/// The subscriptions of project `project_id`, oldest first.
pub async fn list(pool: &PgPool, project_id: Uuid) -> Result<Vec<Subscription>, DbError> {
    sqlx::query_as!(
        Row,
        r#"
        SELECT id, url, events, created_at, last_attempt_at, last_status, last_error
        FROM event_subscriptions WHERE project_id = $1 ORDER BY created_at, id
        "#,
        project_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Subscription::try_from)
    .collect()
}

/// Subscription `id` of project `project_id`; `None` if it does not exist or belongs to another
/// project.
pub async fn get(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
) -> Result<Option<Subscription>, DbError> {
    sqlx::query_as!(
        Row,
        r#"
        SELECT id, url, events, created_at, last_attempt_at, last_status, last_error
        FROM event_subscriptions WHERE project_id = $1 AND id = $2
        "#,
        project_id,
        id,
    )
    .fetch_optional(pool)
    .await?
    .map(Subscription::try_from)
    .transpose()
}

/// Creates (`id` is `None`) or updates a subscription of project `project_id` and audits the
/// change. An update of a subscription outside the project is [`DbError::NotFound`].
pub async fn save(
    pool: &PgPool,
    actor: Actor<'_>,
    project_id: Uuid,
    id: Option<Uuid>,
    input: &SubscriptionInput,
) -> Result<Uuid, DbError> {
    let mut tx = pool.begin().await?;
    let project_key = project_key(&mut tx, project_id).await?;
    let events: Vec<&str> = input.events.iter().map(EventType::as_str).collect();
    let mut before = None;
    let id = if let Some(id) = id {
        before = Some(lock(&mut tx, project_id, id).await?);
        sqlx::query!(
            "UPDATE event_subscriptions SET url = $2, events = $3 WHERE id = $1",
            id,
            input.url,
            &events as &[&str],
        )
        .execute(&mut *tx)
        .await?;
        id
    } else {
        let id = Uuid::now_v7();
        sqlx::query!(
            "INSERT INTO event_subscriptions (id, project_id, url, events) VALUES ($1, $2, $3, $4)",
            id,
            project_id,
            input.url,
            &events as &[&str],
        )
        .execute(&mut *tx)
        .await?;
        id
    };
    let action = if before.is_some() { "update" } else { "create" };
    let after = Stored {
        url: &input.url,
        events: events.iter().map(ToString::to_string).collect(),
    };
    audit(
        &mut tx,
        actor,
        action,
        id,
        &project_key,
        before.as_ref(),
        Some(&after),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Deletes subscription `id` of project `project_id` and audits it. Pending deliveries stay in
/// the outbox; the delivery handler gives them up (design D9).
pub async fn delete(
    pool: &PgPool,
    actor: Actor<'_>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    let project_key = project_key(&mut tx, project_id).await?;
    let before = lock(&mut tx, project_id, id).await?;
    sqlx::query!("DELETE FROM event_subscriptions WHERE id = $1", id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        actor,
        "delete",
        id,
        &project_key,
        Some(&before),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A row of `event_subscriptions` as stored.
struct Row {
    id: Uuid,
    url: String,
    events: Vec<String>,
    created_at: DateTime<Utc>,
    last_attempt_at: Option<DateTime<Utc>>,
    last_status: Option<i16>,
    last_error: Option<String>,
}

impl TryFrom<Row> for Subscription {
    type Error = DbError;

    fn try_from(row: Row) -> Result<Self, DbError> {
        let last_status = row
            .last_status
            .map(|status| {
                u16::try_from(status)
                    .map_err(|e| DbError::InvalidData(format!("HTTP status {status}: {e}")))
            })
            .transpose()?;
        Ok(Self {
            id: row.id,
            url: row.url,
            events: row
                .events
                .iter()
                .map(|event| event.parse())
                .collect::<Result<_, _>>()?,
            created_at: row.created_at,
            last_attempt_at: row.last_attempt_at,
            last_status,
            last_error: row.last_error,
        })
    }
}

/// URL and event types as recorded in the audit details.
#[derive(Debug, Serialize)]
struct Stored<'a> {
    url: &'a str,
    events: Vec<String>,
}

/// The stored values of a subscription, locked for the change.
#[derive(Debug, Serialize)]
struct Before {
    url: String,
    events: Vec<String>,
}

async fn project_key(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<String, DbError> {
    sqlx::query_scalar!("SELECT key FROM projects WHERE id = $1", project_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("project `{project_id}`")))
}

async fn lock(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    id: Uuid,
) -> Result<Before, DbError> {
    sqlx::query_as!(
        Before,
        "SELECT url, events FROM event_subscriptions WHERE id = $1 AND project_id = $2 FOR UPDATE",
        id,
        project_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("event subscription `{id}`")))
}

/// Writes `event_subscription.<action>` with `{id, project, before, after}` details.
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor<'_>,
    action: &str,
    id: Uuid,
    project_key: &str,
    before: Option<&Before>,
    after: Option<&Stored<'_>>,
) -> Result<(), DbError> {
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        format!("event_subscription.{action}"),
        "event_subscription",
        id.to_string(),
    )
    .with_details(&serde_json::json!({
        "id": id,
        "project": project_key,
        "before": before,
        "after": after,
    }))?;
    write_audit_entry(&mut **tx, &entry).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
