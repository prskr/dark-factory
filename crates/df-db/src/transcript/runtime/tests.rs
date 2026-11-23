//! Tests of the preparation of runtime events (container-backed).

use bytes::Bytes;
use df_proto::transcript::v1::{
    AgentMessageChunk, ImageContent, ToolCall, ToolCallContent, tool_call_content::Kind as TcKind,
};

use super::*;
use crate::blobs::sha256_hex;

fn image_event(bytes: &'static [u8]) -> Event {
    Event {
        schema_version: 1,
        occurred_at: Some(df_proto::pbjson_types::Timestamp {
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
                hidden: false,
            }),
        })),
    }
}

#[tokio::test]
async fn inline_image_is_replaced_by_its_hash() -> Result<(), DbError> {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let blobs = PgBlobStore::new(pool);
    let prepared = prepare(&blobs, 1, image_event(b"png-bytes")).await?;

    let sha = sha256_hex(b"png-bytes");
    let data = &prepared.payload["agentMessageChunk"]["content"]["image"]["data"];
    assert_eq!(data["sha256"], sha.as_str());
    assert!(data.get("inline").is_none());
    assert_eq!(
        blobs.get(&sha).await?.expect("blob stored").data,
        b"png-bytes"
    );
    Ok(())
}

#[tokio::test]
async fn tool_call_content_blobs_are_externalized_too() -> Result<(), DbError> {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let blobs = PgBlobStore::new(pool);
    let mut event = image_event(b"shot");
    let Some(event::Kind::AgentMessageChunk(chunk)) = event.kind.take() else {
        panic!("built by image_event")
    };
    event.kind = Some(event::Kind::ToolCall(ToolCall {
        tool_call_id: "tc".into(),
        content: vec![ToolCallContent {
            kind: Some(TcKind::Content(chunk.content.expect("content"))),
        }],
        ..Default::default()
    }));
    let prepared = prepare(&blobs, 1, event).await?;
    let data = &prepared.payload["toolCall"]["content"][0]["content"]["image"]["data"];
    assert_eq!(data["sha256"], sha256_hex(b"shot").as_str());
    Ok(())
}

#[test]
fn only_the_control_plane_may_write_forge_and_factory_activity() {
    let event = |kind| Event {
        kind: Some(kind),
        ..Default::default()
    };
    assert!(allowed_from_runtime(&event(event::Kind::Notice(
        df_proto::transcript::v1::Notice::default()
    ))));
    assert!(!allowed_from_runtime(&event(event::Kind::ForgeActivity(
        df_proto::transcript::v1::ForgeActivity::default()
    ))));
    assert!(!allowed_from_runtime(&event(event::Kind::FactoryActivity(
        df_proto::transcript::v1::FactoryActivity::default()
    ))));
}
