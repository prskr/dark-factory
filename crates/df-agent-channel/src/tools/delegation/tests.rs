//! Tests of `create_task`, `list_personas` and `send_message`.

use df_core::{DependencyKind, Principal};
use serde_json::json;
use sqlx::PgPool;

use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

async fn pool() -> PgPool {
    df_test_postgres::migrated_pool(&df_db::MIGRATOR).await
}

async fn add_persona(pool: &PgPool, name: &str, description: &str) {
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, model, model_provider_id, description)
         SELECT $1, $2, 'p', model, model_provider_id, $3 FROM personas WHERE name = 'Linus'",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(name)
    .bind(description)
    .execute(pool)
    .await
    .unwrap();
}

fn review_args(depends_on: serde_json::Value) -> serde_json::Value {
    json!({
        "persona": "Reviewer",
        "title": "Review DF-2",
        "prompt": "Review the pull request of DF-2.",
        "depends_on": depends_on,
    })
}

#[tokio::test]
async fn create_task_makes_a_child_that_waits_for_its_dependency() {
    let (env, rc) = setup(pool().await).await;
    let pool = &env.ctx.pool;
    add_persona(pool, "Reviewer", "").await;
    let implementation = execute(
        &env.ctx,
        &rc,
        call(
            "create_task",
            json!({"persona": "Linus", "title": "Implement", "prompt": "Do it."}),
        ),
    )
    .await;
    assert!(!implementation.is_error, "{}", message(&implementation));
    assert!(message(&implementation).contains("DF-2 with status queued"));

    let review = execute(
        &env.ctx,
        &rc,
        call(
            "create_task",
            review_args(json!([{"task": "DF-2", "kind": "completion"}])),
        ),
    )
    .await;

    assert!(!review.is_error, "{}", message(&review));
    let structured: serde_json::Value =
        serde_json::from_str(&review.structured_content_json).unwrap();
    assert_eq!(
        structured,
        json!({"key": "DF-3", "status": "awaiting_dependencies"})
    );
    let task = df_db::get_task(
        pool,
        df_core::TaskId::from_uuid(
            sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-3'")
                .fetch_one(pool)
                .await
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(task.parent_id, Some(rc.task.id));
    assert!(matches!(task.created_by, Principal::Agent { task_id, .. } if task_id == rc.task.id));
    assert_eq!(task.title, "Review DF-2");
    let source: String = sqlx::query_scalar("SELECT title_source FROM tasks WHERE key = 'DF-3'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(source, "agent");
    let kind: String = sqlx::query_scalar("SELECT kind FROM task_dependencies")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(kind, DependencyKind::Completion.as_str());
}

#[tokio::test]
async fn create_task_refuses_unknown_personas_empty_input_and_invalid_dependencies() {
    let (env, rc) = setup(pool().await).await;
    add_persona(&env.ctx.pool, "Reviewer", "").await;
    let cases = [
        (
            json!({"persona": "Nobody", "title": "T", "prompt": "P"}),
            "Nobody",
        ),
        (
            json!({"persona": "Linus", "title": "  ", "prompt": "P"}),
            "title",
        ),
        (
            json!({"persona": "Linus", "title": "T", "prompt": "  "}),
            "prompt",
        ),
        (
            review_args(json!([{"task": "DF-99", "kind": "done"}])),
            "DF-99",
        ),
        (
            review_args(json!([{"task": "nonsense", "kind": "done"}])),
            "nonsense",
        ),
    ];
    for (args, named) in cases {
        let result = execute(&env.ctx, &rc, call("create_task", args)).await;
        assert!(
            result.is_error && message(&result).contains(named),
            "{}",
            message(&result)
        );
    }
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(tasks, 1);
}

#[tokio::test]
async fn a_retried_create_task_returns_the_same_key_and_creates_one_task() {
    let (env, rc) = setup(pool().await).await;
    let request = call(
        "create_task",
        json!({"persona": "Linus", "title": "Once", "prompt": "Do it."}),
    );
    let first = execute(&env.ctx, &rc, request.clone()).await;
    let again = execute(&env.ctx, &rc, request).await;
    assert_eq!(message(&first), message(&again));
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(tasks, 2);
}

#[tokio::test]
async fn list_personas_returns_names_with_descriptions_ordered_by_name() {
    let (env, rc) = setup(pool().await).await;
    add_persona(&env.ctx.pool, "Reviewer", "").await;
    sqlx::query("UPDATE personas SET description = 'Rust implementation' WHERE name = 'Linus'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    let result = execute(&env.ctx, &rc, call("list_personas", json!({}))).await;

    assert!(!result.is_error);
    let structured: serde_json::Value =
        serde_json::from_str(&result.structured_content_json).unwrap();
    assert_eq!(
        structured,
        json!({"personas": [
            {"name": "Linus", "description": "Rust implementation"},
            {"name": "Reviewer", "description": ""}
        ]})
    );
    assert_eq!(message(&result), "- Linus: Rust implementation\n- Reviewer");
}

async fn child_of(
    env: &crate::testing::Env,
    rc: &crate::assign::RunContext,
    parent: bool,
) -> String {
    let input = df_db::NewTask {
        project_id: env.project_id,
        persona_id: env.persona_id,
        title: Some("Child".into()),
        description: "Work".into(),
        issue_number: None,
        created_by: Principal::system("test"),
        parent_id: parent.then_some(rc.task.id),
        depends_on: Vec::new(),
    };
    let (task, _) = df_db::create_task(&env.ctx.pool, input).await.unwrap();
    sqlx::query("UPDATE tasks SET status = 'in_review' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    task.key.to_string()
}

#[tokio::test]
async fn send_message_reopens_a_child_with_the_agent_as_author() {
    let (env, rc) = setup(pool().await).await;
    let key = child_of(&env, &rc, true).await;

    let result = execute(
        &env.ctx,
        &rc,
        call("send_message", json!({"task": key, "text": "review again"})),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    let structured: serde_json::Value =
        serde_json::from_str(&result.structured_content_json).unwrap();
    assert_eq!(structured["status"], "queued");
    assert!(structured["prompt_id"].is_string());
    let author: serde_json::Value = sqlx::query_scalar(
        "SELECT author FROM task_messages WHERE task_id = (SELECT id FROM tasks WHERE key = $1)
         ORDER BY queued_at DESC LIMIT 1",
    )
    .bind(&key)
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    let author: Principal = serde_json::from_value(author).unwrap();
    assert!(matches!(author, Principal::Agent { task_id, .. } if task_id == rc.task.id));
}

#[tokio::test]
async fn send_message_refuses_tasks_that_are_not_children_and_empty_text() {
    let (env, rc) = setup(pool().await).await;
    let sibling = child_of(&env, &rc, false).await;
    let child = child_of(&env, &rc, true).await;
    for (args, named) in [
        (json!({"task": sibling, "text": "hello"}), "not a child"),
        (json!({"task": "OPS-1", "text": "hello"}), "not a child"),
        (json!({"task": child, "text": "   "}), "empty"),
    ] {
        let result = execute(&env.ctx, &rc, call("send_message", args)).await;
        assert!(
            result.is_error && message(&result).contains(named),
            "{}",
            message(&result)
        );
    }
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_messages WHERE author->>'kind' = 'agent'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!(queued, 0);
}
