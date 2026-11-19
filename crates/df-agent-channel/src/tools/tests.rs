use super::*;
use crate::testing::Env;
use df_core::testing::ForgeCall;
use df_core::{Comment, ForgeClient, RateLimit};
use df_db::runs::get_run;
use df_orchestrator::run_context::load_run_context;
use sqlx::PgPool;

pub(super) async fn setup(pool: PgPool) -> (Env, RunContext) {
    let env = Env::new(pool.clone()).await;
    let run = env.insert_run(env.task.id, 1, "running").await;
    let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
        .await
        .unwrap();
    (env, rc)
}

pub(super) fn call(tool: &str, args: Value) -> ToolCallRequest {
    ToolCallRequest {
        call_id: uuid::Uuid::now_v7().to_string(),
        tool_name: tool.into(),
        arguments_json: args.to_string(),
        attachment: Vec::new().into(),
    }
}

pub(super) fn message(result: &ToolCallResult) -> String {
    match &result.content[0].kind {
        Some(content_block::Kind::Text(t)) => t.text.clone(),
        other => panic!("unexpected content {other:?}"),
    }
}

#[test]
fn forge_error_for_a_refused_request_shows_the_forge_message_and_the_access_hint() {
    let refused = IntegrationError::Unauthorized(
        "GET /api/v1/repos/o/r/pulls/7/reviews: token does not have required scope".into(),
    );
    let outcome = forge_error(&refused);
    assert!(outcome.is_error);
    assert!(
        outcome
            .text
            .contains("GET /api/v1/repos/o/r/pulls/7/reviews: token does not have required scope"),
        "{}",
        outcome.text
    );
    assert!(
        outcome.text.contains("bot account's access"),
        "{}",
        outcome.text
    );
}

#[test]
fn embedded_schemas_compile() {
    assert_eq!(VALIDATORS.len(), 19);
}

#[tokio::test]
async fn unknown_tool_executes_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(&env.ctx, &rc, call("delete_project", serde_json::json!({}))).await;
    assert!(res.is_error);
    assert!(message(&res).contains("delete_project"));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tool_calls")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn schema_violation_names_the_missing_field() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    let res = execute(
        &env.ctx,
        &rc,
        call("report_progress", serde_json::json!({})),
    )
    .await;
    assert!(res.is_error);
    assert!(message(&res).contains("summary"), "{}", message(&res));
}

#[tokio::test]
async fn retry_with_the_same_call_id_executes_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let request = call("comment_on_issue", serde_json::json!({"body": "findings"}));

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let second = execute(&env.ctx, &rc, request).await;

    assert!(!first.is_error);
    assert_eq!(first, second);
    let comments = env
        .forge
        .calls()
        .into_iter()
        .filter(|c| matches!(c, ForgeCall::AddIssueComment { number: 42, .. }))
        .count();
    assert_eq!(comments, 1);
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'tool_call.execute'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audited, 1);
}

#[tokio::test]
async fn a_claimed_call_that_is_still_running_is_not_executed_again() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
    // Another attempt claimed the call and has not stored a result yet.
    sqlx::query("INSERT INTO tool_calls (call_id, run_id, tool_name, arguments) VALUES ($1, $2, 'comment_on_issue', '{}')")
            .bind(request.call_id.parse::<uuid::Uuid>().unwrap())
            .bind(rc.run.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

    let res = execute(&env.ctx, &rc, request).await;

    assert!(res.is_error);
    assert!(message(&res).contains("in progress"), "{}", message(&res));
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn an_abandoned_claim_is_closed_with_an_error_and_never_executed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
    // The claiming replica died long ago without storing a result.
    sqlx::query("INSERT INTO tool_calls (call_id, run_id, tool_name, arguments, created_at) VALUES ($1, $2, 'comment_on_issue', '{}', now() - interval '1 hour')")
            .bind(request.call_id.parse::<uuid::Uuid>().unwrap())
            .bind(rc.run.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let second = execute(&env.ctx, &rc, request).await;

    assert!(first.is_error);
    assert!(
        message(&first).contains("did not finish"),
        "{}",
        message(&first)
    );
    assert_eq!(
        first, second,
        "the closed claim keeps answering the same way"
    );
    assert!(
        env.forge.calls().is_empty(),
        "at most once: never executed after the claim"
    );
}

#[tokio::test]
async fn the_forge_call_does_not_hold_a_database_transaction() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
    // While the forge call runs, the claim is committed: visible to other connections.
    let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
    let forge = ObservingForge {
        pool: pool.clone(),
        call_id: request.call_id.clone(),
        seen: observed.clone(),
    };
    let mut ctx = env.ctx.clone();
    ctx.orch.forge = std::sync::Arc::new(forge);

    execute(&ctx, &rc, request).await;

    assert_eq!(
        *observed.lock().unwrap(),
        Some(1),
        "claim row visible during the forge call"
    );
}

