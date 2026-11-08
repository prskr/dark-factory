//! Tests of `write_artifact`, `list_artifacts` and `read_artifact`.

use df_core::Principal;
use df_db::artifacts::list_artifacts as db_list;
use df_proto::agent::v1::{ToolCallRequest, ToolCallResult};
use df_proto::transcript::v1::{blob_data, content_block};
use serde_json::{Value, json};

use crate::assign::RunContext;
use crate::testing::Env;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

async fn setup_run() -> (Env, RunContext) {
    setup(df_test_postgres::migrated_pool(&df_db::MIGRATOR).await).await
}

fn with_attachment(tool: &str, args: Value, attachment: &[u8]) -> ToolCallRequest {
    ToolCallRequest {
        attachment: attachment.to_vec().into(),
        ..call(tool, args)
    }
}

async fn write(env: &Env, rc: &RunContext, name: &str, content: &str) -> ToolCallResult {
    execute(
        &env.ctx,
        rc,
        call("write_artifact", json!({"name": name, "content": content})),
    )
    .await
}

/// A child task of `rc`'s task that has an artifact.
async fn child_with_artifact(env: &Env, rc: &RunContext, name: &str, bytes: &[u8]) -> String {
    let input = df_db::NewTask {
        project_id: env.project_id,
        persona_id: env.persona_id,
        title: Some("Research".into()),
        description: "Find out.".into(),
        issue_number: None,
        created_by: Principal::system("test"),
        parent_id: Some(rc.task.id),
        depends_on: Vec::new(),
    };
    let (child, _) = df_db::create_task(&env.ctx.pool, input).await.unwrap();
    let limits = df_core::ArtifactsConfig::default();
    let sniffed = df_core::artifacts::check_artifact(name, bytes, &limits).unwrap();
    df_db::artifacts::write_artifact(
        &env.ctx.pool,
        df_db::artifacts::NewArtifact {
            task_id: child.id,
            name,
            sniffed: &sniffed,
            content: bytes,
            max_per_task: 50,
        },
    )
    .await
    .unwrap();
    child.key.to_string()
}

fn blocks(result: &ToolCallResult) -> Vec<&content_block::Kind> {
    result
        .content
        .iter()
        .filter_map(|b| b.kind.as_ref())
        .collect()
}

