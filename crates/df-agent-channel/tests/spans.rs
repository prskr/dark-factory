//! Spans of the agent channel (`specs/telemetry/spec.md`, "Span coverage").
//!
//! A test binary of its own: tracing caches which callsites are enabled per process, and tests
//! running in parallel threads make that cache race with a scoped subscriber.

use df_agent_channel::runs::get_run;
use df_agent_channel::session::run;
use df_agent_channel::testing::Env;
use df_proto::agent::v1::{AgentMessage, Hello, agent_message};
use df_proto::transcript::v1 as wire;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tracing_subscriber::layer::SubscriberExt as _;

fn msg(seq: u64, body: agent_message::Body) -> AgentMessage {
    AgentMessage {
        seq,
        body: Some(body),
    }
}

fn text_event() -> agent_message::Body {
    agent_message::Body::TranscriptEvent(wire::Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t".into(),
        kind: Some(wire::event::Kind::AgentMessageChunk(
            wire::AgentMessageChunk {
                message_id: "m".into(),
                content: Some(wire::ContentBlock {
                    kind: Some(wire::content_block::Kind::Text(wire::TextContent {
                        text: "secret-prompt-text".into(),
                    })),
                    hidden: false,
                }),
            },
        )),
    })
}

#[tokio::test]
async fn sessions_and_batches_are_spans_without_message_content() -> anyhow::Result<()> {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("test"));
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let row = get_run(&env.ctx.pool, run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("run not found"))?;
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let inbound = futures::stream::iter(
        [
            msg(0, agent_message::Body::Hello(Hello::default())),
            msg(1, text_event()),
            msg(2, text_event()),
        ]
        .into_iter()
        .map(Ok),
    );

    run(
        env.ctx.clone(),
        row,
        inbound,
        tx,
        tokio::sync::watch::channel(false).1,
    )
    .await?;

    let spans = exporter.get_finished_spans()?;
    let attribute = |span: &str, key: &str| {
        spans
            .iter()
            .find(|s| s.name == span)
            .and_then(|s| s.attributes.iter().find(|kv| kv.key.as_str() == key))
            .map(|kv| kv.value.to_string())
    };
    assert_eq!(
        attribute("agent.persist_batch", "messages").as_deref(),
        Some("2")
    );
    assert_eq!(
        attribute("agent.session", "run_id"),
        Some(run_id.to_string())
    );
    assert!(
        !format!("{spans:?}").contains("secret-prompt-text"),
        "message content leaked into a span"
    );
    Ok(())
}
