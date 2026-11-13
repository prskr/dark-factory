//! Tests of the run rows: start query, run end mapping and resume plan.

use chrono::TimeDelta;
use df_core::CAPACITY_DETAIL;

use super::*;
use crate::testing::{Env, task_resume_state};

/// `runs.maxAutoResumes` of the tests.
const MAX_AUTO_RESUMES: u32 = 3;

fn config() -> ChannelConfig {
    let mut config = ChannelConfig::default();
    config.runs.max_auto_resumes = MAX_AUTO_RESUMES;
    config
}

#[tokio::test]
async fn blocked_run_is_terminal_and_blocks_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    let ended = end_run(&pool, &end_with(run_id, RunOutcome::Blocked), &config())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.run_state, RunState::Succeeded);
    assert_eq!(status_of(&pool, env.task.id).await, "blocked");
    // No further phase changes are accepted for it.
    let mut tx = pool.begin().await.unwrap();
    set_idle_tx(&mut tx, run_id, env.task.id, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
}

#[tokio::test]
async fn blocked_run_with_an_undelivered_message_queues_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // The seeded message is still undelivered.
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    end_run(&pool, &end_with(run_id, RunOutcome::Blocked), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "queued");
}

async fn stop(pool: &PgPool, run_id: RunId) {
    sqlx::query(
        "UPDATE tasks SET status = 'running' WHERE id = (SELECT task_id FROM runs WHERE id = $1)",
    )
    .bind(run_id.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE runs SET stop_requested_at = now(), state = 'finalizing' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn stopped_run_with_a_message_queued_before_the_stop_leaves_the_task_waiting() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // The seeded message is undelivered and older than the stop.
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    stop(&pool, run_id).await;
    end_run(&pool, &end_with(run_id, RunOutcome::Cancelled), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "waiting");
}

#[tokio::test]
async fn stopped_run_with_a_message_queued_after_the_stop_queues_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    stop(&pool, run_id).await;
    df_db::queue_message(&pool, env.task.id, env.human(), vec![])
        .await
        .unwrap();
    end_run(&pool, &end_with(run_id, RunOutcome::Cancelled), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "queued");
}

#[tokio::test]
async fn run_ending_after_its_task_was_cancelled_keeps_the_task_cancelled() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    df_db::cancel_task(&pool, env.task.id, env.human())
        .await
        .unwrap();
    let ended = end_run(&pool, &end_with(run_id, RunOutcome::Cancelled), &config())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.task_status, TaskStatus::Cancelled);
    assert_eq!(status_of(&pool, env.task.id).await, "cancelled");
}

#[tokio::test]
async fn task_reopened_before_the_stopped_run_ended_stays_queued() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // The outcome does not matter: the message is next. One fresh run per outcome.
    for (number, outcome) in [(1, RunOutcome::Cancelled), (2, RunOutcome::Failed)] {
        let run_id = env.insert_run(env.task.id, number, "running").await;
        df_db::cancel_task(&pool, env.task.id, env.human())
            .await
            .unwrap();
        df_db::queue_message(&pool, env.task.id, env.human(), vec![])
            .await
            .unwrap();
        let ended = end_run(&pool, &end_with(run_id, outcome), &config())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.task_status, TaskStatus::Queued, "{outcome:?}");
        assert_eq!(status_of(&pool, env.task.id).await, "queued", "{outcome:?}");
    }
}

#[tokio::test]
async fn a_stop_during_finalizing_still_requeues_the_task_for_its_undelivered_message() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // The seeded message is undelivered; the run is ending already (`complete_task`).
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    df_db::stop_run(&pool, env.task.id, env.human())
        .await
        .unwrap();
    end_run(&pool, &end_with(run_id, RunOutcome::Completed), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "queued");
}

fn end_with(run_id: RunId, outcome: RunOutcome) -> RunEnd {
    RunEnd::failure(run_id, outcome, "")
}

fn minutes_from_now(at: Option<DateTime<Utc>>) -> i64 {
    (at.expect("a resume time") - Utc::now() + TimeDelta::seconds(30)).num_minutes()
}

#[tokio::test]
async fn rate_limit_sets_the_resume_time_to_the_reported_retry_time() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    let retry = Utc::now() + TimeDelta::minutes(150);
    let end = RunEnd {
        retry_after: Some(retry),
        ..end_with(run_id, RunOutcome::RateLimited)
    };
    end_run(&pool, &end, &config()).await.unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(state.status, "rate_limited");
    assert_eq!(
        state.resume_at.map(|t| t.timestamp_micros()),
        Some(retry.timestamp_micros())
    );
    assert_eq!(state.auto_resumes, 0);
}