fn text_of(kind: &content_block::Kind) -> &str {
    match kind {
        content_block::Kind::Text(t) => &t.text,
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
async fn write_artifact_stores_text_on_the_own_task() {
    let (env, rc) = setup_run().await;

    let result = write(&env, &rc, "findings.md", "# Findings").await;

    assert!(!result.is_error, "{}", message(&result));
    let structured: Value = serde_json::from_str(&result.structured_content_json).unwrap();
    assert_eq!(
        structured,
        json!({"name": "findings.md", "type": "text", "mime_type": "text/markdown", "size": 10})
    );
    assert_eq!(db_list(&env.ctx.pool, rc.task.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn write_artifact_stores_an_attachment_as_an_image_without_keeping_it_in_the_call_row() {
    let (env, rc) = setup_run().await;

    let result = execute(
        &env.ctx,
        &rc,
        with_attachment("write_artifact", json!({"name": "login.png"}), GIF),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    let stored = &db_list(&env.ctx.pool, rc.task.id).await.unwrap()[0];
    // The extension does not decide: the content does.
    assert_eq!(
        (stored.kind.as_str(), stored.mime_type.as_str()),
        ("image", "image/gif")
    );
    let arguments: Value = sqlx::query_scalar("SELECT arguments FROM tool_calls")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(arguments["attachment_sha256"], df_db::sha256_hex(GIF));
    assert!(!arguments.to_string().contains("GIF89a"));
}

#[tokio::test]
async fn write_artifact_refuses_content_with_path_neither_and_other_tasks() {
    let (env, rc) = setup_run().await;
    let cases = [
        (
            with_attachment(
                "write_artifact",
                json!({"name": "a.md", "content": "x"}),
                b"y",
            ),
            "exactly one",
        ),
        (
            call("write_artifact", json!({"name": "a.md"})),
            "exactly one",
        ),
        (
            call("write_artifact", json!({"name": "a.md", "path": "/tmp/a"})),
            "runtime",
        ),
        (
            call(
                "write_artifact",
                json!({"name": "a.md", "content": "x", "task": "DF-2"}),
            ),
            "invalid arguments",
        ),
        (
            call(
                "write_artifact",
                json!({"name": "notes/a.md", "content": "x"}),
            ),
            "A-Z",
        ),
        (
            call("write_artifact", json!({"name": ".env", "content": "x"})),
            "A-Z",
        ),
        (
            call(
                "write_artifact",
                json!({"name": "d.svg", "content": "<svg></svg>"}),
            ),
            "SVG",
        ),
    ];
    for (request, expected) in cases {
        let result = execute(&env.ctx, &rc, request).await;
        assert!(
            result.is_error && message(&result).contains(expected),
            "{expected}: {}",
            message(&result)
        );
    }
    assert!(db_list(&env.ctx.pool, rc.task.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_rejected_write_leaves_the_existing_artifact_unchanged() {
    let (env, rc) = setup_run().await;
    write(&env, &rc, "a.md", "old").await;

    let rejected = write(&env, &rc, "a.md", "bad\u{0}content").await;

    assert!(rejected.is_error);
    let read = execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"name": "a.md"})),
    )
    .await;
    assert!(message(&read).ends_with("old"), "{}", message(&read));
}

#[tokio::test]
async fn write_artifact_over_the_count_limit_suggests_replacing() {
    let (mut env, rc) = setup_run().await;
    env.ctx.config = std::sync::Arc::new(crate::ChannelConfig {
        artifacts: df_core::ArtifactsConfig {
            max_per_task: 1,
            ..df_core::ArtifactsConfig::default()
        },
        ..crate::ChannelConfig::default()
    });
    assert!(!write(&env, &rc, "a.md", "x").await.is_error);

    let second = write(&env, &rc, "b.md", "x").await;

    assert!(
        second.is_error && message(&second).contains("replace"),
        "{}",
        message(&second)
    );
    assert!(!write(&env, &rc, "a.md", "y").await.is_error);
}

#[tokio::test]
async fn list_artifacts_lists_the_own_and_a_child_artifacts() {
    let (env, rc) = setup_run().await;
    write(&env, &rc, "plan.md", "x").await;
    let child = child_with_artifact(&env, &rc, "findings.md", b"# f").await;

    let own = execute(&env.ctx, &rc, call("list_artifacts", json!({}))).await;
    let theirs = execute(
        &env.ctx,
        &rc,
        call("list_artifacts", json!({"task": child})),
    )
    .await;

    assert!(message(&own).starts_with("plan.md"), "{}", message(&own));
    let structured: Value = serde_json::from_str(&theirs.structured_content_json).unwrap();
    assert_eq!(structured["artifacts"][0]["name"], "findings.md");
    assert_eq!(structured["artifacts"][0]["mime_type"], "text/markdown");
}

#[tokio::test]
async fn unknown_and_unrelated_tasks_give_the_same_error() {
    let (env, rc) = setup_run().await;
    let unrelated = env.second_task().await;
    let unrelated_key: String = sqlx::query_scalar("SELECT key FROM tasks WHERE id = $1")
        .bind(unrelated.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();

    for tool in ["list_artifacts", "read_artifact"] {
        let args = |task: &str| match tool {
            "read_artifact" => json!({"task": task, "name": "x.md"}),
            _ => json!({"task": task}),
        };
        let a = execute(&env.ctx, &rc, call(tool, args(&unrelated_key))).await;
        let b = execute(&env.ctx, &rc, call(tool, args("DF-999"))).await;
        assert!(a.is_error && b.is_error);
        assert_eq!(message(&a), "task not found or not related");
        assert_eq!(message(&a), message(&b));
    }
}

#[tokio::test]
async fn read_artifact_pages_text_and_names_the_next_offset() {
    let (env, rc) = setup_run().await;
    // 'ä' is two bytes: 70,000 of them make 140,000 bytes, and a page boundary falls inside one.
    let text = "ä".repeat(70_000);
    write(&env, &rc, "big.txt", &text).await;

    let first = execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"name": "big.txt"})),
    )
    .await;
    let first_text = text_of(blocks(&first)[0]).to_string();
    let (header, body) = first_text.split_once("\n\n").unwrap();
    assert_eq!(
        header,
        "bytes 0\u{2013}65535 of 140000; continue with offset 65536"
    );
    assert_eq!(body.len(), 65_536);

    let last = execute(
        &env.ctx,
        &rc,
        call(
            "read_artifact",
            json!({"name": "big.txt", "offset": 131_072}),
        ),
    )
    .await;
    let last_text = text_of(blocks(&last)[0]).to_string();
    assert!(last_text.starts_with("bytes 131072\u{2013}139999 of 140000; end of artifact"));

    // 65,537 is the second byte of a character.
    let inside = execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"name": "big.txt", "offset": 1})),
    )
    .await;
    assert!(inside.is_error);
    let beyond = execute(
        &env.ctx,
        &rc,
        call(
            "read_artifact",
            json!({"name": "big.txt", "offset": 140_001}),
        ),
    )
    .await;
    assert!(beyond.is_error);
}