/// Records how many `tool_calls` rows with the call id other connections see during a call.
struct ObservingForge {
    pool: PgPool,
    call_id: String,
    seen: std::sync::Arc<std::sync::Mutex<Option<i64>>>,
}

#[async_trait::async_trait]
impl ForgeClient for ObservingForge {
    async fn create_pull_request(
        &self,
        _: &df_core::ProjectId,
        _: df_core::NewPullRequest,
    ) -> Result<df_core::PullRequest, IntegrationError> {
        panic!("not used")
    }
    async fn find_open_pull_request(
        &self,
        _: &df_core::ProjectId,
        _: &str,
        _: &str,
    ) -> Result<Option<df_core::PullRequest>, IntegrationError> {
        panic!("not used")
    }
    async fn list_open_pull_requests(
        &self,
        _: &df_core::ProjectId,
        _: &str,
    ) -> Result<Vec<df_core::PullRequest>, IntegrationError> {
        panic!("not used")
    }
    async fn list_pull_requests(
        &self,
        _: &df_core::ProjectId,
        _: &df_core::PullRequestFilter,
    ) -> Result<Vec<df_core::ListedPullRequest>, IntegrationError> {
        panic!("not used")
    }
    async fn get_pull_request(
        &self,
        _: &df_core::ProjectId,
        _: u64,
    ) -> Result<df_core::PullRequest, IntegrationError> {
        panic!("not used")
    }
    async fn read_ci_status(
        &self,
        _: &df_core::ProjectId,
        _: &str,
    ) -> Result<df_core::CiStatus, IntegrationError> {
        panic!("not used")
    }
    async fn read_ci_job_log(
        &self,
        _: &df_core::ProjectId,
        _: u64,
    ) -> Result<df_core::CiJobLog, IntegrationError> {
        panic!("not used")
    }
    async fn update_pull_request(
        &self,
        _: &df_core::ProjectId,
        _: u64,
        _: PullRequestUpdate,
    ) -> Result<df_core::PullRequest, IntegrationError> {
        panic!("not used")
    }
    async fn merge_pull_request(
        &self,
        _: &df_core::ProjectId,
        _: u64,
        _: &str,
    ) -> Result<(), IntegrationError> {
        panic!("not used")
    }
    async fn create_branch(
        &self,
        _: &df_core::ProjectId,
        _: &str,
        _: &str,
    ) -> Result<(), IntegrationError> {
        panic!("not used")
    }
    async fn list_pull_request_feedback(
        &self,
        _: &df_core::ProjectId,
        _: u64,
        _: Option<&str>,
    ) -> Result<df_core::Feedback, IntegrationError> {
        panic!("not used")
    }
    async fn reply_to_review_thread(
        &self,
        _: &df_core::ProjectId,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<Comment, IntegrationError> {
        panic!("not used")
    }
    async fn get_issue(
        &self,
        _: &df_core::ProjectId,
        _: u64,
    ) -> Result<df_core::Issue, IntegrationError> {
        panic!("not used")
    }
    async fn add_issue_comment(
        &self,
        _: &df_core::ProjectId,
        _: u64,
        _: &str,
    ) -> Result<Comment, IntegrationError> {
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE call_id = $1")
            .bind(self.call_id.parse::<uuid::Uuid>().unwrap())
            .fetch_one(&self.pool)
            .await
            .unwrap();
        *self.seen.lock().unwrap() = Some(rows);
        Ok(Comment {
            id: "1".into(),
            url: "u".into(),
        })
    }
    async fn whoami(&self, _: &df_core::ProjectId) -> Result<String, IntegrationError> {
        panic!("not used")
    }
    async fn author_role(
        &self,
        _: &df_core::ProjectId,
        _: &str,
    ) -> Result<df_core::AuthorRole, IntegrationError> {
        panic!("not used")
    }
}

#[tokio::test]
async fn update_pull_request_before_the_pr_exists_stores_the_values() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(
        &env.ctx,
        &rc,
        call(
            "update_pull_request",
            serde_json::json!({"title": "New title"}),
        ),
    )
    .await;
    assert!(!res.is_error);
    let title: Option<String> = sqlx::query_scalar("SELECT pr_title FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title.as_deref(), Some("New title"));
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn update_pull_request_updates_an_existing_pr() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let pr = env
        .forge
        .create_pull_request(
            &rc.project.id,
            df_core::NewPullRequest {
                repo: df_orchestrator::pr::repo_ref(&rc.project.repo_url).unwrap(),
                head: "factory/task/DF-1".into(),
                base: "main".into(),
                title: "t".into(),
                body: "b".into(),
                draft: true,
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET pr_number = $2 WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .bind(i64::try_from(pr.id.number).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let res = execute(
        &env.ctx,
        &rc,
        call(
            "update_pull_request",
            serde_json::json!({"body": "Details"}),
        ),
    )
    .await;
    assert!(!res.is_error, "{}", message(&res));
    assert!(env.forge.calls().iter().any(|c| matches!(c,
            ForgeCall::UpdatePullRequest { number: 1, changes, .. } if changes.body.as_deref() == Some("Details"))));
}

#[tokio::test]
async fn update_pull_request_fails_for_an_attached_pr_and_changes_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    sqlx::query("UPDATE tasks SET pr_number = 40, pr_attached = true WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let res = execute(
        &env.ctx,
        &rc,
        call("update_pull_request", serde_json::json!({"title": "New"})),
    )
    .await;
    assert!(res.is_error, "{}", message(&res));
    let title: Option<String> = sqlx::query_scalar("SELECT pr_title FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, None);
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn comment_without_linked_issue_is_an_error() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, mut rc) = setup(pool).await;
    rc.task.issue_number = None;
    let res = execute(
        &env.ctx,
        &rc,
        call("comment_on_issue", serde_json::json!({"body": "x"})),
    )
    .await;
    assert!(res.is_error);
    assert!(message(&res).contains("this task has no linked issue"));
}

#[tokio::test]
async fn report_progress_sets_the_status_line() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(
        &env.ctx,
        &rc,
        call(
            "report_progress",
            serde_json::json!({"summary": "tests pass"}),
        ),
    )
    .await;
    assert!(!res.is_error);
    let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(line, "tests pass");
}

async fn title_of(pool: &PgPool, rc: &RunContext) -> (String, String) {
    sqlx::query_as("SELECT title, title_source FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn make_placeholder(pool: &PgPool, rc: &RunContext) {
    sqlx::query("UPDATE tasks SET title_source = 'placeholder' WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn set_task_title_sets_a_sanitized_agent_title_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    make_placeholder(&pool, &rc).await;
    let mut events = env.ctx.notifier.subscribe();

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "set_task_title",
            serde_json::json!({"title": "Stop duplicate\nnightly backups"}),
        ),
    )
    .await;

    assert!(!res.is_error);
    assert_eq!(
        title_of(&pool, &rc).await,
        ("Stop duplicate nightly backups".into(), "agent".into())
    );
    let changed = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        changed,
        Notification::TaskChanged {
            task_id: rc.task.id
        }
    );

    let again = execute(
        &env.ctx,
        &rc,
        call("set_task_title", serde_json::json!({"title": "Other"})),
    )
    .await;
    assert!(!again.is_error);
    assert!(message(&again).contains("kept"));
    assert_eq!(
        title_of(&pool, &rc).await.0,
        "Stop duplicate nightly backups"
    );
}

#[tokio::test]
async fn set_task_title_keeps_the_title_of_a_person() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "set_task_title",
            serde_json::json!({"title": "Agent title"}),
        ),
    )
    .await;

    assert!(!res.is_error);
    assert!(message(&res).contains("kept"));
    assert_eq!(
        title_of(&pool, &rc).await,
        ("Fix login".into(), "user".into())
    );
}

