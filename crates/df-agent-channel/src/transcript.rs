//! Transcript persistence: canonical JSON payloads, blob extraction, prompt delivery
//! confirmation (`specs/transcript-storage/spec.md`).

use chrono::{DateTime, Utc};
use df_core::{PromptId, RunId, TaskId};
use df_db::PgBlobStore;
use df_proto::pbjson_types::Timestamp;
use df_proto::transcript::v1::{
    BlobData, ContentBlock, Event, blob_data, content_block, embedded_resource, event,
    tool_call_content,
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

use crate::error::ChannelError;

/// An event ready to be stored: binary data moved to the blob store, payload serialized.
#[derive(Debug, Clone)]
pub struct PreparedEvent {
    pub seq: u64,
    pub kind: &'static str,
    pub schema_version: i32,
    pub occurred_at: DateTime<Utc>,
    pub payload: Value,
    /// Set for `UserMessage` events: the queued message this event confirms.
    pub prompt_id: Option<PromptId>,
}

/// Moves inline binary content of `event` into `blobs` and serializes the result.
pub async fn prepare(
    blobs: &PgBlobStore,
    seq: u64,
    mut event: Event,
) -> Result<PreparedEvent, ChannelError> {
    if let Some(kind) = event.kind.as_mut() {
        for block in content_blocks_mut(kind) {
            externalize_block(blobs, block).await?;
        }
    }
    let kind = event.kind.as_ref().map_or("unknown", kind_name);
    let prompt_id = match &event.kind {
        Some(event::Kind::UserMessage(m)) => m.prompt_id.parse().ok(),
        _ => None,
    };
    Ok(PreparedEvent {
        seq,
        kind,
        schema_version: i32::try_from(event.schema_version).unwrap_or(1),
        occurred_at: event
            .occurred_at
            .as_ref()
            .and_then(timestamp_to_utc)
            .unwrap_or_else(Utc::now),
        payload: serde_json::to_value(&event)?,
        prompt_id,
    })
}

/// Stores `event` once per (run, seq). Returns the task-wide id of a newly stored event, `None`
/// for a duplicate. A stored `UserMessage` marks its queued message as delivered by `run_id` in
/// the same transaction.
pub async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    run_id: RunId,
    event: &PreparedEvent,
) -> Result<Option<i64>, ChannelError> {
    let seq = i64::try_from(event.seq)
        .map_err(|_| ChannelError::InvalidData("sequence number out of range".into()))?;
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO transcript_events
             (task_id, run_id, seq, schema_version, occurred_at, kind, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (run_id, seq) DO NOTHING
         RETURNING id",
    )
    .bind(task_id.as_uuid())
    .bind(run_id.as_uuid())
    .bind(seq)
    .bind(event.schema_version)
    .bind(event.occurred_at)
    .bind(event.kind)
    .bind(&event.payload)
    .fetch_optional(&mut **tx)
    .await?;

    if let (Some(_), Some(prompt_id)) = (id, event.prompt_id) {
        sqlx::query(
            "UPDATE task_messages
             SET delivered_run_id = COALESCE(delivered_run_id, $2),
                 delivered_at = COALESCE(delivered_at, now())
             WHERE id = $1 AND task_id = $3",
        )
        .bind(prompt_id.as_uuid())
        .bind(run_id.as_uuid())
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }
    Ok(id)
}

fn timestamp_to_utc(ts: &Timestamp) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(ts.seconds, u32::try_from(ts.nanos).ok()?)
}

const fn kind_name(kind: &event::Kind) -> &'static str {
    match kind {
        event::Kind::TurnStarted(_) => "turn_started",
        event::Kind::UserMessage(_) => "user_message",
        event::Kind::AgentMessageChunk(_) => "agent_message_chunk",
        event::Kind::AgentThoughtChunk(_) => "agent_thought_chunk",
        event::Kind::ToolCall(_) => "tool_call",
        event::Kind::ToolCallUpdate(_) => "tool_call_update",
        event::Kind::Plan(_) => "plan",
        event::Kind::TurnEnded(_) => "turn_ended",
        event::Kind::Notice(_) => "notice",
    }
}

