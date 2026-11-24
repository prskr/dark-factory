//! Tests of `list_issues` and `create_issue` (spec `factory-tools`).

use df_core::testing::ForgeCall;
use df_core::{
    IntegrationError, Issue, IssuePage, IssueRef, IssueState, IssueStateFilter, RateLimit, RepoRef,
};
use df_db::requests::list_for_task;
use serde_json::{Value, json};

use crate::tools::chat_tests::chat_run;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

fn issue(number: u64, title: &str, body: &str, labels: &[&str]) -> Issue {
    Issue {
        id: IssueRef {
            repo: RepoRef {
                owner: "peter".into(),
                name: "dark-factory".into(),
            },
            number,
        },
        url: format!("https://git.example.org/peter/dark-factory/issues/{number}"),
        title: title.into(),
        body: body.into(),
        state: IssueState::Open,
        labels: labels.iter().map(ToString::to_string).collect(),
        pull_request: false,
    }
}

fn structured(result: &df_proto::agent::v1::ToolCallResult) -> Value {
    serde_json::from_str(&result.structured_content_json).unwrap()
}

#[tokio::test]
async fn list_issues_lists_number_state_title_labels_url_and_the_start_of_the_body() {
    let (env, rc) = chat_run().await;
    let long_body = format!("first line\nsecond {}", "x".repeat(300));
    env.forge.insert_issue(
        rc.project.id,
        issue(7, "Flaky login test", &long_body, &["bug", "ui"]),
    );
    env.forge
        .insert_issue(rc.project.id, issue(3, "Docs", "short", &[]));

    let result = execute(&env.ctx, &rc, call("list_issues", json!({}))).await;

    assert!(!result.is_error, "{}", message(&result));
    let text = message(&result);
    assert!(text.contains("#7"), "{text}");
    assert!(text.contains("open"), "{text}");
    assert!(text.contains("Flaky login test"), "{text}");
    assert!(text.contains("bug, ui"), "{text}");
    assert!(
        text.contains("https://git.example.org/peter/dark-factory/issues/7"),
        "{text}"
    );
    assert!(
        text.contains("first line second xxx"),
        "the body starts: {text}"
    );
    assert!(
        !text.contains(&"x".repeat(250)),
        "only the first 200 characters of the body: {text}"
    );
    assert!(text.find("#7") < text.find("#3"), "newest first: {text}");
    let data = structured(&result);
    assert_eq!(data["truncated"], json!(false));
    assert_eq!(data["next_cursor"], Value::Null);
    assert_eq!(data["issues"][0]["number"], json!(7));
    assert_eq!(
        data["issues"][0]["body"].as_str().unwrap().chars().count(),
        200
    );
}

#[tokio::test]
async fn list_issues_passes_the_filter_and_defaults_to_open_issues_with_a_limit_of_30() {
    let (env, rc) = chat_run().await;

    execute(&env.ctx, &rc, call("list_issues", json!({}))).await;
    execute(
        &env.ctx,
        &rc,
        call(
            "list_issues",
            json!({"state": "closed", "labels": ["bug"], "query": "timeout", "limit": 5, "cursor": "c1"}),
        ),
    )
    .await;

    let calls: Vec<_> = env
        .forge
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            ForgeCall::ListIssues {
                filter,
                limit,
                cursor,
                ..
            } => Some((filter, limit, cursor)),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0.state, IssueStateFilter::Open);
    assert_eq!(calls[0].1, 30);
    assert_eq!(calls[0].2, None);
    assert_eq!(calls[1].0.state, IssueStateFilter::Closed);
    assert_eq!(calls[1].0.labels, vec!["bug".to_string()]);
    assert_eq!(calls[1].0.query.as_deref(), Some("timeout"));
    assert_eq!((calls[1].1, calls[1].2.as_deref()), (5, Some("c1")));
}

#[tokio::test]
async fn list_issues_reports_the_next_cursor_and_truncation() {
    let (env, rc) = chat_run().await;
    env.forge.push_list_issues_result(Ok(IssuePage {
        issues: vec![issue(9, "T", "", &[])],
        next_cursor: Some("2:0".into()),
        truncated: true,
    }));

    let result = execute(&env.ctx, &rc, call("list_issues", json!({"limit": 1}))).await;

    let text = message(&result);
    assert!(text.contains("next_cursor: 2:0"), "{text}");
    assert!(text.contains("truncated: true"), "{text}");
    let data = structured(&result);
    assert_eq!(data["next_cursor"], json!("2:0"));
    assert_eq!(data["truncated"], json!(true));
}