#[tokio::test]
async fn set_task_title_rejects_a_blank_title() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    make_placeholder(&pool, &rc).await;

    let res = execute(
        &env.ctx,
        &rc,
        call("set_task_title", serde_json::json!({"title": " \n "})),
    )
    .await;

    assert!(res.is_error);
    assert_eq!(
        title_of(&pool, &rc).await,
        ("Fix login".into(), "placeholder".into())
    );
}

#[tokio::test]
async fn complete_task_stores_the_summary_and_ends_the_run_after_the_turn() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(
        &env.ctx,
        &rc,
        call("complete_task", serde_json::json!({"summary": "Done."})),
    )
    .await;
    assert!(res.end_run_after_turn);
    let stored: String = sqlx::query_scalar("SELECT completion_summary FROM runs WHERE id = $1")
        .bind(rc.run.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "Done.");
    let on_task: String = sqlx::query_scalar("SELECT completion_summary FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(on_task, "", "the task column is unused");
}

#[tokio::test]
async fn forge_errors_are_tool_errors_with_actionable_text() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_add_issue_comment_result(Err(IntegrationError::RateLimited(RateLimit {
            retry_after_seconds: Some(30),
            message: "slow down".into(),
        })));
    let res = execute(
        &env.ctx,
        &rc,
        call("comment_on_issue", serde_json::json!({"body": "x"})),
    )
    .await;
    assert!(res.is_error);
    assert!(message(&res).contains("retry in 30 seconds"));

    env.forge.push_add_issue_comment_result(Ok(Comment {
        id: "1".into(),
        url: "u".into(),
    }));
    let ok = execute(
        &env.ctx,
        &rc,
        call("comment_on_issue", serde_json::json!({"body": "x"})),
    )
    .await;
    assert!(
        !ok.is_error,
        "the run's tools keep working after a forge error"
    );
}

