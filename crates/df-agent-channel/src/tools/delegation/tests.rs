//! Tests of `create_task`, `list_personas` and `send_message`.

use df_core::{DependencyKind, ForgeClient as _, Principal};
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

/// Opens a PR from `head` into `base` on the fake forge.
async fn open_pr(
    env: &crate::testing::Env,
    rc: &crate::assign::RunContext,
    head: &str,
    base: &str,
) {
    env.forge
        .create_pull_request(
            &rc.project.id,
            df_core::NewPullRequest {
                repo: crate::pr::repo_ref(&rc.project.repo_url).unwrap(),
                head: head.into(),
                base: base.into(),
                title: "t".into(),
                body: String::new(),
                draft: false,
            },
        )
        .await
        .unwrap();
}

fn on_branch(branch: &str) -> serde_json::Value {
    json!({"persona": "Linus", "title": "Rebase", "prompt": "Rebase it.", "branch": branch})
}

async fn task_count(env: &crate::testing::Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn create_task_on_a_renovate_branch_attaches_its_pr() {
    let (env, rc) = setup(pool().await).await;
    open_pr(&env, &rc, "renovate/serde-1.x", "release/1.2").await;

    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/serde-1.x")),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    let row: (String, Option<i64>, Option<String>, bool, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT branch, pr_number, base_branch, pr_attached, parent_id FROM tasks WHERE key = 'DF-2'",
    )
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(row.0, "renovate/serde-1.x");
    assert_eq!(
        (row.1, row.2.as_deref(), row.3),
        (Some(1), Some("release/1.2"), true)
    );
    assert_eq!(row.4, Some(rc.task.id.as_uuid()));
}

#[tokio::test]
async fn clearing_the_agent_branches_leaves_a_running_task_on_its_branch() {
    let (env, rc) = setup(pool().await).await;
    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/serde-1.x")),
    )
    .await;
    assert!(!result.is_error, "{}", message(&result));

    sqlx::query("UPDATE projects SET agent_branch_globs = '{}'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let task_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-2'")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    let run_id = env
        .insert_run(df_core::TaskId::from_uuid(task_id), 1, "running")
        .await;
    let run = crate::runs::get_run(&env.ctx.pool, run_id)
        .await
        .unwrap()
        .unwrap();
    let child = crate::assign::load_run_context(&env.ctx.pool, run)
        .await
        .unwrap();
    let workspace = crate::assign::build_assign(&env.ctx, &child)
        .await
        .unwrap()
        .workspace
        .unwrap();
    let routes = df_db::egress::active_runs(&env.ctx.pool).await.unwrap();

    assert_eq!(workspace.task_branch, "renovate/serde-1.x");
    assert!(workspace.existing_branch);
    let route = routes.iter().find(|r| r.task_key == "DF-2").unwrap();
    assert_eq!(route.task_branch, "renovate/serde-1.x");
}

#[tokio::test]
async fn create_task_on_a_branch_without_a_pr_has_no_pr() {
    let (env, rc) = setup(pool().await).await;

    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/login")),
    )
    .await;

    assert!(!result.is_error, "{}", message(&result));
    let row: (Option<i64>, bool) =
        sqlx::query_as("SELECT pr_number, pr_attached FROM tasks WHERE key = 'DF-2'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert_eq!(row, (None, false));
}

#[tokio::test]
async fn create_task_refuses_a_held_branch_naming_the_holder() {
    let (env, rc) = setup(pool().await).await;
    let first = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/serde-1.x")),
    )
    .await;
    assert!(!first.is_error, "{}", message(&first));

    let again = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/serde-1.x")),
    )
    .await;

    assert!(
        again.is_error && message(&again).contains("DF-2"),
        "{}",
        message(&again)
    );
    assert_eq!(task_count(&env).await, 2);
}

