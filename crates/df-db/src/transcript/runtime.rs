//! Runtime events on their way into the transcript: canonical JSON payloads and blob extraction
//! (`specs/transcript-storage/spec.md`).

use df_core::PromptId;
use df_proto::time::from_proto;
use df_proto::transcript::v1::{
    BlobData, ContentBlock, Event, blob_data, content_block, embedded_resource, event,
    tool_call_content,
};
use jiff::Timestamp;
use serde_json::Value;

use super::{
    KIND_AGENT_MESSAGE_CHUNK, KIND_AGENT_THOUGHT_CHUNK, KIND_FACTORY_ACTIVITY, KIND_FORGE_ACTIVITY,
    KIND_NOTICE, KIND_PLAN, KIND_TOOL_CALL, KIND_TOOL_CALL_UPDATE, KIND_TURN_ENDED,
    KIND_TURN_STARTED, KIND_USER_MESSAGE,
};
use crate::blobs::PgBlobStore;
use crate::pool::DbError;

/// An event ready to be stored: binary data moved to the blob store, payload serialized.
#[derive(Debug, Clone)]
pub struct PreparedEvent {
    pub seq: u64,
    pub kind: &'static str,
    pub schema_version: i32,
    pub occurred_at: Timestamp,
    pub payload: Value,
    /// Set for `UserMessage` events: the queued message this event confirms.
    pub prompt_id: Option<PromptId>,
}

/// Moves inline binary content of `event` into `blobs` and serializes the result.
pub async fn prepare(
    blobs: &PgBlobStore,
    seq: u64,
    mut event: Event,
) -> Result<PreparedEvent, DbError> {
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
            .and_then(from_proto)
            .unwrap_or_else(Timestamp::now),
        payload: serde_json::to_value(&event)?,
        prompt_id,
    })
}

/// Whether the runtime may send `event`. Forge and factory activity are written by the control
/// plane only (design D6, ADR-0057): they are neither stored nor ever shown as coming from the
/// run.
pub fn allowed_from_runtime(event: &Event) -> bool {
    event
        .kind
        .as_ref()
        .is_none_or(|kind| kind_allowed_from_runtime(kind_name(kind)))
}

/// [`allowed_from_runtime`] by stored kind name; the one place that knows which kinds are the
/// control plane's.
pub(super) fn kind_allowed_from_runtime(kind: &str) -> bool {
    !matches!(kind, KIND_FORGE_ACTIVITY | KIND_FACTORY_ACTIVITY)
}

/// Moves inline image and blob data of `block` into `blobs`; the block then refers to it by hash.
pub async fn externalize_block(
    blobs: &PgBlobStore,
    block: &mut ContentBlock,
) -> Result<(), DbError> {
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

const fn kind_name(kind: &event::Kind) -> &'static str {
    match kind {
        event::Kind::TurnStarted(_) => KIND_TURN_STARTED,
        event::Kind::UserMessage(_) => KIND_USER_MESSAGE,
        event::Kind::AgentMessageChunk(_) => KIND_AGENT_MESSAGE_CHUNK,
        event::Kind::AgentThoughtChunk(_) => KIND_AGENT_THOUGHT_CHUNK,
        event::Kind::ToolCall(_) => KIND_TOOL_CALL,
        event::Kind::ToolCallUpdate(_) => KIND_TOOL_CALL_UPDATE,
        event::Kind::Plan(_) => KIND_PLAN,
        event::Kind::TurnEnded(_) => KIND_TURN_ENDED,
        event::Kind::Notice(_) => KIND_NOTICE,
        event::Kind::ForgeActivity(_) => KIND_FORGE_ACTIVITY,
        event::Kind::FactoryActivity(_) => KIND_FACTORY_ACTIVITY,
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
        | event::Kind::Notice(_)
        | event::Kind::ForgeActivity(_)
        | event::Kind::FactoryActivity(_) => Vec::new(),
    }
}

async fn externalize_blob(
    blobs: &PgBlobStore,
    mime_type: &str,
    data: Option<&mut BlobData>,
) -> Result<(), DbError> {
    let Some(data) = data else { return Ok(()) };
    if let Some(blob_data::Source::Inline(bytes)) = &data.source {
        let sha256 = blobs.put(mime_type, bytes).await?;
        data.source = Some(blob_data::Source::Sha256(sha256));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
