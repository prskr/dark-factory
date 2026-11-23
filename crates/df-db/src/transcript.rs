//! Transcript events: the one writer ([`TranscriptWriter`]) for runtime events keyed by
//! `(run, seq)` and control-plane events (forge and factory activity, ADR-0046) keyed by
//! `(run, source_key)`, and the reads of the turn state.
//!
//! Both kinds share the task-wide `id` order. Every write needs a [`RunTaskLock`] and ends with
//! one `transcript_event` notification carrying the newest id of the transaction.

use df_core::{RunId, TaskId};
use df_proto::transcript::v1::{Event, FactoryActivity, ForgeActivity, event, forge_activity};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::Value;
use sqlx::{PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::tasks::{mark_prompt_delivered, message_forge_event};

mod runtime;

pub use runtime::{PreparedEvent, allowed_from_runtime, externalize_block, prepare};

/// The `transcript_events.kind` values.
pub const KIND_TURN_STARTED: &str = "turn_started";
pub const KIND_USER_MESSAGE: &str = "user_message";
pub const KIND_AGENT_MESSAGE_CHUNK: &str = "agent_message_chunk";
pub const KIND_AGENT_THOUGHT_CHUNK: &str = "agent_thought_chunk";
pub const KIND_TOOL_CALL: &str = "tool_call";
pub const KIND_TOOL_CALL_UPDATE: &str = "tool_call_update";
pub const KIND_PLAN: &str = "plan";
pub const KIND_TURN_ENDED: &str = "turn_ended";
pub const KIND_NOTICE: &str = "notice";
pub const KIND_FORGE_ACTIVITY: &str = "forge_activity";
pub const KIND_FACTORY_ACTIVITY: &str = "factory_activity";

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

impl RunTaskLock {
    pub(crate) fn run_id(&self) -> RunId {
        self.run_id
    }
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
    sqlx::query!(
        "SELECT 1 AS one FROM runs WHERE id = $1 FOR UPDATE",
        run_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;
    sqlx::query!(
        "SELECT 1 AS one FROM tasks WHERE id = $1 FOR UPDATE",
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(RunTaskLock { run_id, task_id })
}

/// Writes the transcript events of one transaction for a locked run and publishes one
/// `transcript_event` notification with the newest stored id at [`Self::finish`] (delivered when
/// the transaction commits). Forgetting `finish` loses the notification, not the events.
#[must_use = "call `finish` to publish the notification"]
#[derive(Debug)]
pub struct TranscriptWriter<'l> {
    lock: &'l RunTaskLock,
    newest: Option<i64>,
}

impl<'l> TranscriptWriter<'l> {
    pub fn new(lock: &'l RunTaskLock) -> Self {
        Self { lock, newest: None }
    }

    /// Stores a runtime `event` once per `(run, seq)`. Returns the task-wide id of a newly stored
    /// event, `None` for a duplicate. Forge and factory activity are refused (the runtime must
    /// not write them). A newly stored `UserMessage` marks its queued message as delivered by
    /// the run in the same transaction; a duplicate leaves delivery unchanged.
    pub async fn append_runtime(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        event: &PreparedEvent,
    ) -> Result<Option<i64>, DbError> {
        if !runtime::kind_allowed_from_runtime(event.kind) {
            return Err(DbError::InvalidData(format!(
                "`{}` events are written by the control plane only",
                event.kind
            )));
        }
        let seq = i64::try_from(event.seq)
            .map_err(|_out_of_range| DbError::InvalidData("sequence number out of range".into()))?;
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO transcript_events
                (task_id, run_id, seq, schema_version, occurred_at, kind, payload)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (run_id, seq) DO NOTHING
            RETURNING id
            "#,
            self.lock.task_id.as_uuid(),
            self.lock.run_id.as_uuid(),
            seq,
            event.schema_version,
            event.occurred_at.to_sqlx(),
            event.kind,
            event.payload
        )
        .fetch_optional(&mut **tx)
        .await?;
        if let (Some(_), Some(prompt_id)) = (id, event.prompt_id) {
            mark_prompt_delivered(&mut **tx, self.lock.task_id, prompt_id, self.lock.run_id)
                .await?;
            // The event comes from the stored message, never from the runtime's event.
            // Only the run that delivered the prompt first writes the row.
            if let Some(received) =
                message_forge_event(&mut **tx, self.lock.task_id, prompt_id, self.lock.run_id)
                    .await?
            {
                let activity = ForgeActivity {
                    kind: Some(forge_activity::Kind::WebhookReceived(received)),
                };
                self.append_control(tx, &format!("webhook:{prompt_id}"), &forge_event(activity))
                    .await?;
            }
        }
        self.newest = self.newest.max(id);
        Ok(id)
    }

    /// Stores a forge or factory activity `event` once per `(run, source_key)`. Returns the
    /// task-wide id, `None` for a duplicate. Any other kind is refused.
    pub async fn append_control(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
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
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO transcript_events
                (task_id, run_id, source_key, schema_version, occurred_at, kind, payload)
            VALUES ($1, $2, $3, $4, now(), $5, $6)
            ON CONFLICT (run_id, source_key) DO NOTHING
            RETURNING id
            "#,
            self.lock.task_id.as_uuid(),
            self.lock.run_id.as_uuid(),
            source_key,
            SCHEMA_VERSION,
            kind,
            payload
        )
        .fetch_optional(&mut **tx)
        .await?;
        self.newest = self.newest.max(id);
        Ok(id)
    }

    /// Publishes `transcript_event` with the newest id this writer stored; nothing if it stored
    /// none.
    pub async fn finish(self, tx: &mut Transaction<'_, Postgres>) -> Result<(), DbError> {
        let Some(id) = self.newest else {
            return Ok(());
        };
        let task_id = self.lock.task_id;
        publish(&mut **tx, &Notification::TranscriptEvent { task_id, id }).await
    }
}

