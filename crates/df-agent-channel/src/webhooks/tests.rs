//! Tests of applying parsed webhook events: project match, routing, filters and the merge.

use df_core::seams::ReviewVerdict;
use df_core::testing::{FakeForgeClient, ForgeCall};
use df_core::{IntegrationError, Principal, RunOutcome};
use df_db::tasks::list_undelivered_messages;
use serde_json::Value;

use super::*;
use crate::runs::{RunEnd, end_run};
use crate::testing::Env;

const REPO: &str = "https://git.example.org/Peter/dark-factory";

fn applier(env: &Env) -> WebhookApplier {
    WebhookApplier::new(env.ctx.pool.clone(), env.ctx.forge.clone())
}

fn parsed(event: WebhookEvent) -> ParsedEvent {
    ParsedEvent {
        repo_url: REPO.into(),
        event,
    }
}

fn merged(actor: &str) -> ParsedEvent {
    parsed(WebhookEvent::PullRequestClosed {
        number: 12,
        head: "factory/task/DF-1".into(),
        actor: actor.into(),
        merged: true,
    })
}

fn review(actor: &str) -> ParsedEvent {
    parsed(WebhookEvent::ReviewSubmitted {
        number: 12,
        head: "factory/task/DF-1".into(),
        actor: actor.into(),
        verdict: ReviewVerdict::ChangesRequested,
    })
}

async fn set_status(env: &Env, status: &str) {
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .bind(status)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

async fn set_wake(env: &Env, wake: &str) {
    sqlx::query("UPDATE projects SET webhook_wake = $1")
        .bind(wake)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

async fn status(env: &Env, task_id: TaskId) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

/// The task's undelivered messages from webhooks.
async fn queued(env: &Env, task_id: TaskId) -> usize {
    list_undelivered_messages(&env.ctx.pool, task_id)
        .await
        .unwrap()
        .iter()
        .filter(|m| m.author == Principal::system(df_core::system_reason::WEBHOOK))
        .count()
}

async fn done_audits(env: &Env) -> Vec<Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'task.done'")
        .fetch_all(&env.ctx.pool)
        .await
        .unwrap()
}

fn whoami_calls(env: &Env) -> usize {
    env.forge
        .calls()
        .iter()
        .filter(|call| matches!(call, ForgeCall::Whoami { .. }))
        .count()
}

#[tokio::test]
async fn merged_pr_completes_the_task_in_review_without_asking_the_forge() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_status(&env, "in_review").await;

    applier(&env).apply(&[merged("carol")]).await.unwrap();

    assert_eq!(status(&env, env.task.id).await, "completed");
    let audits = done_audits(&env).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["old_status"], "in_review");
    assert_eq!(audits[0]["login"], "carol");
    assert!(env.forge.calls().is_empty(), "{:?}", env.forge.calls());
}

#[tokio::test]
async fn merged_pr_stops_the_running_run_and_the_task_stays_completed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_status(&env, "running").await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;

    applier(&env).apply(&[merged("carol")]).await.unwrap();
    let stopped: bool =
        sqlx::query_scalar("SELECT stop_requested_at IS NOT NULL FROM runs WHERE id = $1")
            .bind(run_id.as_uuid())
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
    assert!(stopped);
    let end = RunEnd::failure(run_id, RunOutcome::Cancelled, "stopped");
    end_run(&env.ctx.pool, &end, &env.ctx.config).await.unwrap();

    assert_eq!(status(&env, env.task.id).await, "completed");
}

#[tokio::test]
async fn merged_pr_of_a_cancelled_task_keeps_it_cancelled() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_status(&env, "cancelled").await;

    applier(&env).apply(&[merged("carol")]).await.unwrap();

    assert_eq!(status(&env, env.task.id).await, "cancelled");
    assert!(done_audits(&env).await.is_empty());
}

#[tokio::test]
async fn merged_pr_redelivery_writes_one_audit_entry() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let applier = applier(&env);

    applier.apply(&[merged("carol")]).await.unwrap();
    applier.apply(&[merged("carol")]).await.unwrap();

    assert_eq!(done_audits(&env).await.len(), 1);
}

#[tokio::test]
async fn merged_pr_by_the_bot_completes_the_task_under_wake_policy_none() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_wake(&env, "none").await;

    applier(&env)
        .apply(&[merged(FakeForgeClient::DEFAULT_BOT_LOGIN)])
        .await
        .unwrap();

    assert_eq!(status(&env, env.task.id).await, "completed");
}

#[tokio::test]
async fn webhook_review_from_a_member_queues_a_message_for_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge.set_author_role("alice", AuthorRole::Member);

    applier(&env).apply(&[review("alice")]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 1);
    assert!(env.forge.calls().contains(&ForgeCall::AuthorRole {
        project_id: env.project_id,
        login: "alice".into(),
    }));
}

