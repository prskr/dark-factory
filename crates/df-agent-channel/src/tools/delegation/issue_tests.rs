//! Tests of `create_task` with `issue` (spec `factory-tools`, "`create_task` links an issue").

use df_core::testing::ForgeCall;
use df_core::{IntegrationError, Issue, IssueRef, IssueState, RepoRef};
use df_db::runs::get_run;
use df_orchestrator::run_context::load_run_context;
use serde_json::json;

use crate::testing::Env;
use crate::tools::chat_tests::chat_run;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};
use df_orchestrator::run_context::RunContext;

fn issue(number: u64, pull_request: bool) -> Issue {
    Issue {
        id: IssueRef {
            repo: RepoRef {
                owner: "peter".into(),
                name: "dark-factory".into(),
            },
            number,
        },
        url: format!("https://git.example.org/peter/dark-factory/issues/{number}"),
        title: "Flaky".into(),
        body: String::new(),
        state: IssueState::Open,
        labels: Vec::new(),
        pull_request,
    }
}

fn args(issue: Option<u64>) -> serde_json::Value {
    let mut args = json!({"persona": "Linus", "title": "Fix it", "prompt": "Do it."});
    if let Some(issue) = issue {
        args["issue"] = json!(issue);
    }
    args
}

async fn children(env: &Env, rc: &RunContext) -> Vec<Option<i64>> {
    sqlx::query_scalar("SELECT issue_number FROM tasks WHERE parent_id = $1 ORDER BY key")
        .bind(rc.task.id.as_uuid())
        .fetch_all(&env.ctx.pool)
        .await
        .unwrap()
}

fn get_issue_calls(env: &Env) -> usize {
    env.forge
        .calls()
        .iter()
        .filter(|c| matches!(c, ForgeCall::GetIssue { .. }))
        .count()
}

#[tokio::test]
async fn an_issue_of_the_repository_links_the_new_task_from_a_chat() {
    let (env, rc) = chat_run().await;
    env.forge.insert_issue(rc.project.id, issue(42, false));

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(42)))).await;

    assert!(!result.is_error, "{}", message(&result));
    assert_eq!(children(&env, &rc).await, vec![Some(42)]);
}

#[tokio::test]
async fn the_child_of_a_linked_issue_comments_on_that_issue() {
    let (env, rc) = chat_run().await;
    env.forge.insert_issue(rc.project.id, issue(42, false));
    let created = execute(&env.ctx, &rc, call("create_task", args(Some(42)))).await;
    assert!(!created.is_error, "{}", message(&created));
    let child: uuid::Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE parent_id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    let run = env
        .insert_run(df_core::TaskId::from_uuid(child), 1, "running")
        .await;
    let child_rc = load_run_context(
        &env.ctx.pool,
        get_run(&env.ctx.pool, run).await.unwrap().unwrap(),
    )
    .await
    .unwrap();

    let result = execute(
        &env.ctx,
        &child_rc,
        call("comment_on_issue", json!({"body": "Findings."})),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    assert!(
        env.forge.calls().iter().any(|c| matches!(
            c,
            ForgeCall::AddIssueComment { number: 42, body, .. } if body == "Findings."
        )),
        "{:?}",
        env.forge.calls()
    );
}

#[tokio::test]
async fn an_issue_links_the_new_task_from_a_regular_task_too() {
    let (env, rc) = setup(df_test_postgres::migrated_pool(&df_db::MIGRATOR).await).await;
    env.forge.insert_issue(rc.project.id, issue(7, false));

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(7)))).await;

    assert!(!result.is_error, "{}", message(&result));
    assert_eq!(children(&env, &rc).await, vec![Some(7)]);
}

#[tokio::test]
async fn a_pull_request_number_is_refused_and_no_task_is_created() {
    let (env, rc) = chat_run().await;
    env.forge.insert_issue(rc.project.id, issue(40, true));

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(40)))).await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("#40 is a pull request"),
        "{}",
        message(&result)
    );
    assert!(children(&env, &rc).await.is_empty());
}

#[tokio::test]
async fn an_unknown_issue_is_refused_and_no_task_is_created() {
    let (env, rc) = chat_run().await;

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(9999)))).await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("issue #9999 does not exist"),
        "{}",
        message(&result)
    );
    assert!(children(&env, &rc).await.is_empty());
}

#[tokio::test]
async fn a_forge_error_is_reported_and_no_task_is_created() {
    let (env, rc) = chat_run().await;
    env.forge
        .push_get_issue_result(Err(IntegrationError::Transport("refused".into())));

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(42)))).await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("the forge request failed"),
        "{}",
        message(&result)
    );
    assert!(children(&env, &rc).await.is_empty());
}

#[tokio::test]
async fn without_an_issue_the_forge_is_not_asked_and_the_task_has_none() {
    let (env, rc) = chat_run().await;

    let result = execute(&env.ctx, &rc, call("create_task", args(None))).await;

    assert!(!result.is_error, "{}", message(&result));
    assert_eq!(get_issue_calls(&env), 0);
    assert_eq!(children(&env, &rc).await, vec![None]);
}

#[tokio::test]
async fn an_issue_written_as_a_float_is_refused_and_no_unlinked_task_is_created() {
    let (env, rc) = chat_run().await;
    env.forge.insert_issue(rc.project.id, issue(42, false));
    let mut arguments = args(None);
    arguments["issue"] = json!(42.0);

    let result = execute(&env.ctx, &rc, call("create_task", arguments)).await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("positive whole number"),
        "{}",
        message(&result)
    );
    assert!(children(&env, &rc).await.is_empty());
}

#[tokio::test]
async fn an_issue_number_below_one_is_rejected_by_the_schema() {
    let (env, rc) = chat_run().await;

    let result = execute(&env.ctx, &rc, call("create_task", args(Some(0)))).await;

    assert!(result.is_error);
    assert_eq!(get_issue_calls(&env), 0);
}