#[tokio::test]
async fn rate_limit_without_a_retry_time_resumes_in_an_hour() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    end_run(&pool, &end_with(run_id, RunOutcome::RateLimited), &config())
        .await
        .unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(state.status, "rate_limited");
    assert_eq!(minutes_from_now(state.resume_at), 60);
}

#[tokio::test]
async fn infrastructure_failures_resume_with_a_growing_backoff_until_the_cap() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    for (number, outcome) in [
        (1, RunOutcome::SandboxLost),
        (2, RunOutcome::ConnectTimeout),
        (3, RunOutcome::ProvisioningFailed),
    ] {
        let run_id = env.insert_run(env.task.id, number, "running").await;
        end_run(&pool, &end_with(run_id, outcome), &config())
            .await
            .unwrap();
        let state = task_resume_state(&pool, env.task.id).await;
        assert_eq!(state.status, "queued", "{outcome:?}");
        assert_eq!(state.auto_resumes, i32::try_from(number).unwrap());
        assert_eq!(minutes_from_now(state.resume_at), 2 * number, "{outcome:?}");
    }
    // The fourth consecutive failure exceeds the cap of 3.
    let run_id = env.insert_run(env.task.id, 4, "running").await;
    end_run(
        &pool,
        &end_with(run_id, RunOutcome::ConnectTimeout),
        &config(),
    )
    .await
    .unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(
        (state.status.as_str(), state.resume_at, state.auto_resumes),
        ("failed", None, 3)
    );
}

#[tokio::test]
async fn a_run_report_resets_the_resume_count() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    sqlx::query("UPDATE tasks SET auto_resumes = 2 WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let run_id = env.insert_run(env.task.id, 3, "finalizing").await;
    end_run(&pool, &end_with(run_id, RunOutcome::EndTurn), &config())
        .await
        .unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(
        (state.status.as_str(), state.resume_at, state.auto_resumes),
        ("waiting", None, 0)
    );
}

#[tokio::test]
async fn a_harness_crash_is_not_resumed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    end_run(&pool, &end_with(run_id, RunOutcome::Failed), &config())
        .await
        .unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!((state.status.as_str(), state.resume_at), ("failed", None));
}

#[tokio::test]
async fn a_missing_existing_branch_fails_the_run_with_its_reason_and_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let task = env.task_on_branch("fix/login", None).await;
    deliver_all(&pool, task.id).await;
    let run_id = env.insert_run(task.id, 1, "running").await;
    let reason =
        "Workspace preparation failed: the branch `fix/login` does not exist on the remote";

    end_run(
        &pool,
        &RunEnd::failure(run_id, RunOutcome::Failed, reason),
        &config(),
    )
    .await
    .unwrap();

    let state = task_resume_state(&pool, task.id).await;
    assert_eq!((state.status.as_str(), state.resume_at), ("failed", None));
    let detail: String = sqlx::query_scalar("SELECT detail FROM runs WHERE id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(detail.contains("fix/login"), "{detail}");
}

#[tokio::test]
async fn capacity_rejection_requeues_in_two_minutes_and_keeps_the_count() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    sqlx::query("UPDATE tasks SET auto_resumes = 3 WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let run_id = env.insert_run(env.task.id, 1, "provisioning").await;
    let end = RunEnd {
        detail: CAPACITY_DETAIL.into(),
        ..end_with(run_id, RunOutcome::ProvisioningFailed)
    };
    end_run(&pool, &end, &config()).await.unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(state.status, "queued");
    assert_eq!(state.auto_resumes, 3);
    assert_eq!(minutes_from_now(state.resume_at), 2);
}

#[tokio::test]
async fn infrastructure_failure_of_a_cancelled_task_does_not_reopen_it() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    df_db::cancel_task(&pool, env.task.id, env.human())
        .await
        .unwrap();
    end_run(&pool, &end_with(run_id, RunOutcome::SandboxLost), &config())
        .await
        .unwrap();
    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(
        (state.status.as_str(), state.resume_at, state.auto_resumes),
        ("cancelled", None, 0)
    );
}

#[tokio::test]
async fn a_lost_sandbox_resumes_after_the_configured_resume_backoff() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    let mut config = config();
    config.scheduling.resume_backoff_seconds = 5;

    let before = Utc::now();
    end_run(&pool, &end_with(run_id, RunOutcome::SandboxLost), &config)
        .await
        .unwrap();
    let after = Utc::now();

    let state = task_resume_state(&pool, env.task.id).await;
    assert_eq!(state.status, "queued");
    let resume_at = state.resume_at.expect("a resume time");
    assert!(
        resume_at >= before + TimeDelta::seconds(5) && resume_at <= after + TimeDelta::seconds(6),
        "resume at {resume_at}, ended between {before} and {after}"
    );
}

