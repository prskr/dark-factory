//! The `pr.sync` job enqueued at the end of a run, driven by the outbox dispatcher
//! (scenarios "Forge down at completion" and "Restart at completion").

use df_core::testing::ForgeCall;
use df_core::{RunId, RunOutcome};
use df_db::outbox::Dispatcher;

use super::*;
use crate::ChannelConfig;
use crate::runs::{RunEnd, end_run};
use crate::testing::Env;

/// A run of `DF-1` that pushed its task branch and ended `completed` with a summary; the job is
/// enqueued by the run end and nothing has been dispatched yet.
async fn completed_run() -> Env {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run = env.insert_run(env.task.id, 1, "running").await;
    sqlx::query("UPDATE runs SET completion_summary = 'Fixed login' WHERE id = $1")
        .bind(run.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let end = RunEnd {
        task_branch_pushed: true,
        ..RunEnd::failure(run, RunOutcome::Completed, "")
    };
    end_run(&pool, &end, &ChannelConfig::default())
        .await
        .unwrap();
    env
}

/// A dispatcher of a (restarted) replica.
fn dispatcher(env: &Env) -> Dispatcher {
    let handler = PrSyncHandler::new(
        env.ctx.pool.clone(),
        Arc::new(env.forge.clone()),
        Duration::from_secs(30),
    );
    Dispatcher::new(
        env.ctx.pool.clone(),
        env.ctx.notifier.clone(),
        vec![Arc::new(handler)],
        &opentelemetry::global::meter("test"),
    )
}

/// Skips the backoff of the waiting job.
async fn make_due(env: &Env) {
    sqlx::query("UPDATE outbox SET next_attempt_at = now()")
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

async fn assert_ready_with_summary(env: &Env) {
    let calls = env.forge.calls();
    assert!(
        calls.iter().any(|c| matches!(c,
            ForgeCall::UpdatePullRequest { number: 1, changes, .. } if changes.draft == Some(false))),
        "{calls:?}"
    );
    assert!(
        calls.iter().any(|c| matches!(c,
            ForgeCall::AddIssueComment { number: 1, body, .. } if body == "Fixed login")),
        "{calls:?}"
    );
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox WHERE done_at IS NULL AND gave_up_at IS NULL",
    )
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(open, 0, "the job finished");
}

#[tokio::test]
async fn a_forge_that_fails_marking_ready_twice_still_ends_with_a_ready_pr_and_the_summary() {
    let env = completed_run().await;
    let down = || Err(IntegrationError::Transport("forge down".into()));
    env.forge.push_update_pull_request_result(down());
    env.forge.push_update_pull_request_result(down());
    let dispatcher = dispatcher(&env);

    for _attempt in 0..3 {
        make_due(&env).await;
        dispatcher.run_due().await.unwrap();
    }

    assert_ready_with_summary(&env).await;
}

#[tokio::test]
async fn a_job_enqueued_before_a_restart_runs_on_the_new_dispatcher() {
    let env = completed_run().await;
    // The session that ended the run is gone; no dispatcher ran before.

    let claimed = dispatcher(&env).run_due().await.unwrap();

    assert_eq!(claimed, 1);
    assert_ready_with_summary(&env).await;
}

/// A task whose run pushed 2 commits ahead of base: the push report enqueued the open job and
/// nothing has been dispatched yet. Returns the run too.
async fn pushed_run() -> (Env, RunId) {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run = env.insert_run(env.task.id, 1, "running").await;
    let mut tx = pool.begin().await.unwrap();
    assert!(
        crate::pr::enqueue_open_after_push(&mut tx, env.task.id, run)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    (env, run)
}

async fn recorded_pr(env: &Env) -> Option<i64> {
    sqlx::query_scalar("SELECT pr_number FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

async fn recorded_pr_is_a_draft(env: &Env) -> bool {
    env.forge
        .find_open_pull_request(&env.project_id, "factory/task/DF-1", "main")
        .await
        .unwrap()
        .is_some_and(|pr| pr.draft)
}

#[tokio::test]
async fn a_forge_that_fails_creating_the_pr_once_still_ends_with_one_draft_pr() {
    let (env, _) = pushed_run().await;
    env.forge
        .push_create_pull_request_result(Err(IntegrationError::Transport("forge down".into())));
    let dispatcher = dispatcher(&env);

    dispatcher.run_due().await.unwrap();
    assert_eq!(recorded_pr(&env).await, None);
    make_due(&env).await;
    dispatcher.run_due().await.unwrap();

    assert_eq!(recorded_pr(&env).await, Some(1));
    assert!(recorded_pr_is_a_draft(&env).await);
    let opened = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM transcript_events WHERE source_key = 'pr-opened:1'",
    )
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap();
    assert_eq!(opened, 1);
    // One failed and one successful create: the forge holds exactly one PR.
    let creates = env
        .forge
        .calls()
        .iter()
        .filter(|call| matches!(call, ForgeCall::CreatePullRequest { .. }))
        .count();
    assert_eq!(creates, 2);
}

#[tokio::test]
async fn a_push_job_enqueued_before_a_restart_opens_the_draft_pr_on_the_new_dispatcher() {
    let (env, _) = pushed_run().await;
    // The session that stored the push report is gone; no dispatcher ran before.

    let claimed = dispatcher(&env).run_due().await.unwrap();

    assert_eq!(claimed, 1);
    assert_eq!(recorded_pr(&env).await, Some(1));
    assert!(recorded_pr_is_a_draft(&env).await);
}

#[tokio::test]
async fn a_message_that_waited_for_the_run_end_keeps_the_pr_a_draft() {
    let (env, run) = pushed_run().await;
    let pool = &env.ctx.pool;
    // The message transaction starts before the run ends and commits after it.
    let mut message_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT now()")
        .execute(&mut *message_tx)
        .await
        .unwrap();
    let end = RunEnd {
        task_branch_pushed: true,
        ..RunEnd::failure(run, RunOutcome::Completed, "")
    };
    end_run(pool, &end, &ChannelConfig::default())
        .await
        .unwrap();
    df_db::tasks::queue_message_tx(
        &mut message_tx,
        env.task.id,
        env.human(),
        vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "one more thing".into(),
                },
            )),
            hidden: false,
        }],
    )
    .await
    .unwrap();
    message_tx.commit().await.unwrap();

    dispatcher(&env).run_due().await.unwrap();

    assert!(recorded_pr(&env).await.is_some());
    assert!(recorded_pr_is_a_draft(&env).await);
}