#[tokio::test]
async fn create_task_refuses_rejected_branches_and_a_base_task() {
    let (env, rc) = setup(pool().await).await;
    let mut with_base = on_branch("renovate/login");
    with_base["base_task"] = json!("DF-1");
    for (args, named) in [
        (on_branch("main"), "base branch"),
        (on_branch("factory/task/DF-7"), "factory"),
        (on_branch("fix/../main"), "not a valid"),
        (on_branch("refs/heads/fix"), "refs/heads"),
        (with_base, "base_task"),
    ] {
        let result = execute(&env.ctx, &rc, call("create_task", args)).await;
        assert!(
            result.is_error && message(&result).contains(named),
            "{named}: {}",
            message(&result)
        );
    }
    assert_eq!(task_count(&env).await, 1);
}

#[tokio::test]
async fn create_task_refuses_a_branch_outside_the_agent_branches_naming_branch_and_globs() {
    let (env, rc) = setup(pool().await).await;

    let result = execute(&env.ctx, &rc, call("create_task", on_branch("release/1.2"))).await;

    let text = message(&result);
    assert!(
        result.is_error
            && text.contains("release/1.2")
            && text.contains("renovate/**")
            && text.contains("dependabot/**"),
        "{text}"
    );
    assert_eq!(task_count(&env).await, 1);
}

#[tokio::test]
async fn create_task_refuses_every_branch_when_the_project_has_no_agent_branches() {
    let (env, mut rc) = setup(pool().await).await;
    rc.project.agent_branch_globs.clear();

    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/serde-1.x")),
    )
    .await;

    assert!(result.is_error, "{}", message(&result));
    assert_eq!(task_count(&env).await, 1);
}

#[tokio::test]
async fn create_task_accepts_a_branch_of_a_glob_the_project_added() {
    let (env, mut rc) = setup(pool().await).await;
    rc.project.agent_branch_globs.push("release/**".into());

    let result = execute(&env.ctx, &rc, call("create_task", on_branch("release/1.2"))).await;

    assert!(!result.is_error, "{}", message(&result));
}

#[tokio::test]
async fn create_task_refuses_a_branch_with_several_open_prs_naming_them() {
    let (env, rc) = setup(pool().await).await;
    open_pr(&env, &rc, "renovate/shared", "main").await;
    open_pr(&env, &rc, "renovate/shared", "release/1.2").await;

    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/shared")),
    )
    .await;

    assert!(
        result.is_error && message(&result).contains("#1") && message(&result).contains("#2"),
        "{}",
        message(&result)
    );
    assert_eq!(task_count(&env).await, 1);
}

#[tokio::test]
async fn create_task_fails_with_the_forge_error_and_creates_nothing() {
    let (env, rc) = setup(pool().await).await;
    env.forge
        .push_list_open_pull_requests_result(Err(df_core::IntegrationError::Transport(
            "down".into(),
        )));

    let result = execute(
        &env.ctx,
        &rc,
        call("create_task", on_branch("renovate/login")),
    )
    .await;

    assert!(
        result.is_error && message(&result).contains("down"),
        "{}",
        message(&result)
    );
    assert_eq!(task_count(&env).await, 1);
}

#[tokio::test]
async fn send_message_to_a_final_child_whose_branch_is_taken_names_the_holder() {
    let (env, rc) = setup(pool().await).await;
    let old = env.task_on_branch("renovate/login", None).await;
    sqlx::query("UPDATE tasks SET status = 'cancelled', parent_id = $2 WHERE id = $1")
        .bind(old.id.as_uuid())
        .bind(rc.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let holder = env.task_on_branch("renovate/login", None).await;

    let result = execute(
        &env.ctx,
        &rc,
        call(
            "send_message",
            json!({"task": old.key.to_string(), "text": "again"}),
        ),
    )
    .await;

    assert!(
        result.is_error && message(&result).contains(&holder.key.to_string()),
        "{}",
        message(&result)
    );
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(old.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");
}