/// A task of project `DF` in `status`, due at `due` (`resume_at`, or `updated_at` when None).
async fn due_task(env: &Env, key: &str, status: &str, resume_at: &str) -> TaskId {
    let (task, _) = df_db::create_task(
        &env.ctx.pool,
        df_db::NewTask {
            project_id: env.project_id,
            persona_id: env.persona_id,
            title: Some(key.into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET status = $2, resume_at = now() + $3::interval WHERE id = $1")
        .bind(task.id.as_uuid())
        .bind(status)
        .bind(resume_at)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    task.id
}

async fn park_seed_task(env: &Env) {
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn only_due_tasks_start_and_a_rate_limited_one_needs_no_message() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    let due = due_task(&env, "due", "rate_limited", "-1 minute").await;
    due_task(&env, "later", "queued", "5 minutes").await;
    let started = start_queued_runs(&pool, 10, 4).await.unwrap();
    assert_eq!(started.iter().map(|r| r.task_id).collect::<Vec<_>>(), [due]);
    let state = task_resume_state(&pool, due).await;
    assert_eq!((state.status.as_str(), state.resume_at), ("running", None));
}

#[tokio::test]
async fn a_start_attempt_at_the_limit_releases_the_start_lock_at_once() {
    use sqlx::Connection as _;

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    due_task(&env, "waiting for a slot", "queued", "-1 hour").await;
    let busy = due_task(&env, "busy", "running", "0 seconds").await;
    env.insert_run(busy, 1, "running").await;

    // A pool whose connections never get back to idle: sqlx queues the rollback of a dropped
    // transaction and sends it only when the connection is used again, so a transaction that
    // the code did not end itself keeps its lock here.
    let stuck = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_release(|_, _| Box::pin(std::future::pending()))
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();

    assert!(start_queued_runs(&stuck, 10, 1).await.unwrap().is_empty());

    let mut other = sqlx::PgConnection::connect_with(&pool.connect_options())
        .await
        .unwrap();
    let mut tx = other.begin().await.unwrap();
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(START_LOCK_KEY)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(locked, "the start lock is still held after the attempt");
}

#[tokio::test]
async fn the_concurrency_limit_holds_further_tasks_and_the_oldest_resume_time_goes_first() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    let newer = due_task(&env, "newer", "queued", "-1 hour").await;
    let older = due_task(&env, "older", "rate_limited", "-3 hours").await;
    // One slot of two is taken.
    let other = due_task(&env, "busy", "running", "0 seconds").await;
    env.insert_run(other, 1, "running").await;

    let first = start_queued_runs(&pool, 10, 2).await.unwrap();
    assert_eq!(first.iter().map(|r| r.task_id).collect::<Vec<_>>(), [older]);
    // The limit is reached: nothing more starts until a run ends.
    assert!(start_queued_runs(&pool, 10, 2).await.unwrap().is_empty());
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE task_id = $1")
        .bind(other.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let second = start_queued_runs(&pool, 10, 2).await.unwrap();
    assert_eq!(
        second.iter().map(|r| r.task_id).collect::<Vec<_>>(),
        [newer]
    );
}

#[tokio::test]
async fn two_replicas_fill_the_last_slot_only_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    due_task(&env, "second", "queued", "0 seconds").await;
    for _ in 0..5 {
        sqlx::query("DELETE FROM runs")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET status = 'queued'")
            .execute(&pool)
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            start_queued_runs(&pool, 10, 1),
            start_queued_runs(&pool, 10, 1)
        );
        let started = a.unwrap().len() + b.unwrap().len();
        assert_eq!(started, 1);
    }
}

async fn status_of(pool: &PgPool, task_id: TaskId) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn deliver_all(pool: &PgPool, task_id: TaskId) {
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(task_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn completed_run_puts_the_task_in_review_or_queues_a_waiting_message() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    // The seeded task has an undelivered message.
    end_run(&pool, &end_with(run_id, RunOutcome::Completed), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "queued");

    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 2, "running").await;
    end_run(&pool, &end_with(run_id, RunOutcome::Completed), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "in_review");
}

#[tokio::test]
async fn run_end_keeps_a_task_a_human_marked_done() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let ended = end_run(&pool, &end_with(run_id, RunOutcome::Cancelled), &config())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.task_status, TaskStatus::Completed);
    assert_eq!(status_of(&pool, env.task.id).await, "completed");
}

#[tokio::test]
async fn run_end_of_a_task_marked_done_queues_a_message_that_arrived_since() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // The seeded message is still undelivered.
    let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    end_run(&pool, &end_with(run_id, RunOutcome::Cancelled), &config())
        .await
        .unwrap();
    assert_eq!(status_of(&pool, env.task.id).await, "queued");
}