async fn pending_requests(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM agent_requests WHERE state = 'pending'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn non_blocking_question_returns_its_id_and_does_not_pause_the_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(
            &env.ctx,
            &rc,
            call(
                "ask_question",
                serde_json::json!({"text": "Should I also update the docs?", "suggested_answers": ["yes", "no"]}),
            ),
        )
        .await;
    assert!(!res.is_error, "{}", message(&res));
    assert!(!res.await_input_after_turn && !res.end_run_after_turn);
    let structured: Value = serde_json::from_str(&res.structured_content_json).unwrap();
    assert_eq!(structured["state"], "pending");
    assert!(structured["id"].is_string());
    assert_eq!(pending_requests(&pool).await, 1);
}

#[tokio::test]
async fn blocking_question_awaits_input_after_the_turn() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let res = execute(
        &env.ctx,
        &rc,
        call(
            "ask_question",
            serde_json::json!({"text": "Which?", "blocking": true}),
        ),
    )
    .await;
    assert!(res.await_input_after_turn);
    assert!(!res.end_run_after_turn);
}

#[tokio::test]
async fn too_many_questions_is_an_error_the_agent_can_read() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    for i in 0..21 {
        let res = execute(
            &env.ctx,
            &rc,
            call("ask_question", serde_json::json!({"text": format!("q{i}")})),
        )
        .await;
        assert_eq!(res.is_error, i == 20, "{}", message(&res));
        if res.is_error {
            assert!(message(&res).contains("too many pending requests"));
        }
    }
}

