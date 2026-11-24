//! Tests of the merge tool (`task-tree` "Merging subtask PRs" and "Merge outcome",
//! `factory-tools` "`merge_subtask_pr`", `pull-request-lifecycle` "PR after a subtask merge").

use std::sync::Arc;
use std::time::Duration;

use df_core::testing::ForgeCall;
use df_core::{IntegrationError, RunId, TaskId};
use df_db::git_pushes::{PushRecord, PushVerdict, RefUpdate, record_push};
use df_db::outbox::OutboxHandler;
use df_orchestrator::pr::sync::PrSyncHandler;
use df_orchestrator::run_context::RunContext;
use serde_json::json;
use sqlx::PgPool;

use crate::testing::Env;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

const HEAD: &str = "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";

async fn pool() -> PgPool {
    df_test_postgres::migrated_pool(&df_db::MIGRATOR).await
}

fn merge(task: &str) -> df_proto::agent::v1::ToolCallRequest {
    call("merge_subtask_pr", json!({ "task": task }))
}

/// `DF-2`, an iteration of the run's task `DF-1` (parent and base task), in review with PR #5 and
/// an accepted push of `HEAD`; the project lets parent agents merge.
async fn iteration(env: &Env, rc: &RunContext) -> TaskId {
    sqlx::query("UPDATE projects SET subtask_merge = 'parent-agent'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let created = execute(
        &env.ctx,
        rc,
        call(
            "create_task",
            json!({"persona": "linus", "title": "Iteration", "prompt": "Do it.", "base_task": "DF-1"}),
        ),
    )
    .await;
    assert!(!created.is_error, "{}", message(&created));
    let task = task_id(env, "DF-2").await;
    let run = env.insert_run(task, 1, "succeeded").await;
    push(env, run, HEAD).await;
    sqlx::query(
        "UPDATE tasks SET status = 'in_review', pr_number = 5, pr_url = 'https://forge/pulls/5'
         WHERE id = $1",
    )
    .bind(task.as_uuid())
    .execute(&env.ctx.pool)
    .await
    .unwrap();
    task
}

async fn push(env: &Env, run: RunId, sha: &str) {
    let record = PushRecord {
        push_id: uuid::Uuid::now_v7(),
        run_id: run,
        gateway_pod: "gateway-0".into(),
        refs: vec![RefUpdate {
            name: "refs/heads/factory/task/DF-2".into(),
            old_sha: "0".repeat(40),
            new_sha: sha.into(),
            verdict: PushVerdict::Accepted,
            reason: String::new(),
        }],
        verdict: PushVerdict::Accepted,
        bytes: 1,
        duration: Duration::from_millis(1),
        findings: Vec::new(),
        approval_requested: false,
    };
    record_push(&env.ctx.pool, &record).await.unwrap();
}

async fn task_id(env: &Env, key: &str) -> TaskId {
    let id = sqlx::query_scalar("SELECT id FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    TaskId::from_uuid(id)
}

async fn status(env: &Env, key: &str) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

fn merges(env: &Env) -> Vec<(u64, String)> {
    env.forge
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            ForgeCall::MergePullRequest {
                number, head_sha, ..
            } => Some((number, head_sha)),
            _ => None,
        })
        .collect()
}

async fn refused(env: &Env, rc: &RunContext, task: &str, named: &str) {
    let result = execute(&env.ctx, rc, merge(task)).await;
    assert!(
        result.is_error && message(&result).contains(named),
        "{}",
        message(&result)
    );
    assert!(merges(env).is_empty(), "nothing is merged");
}

// Scenario "Iteration merged by the feature agent" and "Merged".
#[tokio::test]
async fn the_parent_agent_merges_an_iteration_with_the_last_pushed_head() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;

    let result = execute(&env.ctx, &rc, merge("DF-2")).await;

    assert!(!result.is_error, "{}", message(&result));
    assert!(message(&result).contains("https://forge/pulls/5"));
    assert_eq!(merges(&env), [(5, HEAD.to_string())]);
    assert_eq!(status(&env, "DF-2").await, "completed");
    let merged: bool =
        sqlx::query_scalar("SELECT pr_merged_at IS NOT NULL FROM tasks WHERE key = 'DF-2'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert!(merged);
    let actor: serde_json::Value = sqlx::query_scalar(
        "SELECT actor FROM audit_log WHERE action = 'task.done' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(actor["kind"], "agent");
    assert_eq!(actor["task_id"], json!(rc.task.id));
}

// Scenario "Retried call".
#[tokio::test]
async fn a_retried_call_merges_once_and_answers_the_same() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    let request = merge("DF-2");

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let again = execute(&env.ctx, &rc, request).await;

    assert!(!first.is_error);
    assert_eq!(first, again);
    assert_eq!(merges(&env).len(), 1);
}

