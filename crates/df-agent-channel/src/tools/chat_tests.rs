//! Tests of the tools of a chat run (`specs/chats/spec.md`, "Tools of a chat run").

use df_core::TaskKind;
use df_db::runs::get_run;
use df_orchestrator::run_context::{RunContext, load_run_context};
use serde_json::json;

use crate::testing::Env;
use crate::tools::execute;
use crate::tools::tests::{call, message};

/// A run of a chat that is `running`.
pub(super) async fn chat_run() -> (Env, RunContext) {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let chat = env.chat().await;
    let run = env.insert_run(chat.id, 1, "running").await;
    sqlx::query("UPDATE tasks SET status = 'running' WHERE id = $1")
        .bind(chat.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
        .await
        .unwrap();
    (env, rc)
}

async fn stored_calls(env: &Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM tool_calls")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_chat_run_that_delegates_creates_a_task_with_the_chat_as_its_parent() {
    let (env, rc) = chat_run().await;

    let created = execute(
        &env.ctx,
        &rc,
        call(
            "create_task",
            json!({"persona": "linus", "title": "Implement", "prompt": "Do it."}),
        ),
    )
    .await;

    assert!(!created.is_error, "{}", message(&created));
    let (kind, parent): (String, Option<uuid::Uuid>) =
        sqlx::query_as("SELECT kind, parent_id FROM tasks WHERE key = 'DF-3'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!(kind, TaskKind::Task.as_str());
    assert_eq!(parent, Some(rc.task.id.as_uuid()));
}

#[tokio::test]
async fn a_chat_run_cannot_complete_the_chat_and_nothing_changes() {
    let (env, rc) = chat_run().await;

    let result = execute(
        &env.ctx,
        &rc,
        call("complete_task", json!({"summary": "Done."})),
    )
    .await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("not available in a chat"),
        "{}",
        message(&result)
    );
    assert!(!result.end_run_after_turn);
    assert_eq!(stored_calls(&env).await, 0);
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn a_chat_run_is_refused_every_tool_it_is_not_offered() {
    let (env, rc) = chat_run().await;

    for tool in [
        "update_pull_request",
        "comment_on_issue",
        "read_pull_request_feedback",
        "comment_on_pull_request",
        "merge_subtask_pr",
    ] {
        let result = execute(&env.ctx, &rc, call(tool, json!({}))).await;
        assert!(result.is_error, "{tool}");
        assert!(
            message(&result).contains("not available in a chat"),
            "{tool}"
        );
    }
    assert_eq!(stored_calls(&env).await, 0);
}

#[tokio::test]
async fn a_chat_run_can_ask_a_question_which_is_stored_on_the_chat() {
    let (env, rc) = chat_run().await;
    let options = json!([{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]);

    let result = execute(
        &env.ctx,
        &rc,
        call(
            "ask_question",
            json!({"text": "Which?", "kind": "single_choice", "options": options}),
        ),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    assert!(result.await_input_after_turn);
    let stored = df_db::requests::list_for_task(&env.ctx.pool, rc.task.id)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].question().kind,
        df_core::QuestionKind::SingleChoice
    );
    assert_eq!(stored[0].state, df_db::requests::RequestState::Pending);
}