#[tokio::test]
async fn completing_the_task_closes_its_pending_questions() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    execute(
        &env.ctx,
        &rc,
        call("ask_question", serde_json::json!({"text": "Which?"})),
    )
    .await;
    execute(
        &env.ctx,
        &rc,
        call("complete_task", serde_json::json!({"summary": "Done."})),
    )
    .await;
    assert_eq!(pending_requests(&pool).await, 0);
    let reason: String = sqlx::query_scalar("SELECT close_reason FROM agent_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reason, "task-completed");
}

fn network_access(host: &str, blocking: bool) -> ToolCallRequest {
    call(
        "request_approval",
        serde_json::json!({
            "type": "network-access",
            "payload": {"host": host},
            "reason": "pip install needs PyPI",
            "blocking": blocking,
        }),
    )
}

#[tokio::test]
async fn request_approval_raises_a_pending_network_access_request() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;

    let res = execute(&env.ctx, &rc, network_access("PyPI.org", false)).await;

    assert!(!res.is_error, "{}", message(&res));
    assert!(!res.await_input_after_turn && !res.end_run_after_turn);
    let structured: Value = serde_json::from_str(&res.structured_content_json).unwrap();
    assert_eq!(structured["state"], "pending");
    let id: uuid::Uuid = structured["id"].as_str().unwrap().parse().unwrap();
    let (payload, reason, run): (Value, String, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT payload, reason, run_id FROM agent_requests
             WHERE id = $1 AND approval_type = 'network-access' AND state = 'pending'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        payload,
        serde_json::json!({ "host": "pypi.org", "port": 443 })
    );
    assert_eq!(reason, "pip install needs PyPI");
    assert_eq!(run, Some(rc.run.id.as_uuid()));
}

#[tokio::test]
async fn blocking_request_approval_awaits_input_after_the_turn() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;

    let res = execute(&env.ctx, &rc, network_access("pypi.org", true)).await;

    assert!(!res.is_error, "{}", message(&res));
    assert!(res.await_input_after_turn);
}

#[tokio::test]
async fn request_approval_with_an_invalid_payload_is_an_error() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;

    let res = execute(&env.ctx, &rc, network_access("10.0.0.5", false)).await;

    assert!(res.is_error);
    assert!(
        message(&res).contains("invalid request"),
        "{}",
        message(&res)
    );
    assert_eq!(pending_requests(&pool).await, 0);
}

#[tokio::test]
async fn request_approval_for_an_allowed_host_stores_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let (peter, roles) = (
        Principal::human(df_core::UserId::new(), "Peter"),
        Vec::new(),
    );
    let actor = df_db::catalog::Actor {
        principal: &peter,
        roles: &roles,
    };
    let entry = df_db::egress::EntryKind::HttpHost {
        host: "pypi.org".into(),
        port: 443,
    };
    df_db::egress::add_entry(&pool, actor, Some("DF"), &entry)
        .await
        .unwrap();

    let res = execute(&env.ctx, &rc, network_access("PyPI.org", true)).await;

    assert!(!res.is_error, "{}", message(&res));
    assert!(
        message(&res).contains("already allowed"),
        "{}",
        message(&res)
    );
    assert!(!res.await_input_after_turn);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn request_approval_for_a_denied_host_returns_the_policy_error_and_stores_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    let (peter, roles) = (
        Principal::human(df_core::UserId::new(), "Peter"),
        Vec::new(),
    );
    let actor = df_db::catalog::Actor {
        principal: &peter,
        roles: &roles,
    };
    let mut tx = pool.begin().await.unwrap();
    df_db::egress::create_deny_rule(&mut tx, "*.datadoghq.com", actor, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let res = execute(
        &env.ctx,
        &rc,
        network_access("http-intake.logs.us5.datadoghq.com", true),
    )
    .await;

    assert!(res.is_error);
    assert_eq!(
        message(&res),
        "http-intake.logs.us5.datadoghq.com is denied by instance policy (deny rule \
             *.datadoghq.com); do not request it again"
    );
    assert!(!res.await_input_after_turn);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}