// Scenario "Project keeps merges with humans".
#[tokio::test]
async fn a_project_that_keeps_merges_with_humans_refuses_and_names_the_setting() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    sqlx::query("UPDATE projects SET subtask_merge = 'human'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, "DF-2", "subtask_merge").await;
    assert_eq!(status(&env, "DF-2").await, "in_review");
}

// Scenario "Subtask still running".
#[tokio::test]
async fn a_subtask_that_is_not_in_review_is_refused() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    sqlx::query("UPDATE tasks SET status = 'running' WHERE key = 'DF-2'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, "DF-2", "in_review").await;
}

#[tokio::test]
async fn a_subtask_without_a_pr_is_refused() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    sqlx::query("UPDATE tasks SET pr_number = NULL, pr_url = NULL WHERE key = 'DF-2'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, "DF-2", "pull request").await;
}

#[tokio::test]
async fn a_subtask_that_is_not_based_on_the_caller_is_refused() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    sqlx::query("UPDATE tasks SET base_task_id = NULL WHERE key = 'DF-2'")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, "DF-2", "base task").await;
}

// Scenario "Not a subtask" and "Reviewer PR into the implementation branch".
#[tokio::test]
async fn a_task_that_is_not_a_subtask_of_the_caller_is_refused() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    // DF-3 builds on DF-1 but is a subtask of DF-2.
    let other = task_id(&env, "DF-2").await;
    let reviewer = env.task_based_on(&env.task).await;
    sqlx::query("UPDATE tasks SET parent_id = $2, status = 'in_review', pr_number = 6, pr_url = 'u' WHERE id = $1")
        .bind(reviewer.id.as_uuid())
        .bind(other.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, &reviewer.key.to_string(), "subtask").await;
    refused(&env, &rc, "DF-99", "subtask").await;
    refused(&env, &rc, "nonsense", "subtask").await;
}

// Scenario "Conflict".
#[tokio::test]
async fn a_forge_refusal_comes_back_with_its_reason_and_changes_nothing() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    env.forge
        .push_merge_pull_request_result(Err(IntegrationError::Conflict(
            "merge conflict in src/lib.rs".into(),
        )));

    let result = execute(&env.ctx, &rc, merge("DF-2")).await;

    assert!(
        result.is_error && message(&result).contains("merge conflict in src/lib.rs"),
        "{}",
        message(&result)
    );
    assert_eq!(status(&env, "DF-2").await, "in_review");
    let merged: bool =
        sqlx::query_scalar("SELECT pr_merged_at IS NOT NULL FROM tasks WHERE key = 'DF-2'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert!(!merged);
}

#[tokio::test]
async fn a_subtask_without_an_accepted_push_cannot_be_merged() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    sqlx::query("DELETE FROM git_pushes")
        .execute(&env.ctx.pool)
        .await
        .unwrap();

    refused(&env, &rc, "DF-2", "push").await;
}