fn content_blocks_mut(kind: &mut event::Kind) -> Vec<&mut ContentBlock> {
    fn tool_content(
        items: &mut [df_proto::transcript::v1::ToolCallContent],
    ) -> Vec<&mut ContentBlock> {
        items
            .iter_mut()
            .filter_map(|c| match &mut c.kind {
                Some(tool_call_content::Kind::Content(block)) => Some(block),
                _ => None,
            })
            .collect()
    }
    match kind {
        event::Kind::UserMessage(m) => m.content.iter_mut().collect(),
        event::Kind::AgentMessageChunk(m) => m.content.iter_mut().collect(),
        event::Kind::AgentThoughtChunk(m) => m.content.iter_mut().collect(),
        event::Kind::ToolCall(c) => tool_content(&mut c.content),
        event::Kind::ToolCallUpdate(u) => u
            .content
            .as_mut()
            .map(|list| tool_content(&mut list.items))
            .unwrap_or_default(),
        event::Kind::TurnStarted(_)
        | event::Kind::Plan(_)
        | event::Kind::TurnEnded(_)
        | event::Kind::Notice(_) => Vec::new(),
    }
}

async fn externalize_block(
    blobs: &PgBlobStore,
    block: &mut ContentBlock,
) -> Result<(), ChannelError> {
    match &mut block.kind {
        Some(content_block::Kind::Image(image)) => {
            externalize_blob(blobs, &image.mime_type, image.data.as_mut()).await
        }
        Some(content_block::Kind::Resource(resource)) => {
            let mime = resource.mime_type.clone();
            match resource.contents.as_mut() {
                Some(embedded_resource::Contents::Blob(blob)) => {
                    externalize_blob(blobs, &mime, Some(blob)).await
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

async fn externalize_blob(
    blobs: &PgBlobStore,
    mime_type: &str,
    data: Option<&mut BlobData>,
) -> Result<(), ChannelError> {
    let Some(data) = data else { return Ok(()) };
    if let Some(blob_data::Source::Inline(bytes)) = &data.source {
        let sha256 = blobs.put(mime_type, bytes).await?;
        data.source = Some(blob_data::Source::Sha256(sha256));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Env;
    use bytes::Bytes;
    use df_proto::transcript::v1::{
        AgentMessageChunk, ImageContent, TextContent, UserMessage,
        tool_call_content::Kind as TcKind,
    };
    use sqlx::PgPool;

    fn text(t: &str) -> ContentBlock {
        ContentBlock {
            kind: Some(content_block::Kind::Text(TextContent { text: t.into() })),
        }
    }

    fn image_event(bytes: &'static [u8]) -> Event {
        Event {
            schema_version: 1,
            occurred_at: Some(Timestamp {
                seconds: 1_760_000_000,
                nanos: 0,
            }),
            turn_id: "t1".into(),
            kind: Some(event::Kind::AgentMessageChunk(AgentMessageChunk {
                message_id: "m1".into(),
                content: Some(ContentBlock {
                    kind: Some(content_block::Kind::Image(ImageContent {
                        mime_type: "image/png".into(),
                        data: Some(BlobData {
                            source: Some(blob_data::Source::Inline(Bytes::from_static(bytes))),
                        }),
                    })),
                }),
            })),
        }
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn inline_image_is_replaced_by_its_hash(pool: PgPool) -> anyhow::Result<()> {
        let blobs = PgBlobStore::new(pool);
        let prepared = prepare(&blobs, 1, image_event(b"png-bytes")).await?;

        let sha = df_db::sha256_hex(b"png-bytes");
        let data = &prepared.payload["agentMessageChunk"]["content"]["image"]["data"];
        assert_eq!(data["sha256"], sha.as_str());
        assert!(data.get("inline").is_none());
        assert_eq!(
            blobs.get(&sha).await?.expect("blob stored").data,
            b"png-bytes"
        );
        Ok(())
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn tool_call_content_blobs_are_externalized_too(pool: PgPool) -> anyhow::Result<()> {
        let blobs = PgBlobStore::new(pool);
        let mut event = image_event(b"shot");
        let Some(event::Kind::AgentMessageChunk(chunk)) = event.kind.take() else {
            unreachable!("built by image_event")
        };
        event.kind = Some(event::Kind::ToolCall(df_proto::transcript::v1::ToolCall {
            tool_call_id: "tc".into(),
            content: vec![df_proto::transcript::v1::ToolCallContent {
                kind: Some(TcKind::Content(chunk.content.expect("content"))),
            }],
            ..Default::default()
        }));
        let prepared = prepare(&blobs, 1, event).await?;
        let data = &prepared.payload["toolCall"]["content"][0]["content"]["image"]["data"];
        assert_eq!(data["sha256"], df_db::sha256_hex(b"shot").as_str());
        Ok(())
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn duplicate_events_are_stored_once_and_ids_increase_across_runs(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let env = Env::new(pool.clone()).await;
        let blobs = PgBlobStore::new(pool.clone());
        let run1 = env.insert_run(env.task.id, 1, "running").await;
        sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
            .bind(run1.as_uuid())
            .execute(&pool)
            .await?;
        let run2 = env.insert_run(env.task.id, 2, "running").await;

        let event = |t: &str| Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(event::Kind::AgentMessageChunk(AgentMessageChunk {
                message_id: "m".into(),
                content: Some(text(t)),
            })),
        };
        let mut tx = pool.begin().await?;
        let first = insert(
            &mut tx,
            env.task.id,
            run1,
            &prepare(&blobs, 1, event("a")).await?,
        )
        .await?;
        let dup = insert(
            &mut tx,
            env.task.id,
            run1,
            &prepare(&blobs, 1, event("a")).await?,
        )
        .await?;
        let later = insert(
            &mut tx,
            env.task.id,
            run2,
            &prepare(&blobs, 1, event("b")).await?,
        )
        .await?;
        tx.commit().await?;

        assert!(dup.is_none());
        assert!(later.expect("stored") > first.expect("stored"));
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transcript_events")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows, 2);
        Ok(())
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn user_message_marks_the_queued_prompt_delivered(pool: PgPool) -> anyhow::Result<()> {
        let env = Env::new(pool.clone()).await;
        let blobs = PgBlobStore::new(pool.clone());
        let run = env.insert_run(env.task.id, 1, "running").await;
        let prompt = df_db::list_undelivered_messages(&pool, env.task.id)
            .await?
            .remove(0);

        let event = Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(event::Kind::UserMessage(UserMessage {
                prompt_id: prompt.id.to_string(),
                author: None,
                content: vec![text("hi")],
            })),
        };
        let mut tx = pool.begin().await?;
        insert(&mut tx, env.task.id, run, &prepare(&blobs, 1, event).await?).await?;
        tx.commit().await?;

        assert!(
            df_db::list_undelivered_messages(&pool, env.task.id)
                .await?
                .is_empty()
        );
        let delivered_by: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT delivered_run_id FROM task_messages WHERE id = $1")
                .bind(prompt.id.as_uuid())
                .fetch_one(&pool)
                .await?;
        assert_eq!(delivered_by, Some(run.as_uuid()));
        Ok(())
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn user_message_cannot_deliver_the_prompt_of_another_task(
        pool: PgPool,
    ) -> anyhow::Result<()> {
        let env = Env::new(pool.clone()).await;
        let blobs = PgBlobStore::new(pool.clone());
        let other = df_db::create_task(
            &pool,
            df_db::NewTask {
                project_id: env.project_id,
                persona_id: env.persona_id,
                title: "Other".into(),
                description: String::new(),
                issue_number: None,
                created_by: None,
            },
        )
        .await?
        .0;
        let foreign = df_db::list_undelivered_messages(&pool, other.id)
            .await?
            .remove(0);
        let run = env.insert_run(env.task.id, 1, "running").await;

        // The runtime of `env.task` names a prompt of `other`.
        let event = Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(event::Kind::UserMessage(UserMessage {
                prompt_id: foreign.id.to_string(),
                author: None,
                content: vec![text("hi")],
            })),
        };
        let mut tx = pool.begin().await?;
        insert(&mut tx, env.task.id, run, &prepare(&blobs, 1, event).await?).await?;
        tx.commit().await?;

        assert_eq!(
            df_db::list_undelivered_messages(&pool, other.id)
                .await?
                .len(),
            1
        );
        Ok(())
    }
}