#[tokio::test]
async fn message_queued_while_the_run_ends_is_not_stranded() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    // `queue_message`, not yet committed: the task row is locked and the message inserted.
    let mut queueing = pool.begin().await.unwrap();
    sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&mut *queueing)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO task_messages (id, task_id, author, content) VALUES ($1, $2, '{}', '[]')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(env.task.id.as_uuid())
    .execute(&mut *queueing)
    .await
    .unwrap();

    let ending = tokio::spawn({
        let pool = pool.clone();
        async move {
            let end = RunEnd {
                run_id,
                outcome: RunOutcome::EndTurn,
                detail: String::new(),
                retry_after: None,
                harness_session_id: String::new(),
                task_branch_sha: String::new(),
                wip_sha: String::new(),
                diverged: false,
                task_branch_pushed: false,
            };
            end_run(&pool, &end, &config()).await
        }
    });
    // The run end is now waiting for the task row.
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    queueing.commit().await.unwrap();
    ending.await.unwrap().unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "queued");
}

/// The `pr.sync` jobs of the database as `(open, ready set, run set)`.
async fn pr_sync_jobs(pool: &PgPool) -> Vec<(bool, bool, bool)> {
    let payloads: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE kind = 'pr.sync' ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap();
    payloads
        .iter()
        .map(|p| {
            (
                p["open"].as_bool().unwrap(),
                !p["ready"].is_null(),
                !p["run_id"].is_null(),
            )
        })
        .collect()
}

/// Ends a fresh run of a new env with `outcome`, `pushed` and the task status set before.
async fn end_run_of(
    outcome: RunOutcome,
    pushed: bool,
    task_status: Option<&str>,
) -> Vec<(bool, bool, bool)> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    if let Some(status) = task_status {
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    let end = RunEnd {
        task_branch_pushed: pushed,
        ..end_with(run_id, outcome)
    };
    end_run(&pool, &end, &config()).await.unwrap();
    pr_sync_jobs(&pool).await
}

#[tokio::test]
async fn a_completed_run_that_pushed_enqueues_open_and_ready() {
    assert_eq!(
        end_run_of(RunOutcome::Completed, true, None).await,
        [(true, true, true)]
    );
}

#[tokio::test]
async fn a_completed_run_without_a_push_enqueues_ready_only() {
    assert_eq!(
        end_run_of(RunOutcome::Completed, false, None).await,
        [(false, true, true)]
    );
}

#[tokio::test]
async fn a_failed_run_that_pushed_enqueues_open_only() {
    assert_eq!(
        end_run_of(RunOutcome::Failed, true, None).await,
        [(true, false, true)]
    );
}

#[tokio::test]
async fn a_failed_run_without_a_push_enqueues_no_pr_job() {
    assert!(end_run_of(RunOutcome::Failed, false, None).await.is_empty());
}

#[tokio::test]
async fn the_end_of_a_run_of_a_cancelled_task_enqueues_no_pr_job() {
    assert!(
        end_run_of(RunOutcome::Completed, true, Some("cancelled"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_completion_releases_the_dependent_and_wakes_the_parent_in_one_transaction() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    deliver_all(&pool, env.task.id).await;
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let new_task = |title: &str, parent: Option<TaskId>, depends_on: Vec<df_db::NewDependency>| {
        df_db::NewTask {
            project_id: env.project_id,
            persona_id: env.persona_id,
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: parent,
            depends_on,
        }
    };
    let (child, _) = df_db::create_task(&pool, new_task("Implement", Some(env.task.id), vec![]))
        .await
        .unwrap();
    let on_child = vec![df_db::NewDependency {
        key: child.key.clone(),
        kind: df_core::DependencyKind::Completion,
    }];
    let (review, _) = df_db::create_task(&pool, new_task("Review", Some(env.task.id), on_child))
        .await
        .unwrap();
    assert_eq!(review.status, TaskStatus::AwaitingDependencies);
    deliver_all(&pool, child.id).await;
    let run_id = env.insert_run(child.id, 1, "running").await;
    sqlx::query("UPDATE tasks SET status = 'running' WHERE id = $1")
        .bind(child.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    end_run(&pool, &end_with(run_id, RunOutcome::Completed), &config())
        .await
        .unwrap();

    assert_eq!(status_of(&pool, child.id).await, "in_review");
    assert_eq!(status_of(&pool, review.id).await, "queued");
    assert_eq!(status_of(&pool, env.task.id).await, "queued");
    let woken: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_messages
         WHERE task_id = $1 AND author->>'reason' = 'task.child_ended'",
    )
    .bind(env.task.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(woken, 1);
}