// The forge merged, but the subtask was cancelled while the call ran: the merge is reported, the
// subtask stays as it is.
#[tokio::test]
async fn a_subtask_cancelled_during_the_forge_call_is_not_marked_done() {
    let (env, rc) = setup(pool().await).await;
    let subtask = iteration(&env, &rc).await;
    sqlx::query("UPDATE tasks SET status = 'cancelled' WHERE id = $1")
        .bind(subtask.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let merged = super::merge::Mergeable {
        task: subtask,
        number: 5,
        url: "https://forge/pulls/5".into(),
        head: HEAD.into(),
    };

    let not_done = super::merge::record_merge(&env.ctx, &rc, &merged)
        .await
        .unwrap();

    assert_eq!(not_done, Some(df_core::TaskStatus::Cancelled));
    let text = super::merge::merged_text("DF-2", "https://forge/pulls/5", not_done);
    assert_eq!(
        text,
        "Merged DF-2: https://forge/pulls/5. DF-2 was cancelled meanwhile and stays cancelled."
    );
    assert_eq!(status(&env, "DF-2").await, "cancelled");
    let merged_at: bool =
        sqlx::query_scalar("SELECT pr_merged_at IS NOT NULL FROM tasks WHERE key = 'DF-2'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert!(!merged_at);
}

#[tokio::test]
async fn a_subtask_completed_during_the_forge_call_is_reported_as_already_done() {
    let (env, rc) = setup(pool().await).await;
    let subtask = iteration(&env, &rc).await;
    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(subtask.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let merged = super::merge::Mergeable {
        task: subtask,
        number: 5,
        url: "https://forge/pulls/5".into(),
        head: HEAD.into(),
    };

    let not_done = super::merge::record_merge(&env.ctx, &rc, &merged)
        .await
        .unwrap();

    assert_eq!(not_done, Some(df_core::TaskStatus::Completed));
    assert_eq!(
        super::merge::merged_text("DF-2", "https://forge/pulls/5", not_done),
        "Merged DF-2: https://forge/pulls/5. DF-2 is already done."
    );
}

#[test]
fn a_merge_that_marked_the_subtask_done_says_so() {
    assert_eq!(
        super::merge::merged_text("DF-2", "u", None),
        "Merged DF-2: u. DF-2 is done."
    );
}

// The run rows are locked before the task row, as in every other transaction (a run end takes
// them in that order). One transaction holds the subtask's run rows; once the recording waits
// for them, that transaction asks for the task row. With the task row locked first, the recording
// would hold it and the two would wait for each other.
#[tokio::test]
async fn recording_a_merge_does_not_deadlock_with_a_transaction_holding_the_run_rows() {
    let (env, rc) = setup(pool().await).await;
    let subtask = iteration(&env, &rc).await;
    sqlx::query("INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, 10, 'running', '10.0.0.1')")
        .bind(RunId::new().as_uuid())
        .bind(subtask.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let mut holder = env.ctx.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM runs WHERE task_id = $1 FOR UPDATE")
        .bind(subtask.as_uuid())
        .fetch_all(&mut *holder)
        .await
        .unwrap();
    let merged = super::merge::Mergeable {
        task: subtask,
        number: 5,
        url: "u".into(),
        head: HEAD.into(),
    };

    let recording = super::merge::record_merge(&env.ctx, &rc, &merged);
    let holding = async {
        df_test_postgres::wait_for_lock_wait(&env.ctx.pool).await;
        sqlx::query("SELECT id FROM tasks WHERE id = $1 FOR UPDATE")
            .bind(subtask.as_uuid())
            .fetch_one(&mut *holder)
            .await?;
        holder.commit().await
    };
    let (recorded, held) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(recording, holding)
    })
    .await
    .expect("no deadlock");

    held.unwrap();
    recorded.unwrap();
}

// Scenario "Merged": a later merge webhook changes nothing.
#[tokio::test]
async fn a_later_merge_webhook_changes_nothing() {
    let (env, rc) = setup(pool().await).await;
    let subtask = iteration(&env, &rc).await;
    execute(&env.ctx, &rc, merge("DF-2")).await;
    let audits = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_log WHERE action = 'task.done'")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap()
    };
    let before = audits().await;

    let mut tx = env.ctx.pool.begin().await.unwrap();
    let applied = df_db::webhooks::complete_on_merge_tx(&mut tx, subtask, "bot")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(!applied);
    assert_eq!(audits().await, before);
}

// Scenario "First iteration merged" of `pull-request-lifecycle`.
#[tokio::test]
async fn the_merge_opens_the_draft_pr_of_the_parent() {
    let (env, rc) = setup(pool().await).await;
    iteration(&env, &rc).await;
    execute(&env.ctx, &rc, merge("DF-2")).await;
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM outbox WHERE kind = 'pr.sync' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(payload["task_id"], json!(rc.task.id));
    assert_eq!(payload["open"], json!(true));

    let handler = PrSyncHandler::new(
        env.ctx.pool.clone(),
        Arc::new(env.forge.clone()),
        Duration::from_secs(30),
    );
    handler.handle(&payload).await.unwrap();

    let created: Vec<_> = env
        .forge
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            ForgeCall::CreatePullRequest { request, .. } => Some((request.head, request.base)),
            _ => None,
        })
        .collect();
    assert_eq!(
        created,
        [("factory/task/DF-1".to_string(), "main".to_string())]
    );
}