#[tokio::test]
async fn list_issues_turns_a_forge_error_into_an_error_result() {
    let (env, rc) = chat_run().await;
    env.forge
        .push_list_issues_result(Err(IntegrationError::RateLimited(RateLimit {
            retry_after_seconds: Some(12),
            message: "slow".into(),
        })));

    let result = execute(&env.ctx, &rc, call("list_issues", json!({}))).await;

    assert!(result.is_error);
    assert!(
        message(&result).contains("retry in 12 seconds"),
        "{}",
        message(&result)
    );
}

#[tokio::test]
async fn list_issues_rejects_a_limit_over_100() {
    let (env, rc) = chat_run().await;

    let result = execute(&env.ctx, &rc, call("list_issues", json!({"limit": 101}))).await;

    assert!(result.is_error);
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn a_task_run_is_refused_the_issue_tools_before_validation_and_without_the_forge() {
    let (env, rc) = setup(df_test_postgres::migrated_pool(&df_db::MIGRATOR).await).await;

    for (tool, args) in [
        ("list_issues", json!({"limit": 0})),
        ("create_issue", json!({"title": ""})),
        ("create_issue", json!({"title": "T", "body": "B"})),
    ] {
        let result = execute(&env.ctx, &rc, call(tool, args)).await;

        assert!(result.is_error, "{tool}");
        assert!(
            message(&result).contains(&format!("`{tool}` is only available in a chat")),
            "{}",
            message(&result)
        );
    }
    assert!(env.forge.calls().is_empty());
    assert!(
        list_for_task(&env.ctx.pool, rc.task.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn create_issue_raises_a_pending_request_with_exactly_the_given_text_and_calls_no_forge() {
    let (env, rc) = chat_run().await;

    let result = execute(
        &env.ctx,
        &rc,
        call(
            "create_issue",
            json!({"title": "Flaky login test", "body": "It fails on CI.\n\n`code`", "labels": ["bug"]}),
        ),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    let requests = list_for_task(&env.ctx.pool, rc.task.id).await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.approval_type.as_deref(), Some("create-issue"));
    assert!(request.state.is_pending());
    assert_eq!(
        request.payload,
        json!({"title": "Flaky login test", "body": "It fails on CI.\n\n`code`", "labels": ["bug"]})
    );
    assert!(message(&result).contains(&request.id.to_string()));
    assert_eq!(structured(&result)["id"], json!(request.id));
    assert!(
        env.forge.calls().is_empty(),
        "nothing is posted before approval"
    );
}

#[tokio::test]
async fn create_issue_with_invalid_arguments_stores_nothing() {
    let (env, rc) = chat_run().await;

    for args in [
        json!({"title": "", "body": "b"}),
        json!({"title": "   ", "body": "b"}),
        json!({"title": "two\nlines", "body": "b"}),
        json!({"title": "t"}),
        json!({"title": "t", "body": "b", "labels": "bug"}),
    ] {
        let result = execute(&env.ctx, &rc, call("create_issue", args.clone())).await;

        assert!(result.is_error, "{args}");
    }
    assert!(
        list_for_task(&env.ctx.pool, rc.task.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn identical_create_issue_calls_merge_and_a_different_body_is_a_separate_request() {
    let (env, rc) = chat_run().await;
    let args = json!({"title": "Flaky", "body": "one", "labels": ["bug"]});

    execute(&env.ctx, &rc, call("create_issue", args.clone())).await;
    execute(&env.ctx, &rc, call("create_issue", args)).await;
    execute(
        &env.ctx,
        &rc,
        call(
            "create_issue",
            json!({"title": "Flaky", "body": "two", "labels": ["bug"]}),
        ),
    )
    .await;

    let requests = list_for_task(&env.ctx.pool, rc.task.id).await.unwrap();
    let bodies: Vec<_> = requests.iter().map(|r| r.payload["body"].clone()).collect();
    assert_eq!(bodies, vec![json!("one"), json!("two")]);
}

#[tokio::test]
async fn create_issue_leaves_the_linked_issue_of_the_chat_unchanged() {
    let (env, rc) = chat_run().await;
    let before = rc.task.issue_number;

    execute(
        &env.ctx,
        &rc,
        call("create_issue", json!({"title": "T", "body": "b"})),
    )
    .await;

    let after: Option<i64> = sqlx::query_scalar("SELECT issue_number FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(after, before);
}