/// The forge answers `outside` for an author it would not tell the role of (a refused permission
/// lookup in the plugin): under `members` the event is dropped without an error.
#[tokio::test]
async fn webhook_review_from_an_outside_author_is_dropped_under_members_and_accepted_under_anyone()
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_wake(&env, "members").await;
    let applier = applier(&env);

    applier.apply(&[review("mallory")]).await.unwrap();
    assert_eq!(queued(&env, env.task.id).await, 0);

    set_wake(&env, "anyone").await;
    applier.apply(&[review("mallory")]).await.unwrap();
    assert_eq!(queued(&env, env.task.id).await, 1);
}

#[tokio::test]
async fn webhook_review_is_dropped_under_wake_policy_none() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge.set_author_role("alice", AuthorRole::Member);
    set_wake(&env, "none").await;

    applier(&env).apply(&[review("alice")]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 0);
}

#[tokio::test]
async fn webhook_review_from_the_bot_is_dropped() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge.set_bot_login("DF-Bot");
    set_wake(&env, "anyone").await;

    applier(&env).apply(&[review("df-bot")]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 0);
    assert_eq!(whoami_calls(&env), 1);
}

#[tokio::test]
async fn webhook_comment_without_a_mention_of_the_bot_is_dropped() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    set_wake(&env, "anyone").await;
    sqlx::query("UPDATE tasks SET pr_number = 12 WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let comment = parsed(WebhookEvent::CommentCreated {
        number: 12,
        is_pull_request: true,
        actor: "alice".into(),
        mentions: vec!["carol".into()],
    });

    applier(&env).apply(&[comment]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 0);
}

#[tokio::test]
async fn webhook_mention_on_a_linked_issue_reaches_every_open_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge.set_author_role("alice", AuthorRole::Member);
    let mention = parsed(WebhookEvent::CommentCreated {
        number: 42,
        is_pull_request: false,
        actor: "alice".into(),
        mentions: vec![FakeForgeClient::DEFAULT_BOT_LOGIN.into()],
    });

    applier(&env).apply(&[mention]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 1);
}

#[tokio::test]
async fn webhook_event_of_another_repository_is_ignored_and_logged() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let mut event = merged("eve");
    event.repo_url = "https://git.example.org/eve/other".into();
    let (logs, _guard) = df_telemetry::testing::capture("debug");

    applier(&env).apply(&[event]).await.unwrap();

    assert_eq!(status(&env, env.task.id).await, "queued");
    let captured = logs.text();
    assert!(
        captured.contains("webhook repository matches no project"),
        "{captured}"
    );
    assert!(captured.contains("eve/other"), "{captured}");
}

#[tokio::test]
async fn webhook_branch_outside_the_factory_is_ignored() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let event = parsed(WebhookEvent::CiFailed {
        head: "feature/login".into(),
        check: "test".into(),
        url: String::new(),
    });

    applier(&env).apply(&[event]).await.unwrap();

    assert_eq!(queued(&env, env.task.id).await, 0);
}

#[tokio::test]
async fn webhook_failing_bot_lookup_fails_the_delivery() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge
        .push_whoami_result(Err(IntegrationError::Transport("down".into())));

    let result = applier(&env).apply(&[review("alice")]).await;

    assert!(result.is_err(), "{result:?}");
    assert_eq!(queued(&env, env.task.id).await, 0);
}

#[tokio::test]
async fn webhook_review_on_an_attached_pr_routes_to_the_task_holding_its_branch() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    env.forge.set_author_role("alice", AuthorRole::Member);
    let attached = env.task_on_branch("renovate/serde-1.x", None).await;
    let event = parsed(WebhookEvent::ReviewSubmitted {
        number: 40,
        head: "renovate/serde-1.x".into(),
        actor: "alice".into(),
        verdict: ReviewVerdict::ChangesRequested,
    });

    applier(&env).apply(&[event]).await.unwrap();

    assert_eq!(queued(&env, attached.id).await, 1);
    assert_eq!(queued(&env, env.task.id).await, 0);
}

#[tokio::test]
async fn webhook_merge_of_an_attached_pr_completes_its_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let attached = env.task_on_branch("renovate/serde-1.x", None).await;
    sqlx::query("UPDATE tasks SET status = 'in_review' WHERE id = $1")
        .bind(attached.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    let event = parsed(WebhookEvent::PullRequestClosed {
        number: 40,
        head: "renovate/serde-1.x".into(),
        actor: "bob".into(),
        merged: true,
    });

    applier(&env).apply(&[event]).await.unwrap();

    assert_eq!(status(&env, attached.id).await, "completed");
}