/// Kind (`turn_started` or `turn_ended`) and time of the run's last persisted turn event.
pub async fn last_turn_event(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
) -> Result<Option<(String, Timestamp)>, DbError> {
    let kinds = [KIND_TURN_STARTED.to_owned(), KIND_TURN_ENDED.to_owned()];
    Ok(sqlx::query!(
        "SELECT kind, occurred_at FROM transcript_events
         WHERE run_id = $1 AND kind = ANY($2)
         ORDER BY seq DESC LIMIT 1",
        run_id.as_uuid(),
        &kinds
    )
    .fetch_optional(executor)
    .await?
    .map(|row| (row.kind, row.occurred_at.into())))
}

/// Whether the run's last persisted turn event is a `TurnStarted`.
pub async fn turn_open(executor: impl PgExecutor<'_>, run_id: RunId) -> Result<bool, DbError> {
    Ok(last_turn_event(executor, run_id)
        .await?
        .is_some_and(|(kind, _)| kind == KIND_TURN_STARTED))
}

/// When the run's last persisted turn ended, if the last turn event is a `TurnEnded`.
pub async fn last_turn_end(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
) -> Result<Option<Timestamp>, DbError> {
    Ok(last_turn_event(executor, run_id)
        .await?
        .and_then(|(kind, at)| (kind == KIND_TURN_ENDED).then_some(at)))
}

/// The payload of the stored `tool_call` event of `tool_call_id` in the run.
pub async fn stored_tool_call(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
    tool_call_id: &str,
) -> Result<Option<Value>, DbError> {
    Ok(sqlx::query_scalar!(
        "SELECT payload FROM transcript_events
         WHERE run_id = $1 AND kind = $3 AND payload -> 'toolCall' ->> 'toolCallId' = $2
         ORDER BY seq LIMIT 1",
        run_id.as_uuid(),
        tool_call_id,
        KIND_TOOL_CALL
    )
    .fetch_optional(executor)
    .await?)
}

/// A transcript event of another run, flattened for the continuation summary. Which fields are
/// set depends on `kind`.
#[derive(Debug, Clone)]
pub struct ContinuationRow {
    pub run_id: Uuid,
    pub kind: String,
    pub turn_id: String,
    pub message_id: String,
    pub text: String,
    pub tool_call_id: String,
    pub title: Option<String>,
    pub status: Option<String>,
}

/// The user messages, agent message chunks and tool events of the task's runs other than
/// `excluding_run`, oldest first, as flat columns (no raw tool input or output). The text of a
/// user message holds its visible blocks only (`hidden` blocks are left out).
pub async fn continuation_events(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
    excluding_run: RunId,
) -> Result<Vec<ContinuationRow>, DbError> {
    let kinds = [
        KIND_USER_MESSAGE.to_owned(),
        KIND_AGENT_MESSAGE_CHUNK.to_owned(),
        KIND_TOOL_CALL.to_owned(),
        KIND_TOOL_CALL_UPDATE.to_owned(),
    ];
    Ok(sqlx::query_as!(
        ContinuationRow,
        r#"
        SELECT run_id, kind,
          COALESCE(payload ->> 'turnId', '') AS "turn_id!",
          COALESCE(payload #>> '{agentMessageChunk,messageId}', '') AS "message_id!",
          COALESCE(CASE kind
            WHEN $4 THEN
              (SELECT string_agg(b.block #>> '{text,text}', E'\n' ORDER BY b.ord)
                 FROM jsonb_array_elements(payload #> '{userMessage,content}')
                      WITH ORDINALITY AS b(block, ord)
                WHERE COALESCE((b.block ->> 'hidden')::boolean, false) = false)
            ELSE payload #>> '{agentMessageChunk,content,text,text}'
          END, '') AS "text!",
          COALESCE(payload #>> '{toolCall,toolCallId}', payload #>> '{toolCallUpdate,toolCallId}', '')
            AS "tool_call_id!",
          COALESCE(payload #>> '{toolCall,title}', payload #>> '{toolCallUpdate,title}') AS title,
          COALESCE(payload #>> '{toolCall,status}', payload #>> '{toolCallUpdate,status}') AS status
        FROM transcript_events
        WHERE task_id = $1 AND run_id <> $2 AND kind = ANY($3)
        ORDER BY id
        "#,
        task_id.as_uuid(),
        excluding_run.as_uuid(),
        &kinds,
        KIND_USER_MESSAGE
    )
    .fetch_all(executor)
    .await?)
}

#[cfg(test)]
mod tests;
