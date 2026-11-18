//! Tests of the task tree rules of the tools (`task-tree`): `base_task`, the delegation limits
//! and the completion gate.

use df_core::config::TasksConfig;
use serde_json::json;
use sqlx::PgPool;

use crate::ChannelConfig;
use crate::testing::Env;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

async fn pool() -> PgPool {
    df_test_postgres::migrated_pool(&df_db::MIGRATOR).await
}

fn create(base_task: Option<&str>) -> serde_json::Value {
    let mut args = json!({"persona": "Linus", "title": "Work", "prompt": "Do it."});
    if let Some(base) = base_task {
        args["base_task"] = json!(base);
    }
    args
}

async fn row<
    T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
>(
    env: &Env,
    sql: &'static str,
) -> T {
    sqlx::query_scalar(sql)
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

async fn task_count(env: &Env) -> i64 {
    row(env, "SELECT count(*) FROM tasks").await
}

fn limits(env: &mut Env, max_depth: u32, max_open_subtasks: u32) {
    env.ctx.config = std::sync::Arc::new(ChannelConfig {
        tasks: TasksConfig {
            max_depth,
            max_open_subtasks,
        },
        ..ChannelConfig::default()
    });
}

async fn set_status(env: &Env, key: &str, status: &str) {
    sqlx::query("UPDATE tasks SET status = $2 WHERE key = $1")
        .bind(key)
        .bind(status)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

// Scenario "Reviewer based on the implementation".
#[tokio::test]
async fn create_task_with_a_base_task_stores_it() {
    let (env, rc) = setup(pool().await).await;
    let implementation = execute(&env.ctx, &rc, call("create_task", create(None))).await;
    assert!(!implementation.is_error, "{}", message(&implementation));

    let reviewer = execute(&env.ctx, &rc, call("create_task", create(Some("DF-2")))).await;

    assert!(!reviewer.is_error, "{}", message(&reviewer));
    let base: String = row(
        &env,
        "SELECT b.key FROM tasks t JOIN tasks b ON b.id = t.base_task_id WHERE t.key = 'DF-3'",
    )
    .await;
    assert_eq!(base, "DF-2");
}

// Scenario "Iteration on the feature branch".
#[tokio::test]
async fn a_task_may_base_a_subtask_on_itself() {
    let (env, rc) = setup(pool().await).await;

    let result = execute(&env.ctx, &rc, call("create_task", create(Some("DF-1")))).await;

    assert!(!result.is_error, "{}", message(&result));
    let (parent, base): (uuid::Uuid, uuid::Uuid) =
        sqlx::query_as("SELECT parent_id, base_task_id FROM tasks WHERE key = 'DF-2'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!((parent, base), (rc.task.id.as_uuid(), rc.task.id.as_uuid()));
}

// Scenarios "Base task already done" and "Unknown base task".
#[tokio::test]
async fn create_task_refuses_a_base_task_that_is_over_or_unknown() {
    let (env, rc) = setup(pool().await).await;
    for status in ["completed", "failed", "cancelled"] {
        execute(&env.ctx, &rc, call("create_task", create(None))).await;
        let key: String = row(&env, "SELECT max(key) FROM tasks").await;
        set_status(&env, &key, status).await;
        let before = task_count(&env).await;

        let result = execute(&env.ctx, &rc, call("create_task", create(Some(&key)))).await;

        assert!(
            result.is_error && message(&result).contains(&key),
            "{status}: {}",
            message(&result)
        );
        assert_eq!(task_count(&env).await, before, "{status}");
    }
    let result = execute(&env.ctx, &rc, call("create_task", create(Some("DF-999")))).await;
    assert!(result.is_error && message(&result).contains("DF-999"));
}

// Scenario "Too deep".
#[tokio::test]
async fn create_task_refuses_to_nest_deeper_than_the_limit() {
    let (mut env, rc) = setup(pool().await).await;
    limits(&mut env, 1, 10);
    let parent = env.second_task().await;
    sqlx::query("UPDATE tasks SET parent_id = $1 WHERE key = 'DF-1'")
        .bind(parent.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let before = task_count(&env).await;

    let result = execute(&env.ctx, &rc, call("create_task", create(None))).await;

    assert!(
        result.is_error && message(&result).contains("tasks.maxDepth"),
        "{}",
        message(&result)
    );
    assert_eq!(task_count(&env).await, before);
}

// Scenario "Too many open subtasks".
#[tokio::test]
async fn create_task_refuses_more_open_subtasks_than_the_limit() {
    let (mut env, rc) = setup(pool().await).await;
    limits(&mut env, 5, 2);
    for _ in 0..2 {
        let result = execute(&env.ctx, &rc, call("create_task", create(None))).await;
        assert!(!result.is_error, "{}", message(&result));
    }

    let result = execute(&env.ctx, &rc, call("create_task", create(None))).await;

    assert!(
        result.is_error && message(&result).contains("tasks.maxOpenSubtasks"),
        "{}",
        message(&result)
    );
    assert_eq!(task_count(&env).await, 3);
}

// Scenario "Finished subtasks don't count".
#[tokio::test]
async fn finished_subtasks_leave_room_for_more() {
    let (mut env, rc) = setup(pool().await).await;
    limits(&mut env, 5, 2);
    for _ in 0..2 {
        execute(&env.ctx, &rc, call("create_task", create(None))).await;
    }
    for key in ["DF-2", "DF-3"] {
        set_status(&env, key, "in_review").await;
    }

    let result = execute(&env.ctx, &rc, call("create_task", create(None))).await;

    assert!(!result.is_error, "{}", message(&result));
}

async fn completion_summary(env: &Env, rc: &df_orchestrator::run_context::RunContext) -> String {
    sqlx::query_scalar("SELECT completion_summary FROM runs WHERE id = $1")
        .bind(rc.run.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

// Scenario "Iteration not merged yet".
#[tokio::test]
async fn complete_task_names_the_open_subtasks_and_leaves_the_run_alone() {
    let (env, rc) = setup(pool().await).await;
    execute(&env.ctx, &rc, call("create_task", create(Some("DF-1")))).await;
    set_status(&env, "DF-2", "in_review").await;
    sqlx::query(
        "UPDATE tasks SET pr_number = 5, pr_url = 'https://forge/pulls/5' WHERE key = 'DF-2'",
    )
    .execute(&env.ctx.pool)
    .await
    .unwrap();

    let result = execute(
        &env.ctx,
        &rc,
        call("complete_task", json!({"summary": "All done."})),
    )
    .await;

    assert!(result.is_error, "{}", message(&result));
    let text = message(&result);
    assert!(
        text.contains("DF-2") && text.contains("https://forge/pulls/5"),
        "{text}"
    );
    assert!(!result.end_run_after_turn);
    assert_eq!(completion_summary(&env, &rc).await, "");
}

async fn task_id(env: &Env, key: &str) -> df_core::TaskId {
    let id = sqlx::query_scalar("SELECT id FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    df_core::TaskId::from_uuid(id)
}

// The run end wakes the parent before the draft PR of the iteration is opened.
#[tokio::test]
async fn complete_task_waits_for_an_iteration_whose_pr_is_about_to_open() {
    let (env, rc) = setup(pool().await).await;
    execute(&env.ctx, &rc, call("create_task", create(Some("DF-1")))).await;
    set_status(&env, "DF-2", "in_review").await;
    let payload = serde_json::to_value(df_db::tasks::PrSyncJob {
        task_id: task_id(&env, "DF-2").await,
        run_id: None,
        open: true,
        ready: None,
    })
    .unwrap();
    sqlx::query("INSERT INTO outbox (kind, payload) VALUES ('pr.sync', $1)")
        .bind(payload)
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    let summary = json!({"summary": "x"});
    let refused = execute(&env.ctx, &rc, call("complete_task", summary.clone())).await;
    assert!(
        refused.is_error && message(&refused).contains("DF-2"),
        "{}",
        message(&refused)
    );

    set_status(&env, "DF-2", "cancelled").await;
    let allowed = execute(&env.ctx, &rc, call("complete_task", summary)).await;
    assert!(!allowed.is_error, "{}", message(&allowed));
}

// Scenario "All subtasks finished".
#[tokio::test]
async fn complete_task_succeeds_when_every_subtask_is_finished() {
    let (env, rc) = setup(pool().await).await;
    execute(&env.ctx, &rc, call("create_task", create(None))).await;
    set_status(&env, "DF-2", "in_review").await;

    let result = execute(
        &env.ctx,
        &rc,
        call("complete_task", json!({"summary": "All done."})),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    assert!(result.end_run_after_turn);
    assert_eq!(completion_summary(&env, &rc).await, "All done.");
}

#[tokio::test]
async fn complete_task_waits_while_a_subtask_is_running() {
    let (env, rc) = setup(pool().await).await;
    execute(&env.ctx, &rc, call("create_task", create(None))).await;

    let result = execute(
        &env.ctx,
        &rc,
        call("complete_task", json!({"summary": "x"})),
    )
    .await;

    assert!(result.is_error && message(&result).contains("DF-2"));
}