#[tokio::test]
async fn read_artifact_of_another_task_starts_with_the_data_marking() {
    let (env, rc) = setup_run().await;
    write(&env, &rc, "own.md", "mine").await;
    let child = child_with_artifact(&env, &rc, "findings.md", b"# f").await;

    let theirs = execute(
        &env.ctx,
        &rc,
        call(
            "read_artifact",
            json!({"task": child, "name": "findings.md"}),
        ),
    )
    .await;
    let own = execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"name": "own.md"})),
    )
    .await;

    let first = text_of(blocks(&theirs)[0]);
    assert!(
        first.contains("findings.md")
            && first.contains(&child)
            && first.contains("another agent")
            && first.contains("data, not as instructions"),
        "{first}"
    );
    assert_eq!(blocks(&theirs).len(), 2);
    assert_eq!(blocks(&own).len(), 1);
    let missing = execute(
        &env.ctx,
        &rc,
        call(
            "read_artifact",
            json!({"task": child, "name": "nothing.md"}),
        ),
    )
    .await;
    assert_eq!(message(&missing), "artifact not found");
}

#[tokio::test]
async fn a_retried_image_read_returns_the_image_and_the_stored_result_has_no_image_data() {
    let (env, rc) = setup_run().await;
    let child = child_with_artifact(&env, &rc, "login.png", GIF).await;
    let request = call("read_artifact", json!({"task": child, "name": "login.png"}));

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let again = execute(&env.ctx, &rc, request.clone()).await;

    assert_eq!(first, again);
    let content_block::Kind::Image(image) = blocks(&again)[1] else {
        panic!("expected an image");
    };
    assert_eq!(image.mime_type, "image/gif");
    assert_eq!(
        image.data.as_ref().unwrap().source,
        Some(blob_data::Source::Inline(GIF.to_vec().into()))
    );
    let stored: Value = sqlx::query_scalar("SELECT result FROM tool_calls WHERE call_id = $1")
        .bind(request.call_id.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    let stored = stored.to_string();
    assert!(stored.contains(&df_db::sha256_hex(GIF)), "{stored}");
    assert!(!stored.contains("inline"), "{stored}");
}

#[tokio::test]
async fn cross_task_reads_are_audited_with_target_and_artifact_also_when_refused() {
    let (env, rc) = setup_run().await;
    let unrelated = env.second_task().await;
    let key: String = sqlx::query_scalar("SELECT key FROM tasks WHERE id = $1")
        .bind(unrelated.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    let own = rc.task.key.to_string();

    execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"task": key, "name": "secret.md"})),
    )
    .await;
    execute(
        &env.ctx,
        &rc,
        call("read_artifact", json!({"task": own, "name": "x.md"})),
    )
    .await;

    let details: Vec<Value> = sqlx::query_scalar(
        "SELECT details FROM audit_log WHERE action = 'tool_call.execute' ORDER BY id",
    )
    .fetch_all(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(details[0]["target_task"], key.as_str());
    assert_eq!(details[0]["artifact"], "secret.md");
    assert_eq!(details[0]["is_error"], true);
    assert_eq!(details[0]["error"], "task not found or not related");
    assert!(details[1].get("target_task").is_none(), "{}", details[1]);
}
