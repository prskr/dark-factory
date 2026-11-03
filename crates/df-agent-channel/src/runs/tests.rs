//! Tests of the run rows: start query, run end mapping and resume plan.

use super::*;
use crate::testing::{Env, task_resume_state};

/// `runs.maxAutoResumes` of the tests.
const MAX_AUTO_RESUMES: u32 = 3;

fn config() -> ChannelConfig {
    let mut config = ChannelConfig::default();
    config.runs.max_auto_resumes = MAX_AUTO_RESUMES;
    config
}

fn failure_of(outcome: RunOutcome, detail: &str) -> RunEnd {
    RunEnd::failure(RunId::new(), outcome, detail)
}

#[test]
fn an_infrastructure_failure_below_the_cap_resumes_with_a_growing_backoff() {
    let now = Utc::now();
    let end = failure_of(RunOutcome::SandboxLost, "");
    let plan = plan_resume(&end, None, 1, MAX_AUTO_RESUMES, now);
    assert_eq!(plan.status, TaskStatus::Queued);
    assert_eq!(plan.resume_at, Some(now + TimeDelta::minutes(4)));
    assert_eq!(plan.auto_resumes, 2);
}

#[test]
fn an_infrastructure_failure_at_the_cap_fails_the_task_and_keeps_the_count() {
    let now = Utc::now();
    let end = failure_of(RunOutcome::SandboxLost, "");
    let plan = plan_resume(&end, None, 3, MAX_AUTO_RESUMES, now);
    assert_eq!(
        (plan.status, plan.resume_at, plan.auto_resumes),
        (TaskStatus::Failed, None, 3)
    );
    assert_eq!(
        plan_resume(&end, None, 0, 0, now).status,
        TaskStatus::Failed
    );
}

#[test]
fn a_capacity_rejection_requeues_without_counting_a_resume() {
    let now = Utc::now();
    let end = failure_of(RunOutcome::ProvisioningFailed, CAPACITY_DETAIL);
    let plan = plan_resume(&end, None, 5, 0, now);
    assert_eq!((plan.status, plan.auto_resumes), (TaskStatus::Queued, 5));
    assert_eq!(plan.resume_at, Some(now + TimeDelta::minutes(2)));
}

#[test]
fn a_rate_limit_without_a_retry_time_resumes_in_an_hour_and_resets_the_count() {
    let now = Utc::now();
    let end = failure_of(RunOutcome::RateLimited, "");
    let plan = plan_resume(&end, None, 2, MAX_AUTO_RESUMES, now);
    assert_eq!(plan.resume_at, Some(now + TimeDelta::hours(1)));
    assert_eq!(plan.auto_resumes, 0);
}

#[test]
fn an_overridden_status_gets_no_resume_time_and_resets_the_count_unless_infrastructure_failed() {
    let now = Utc::now();
    let reset = plan_resume(
        &failure_of(RunOutcome::EndTurn, ""),
        Some(TaskStatus::Queued),
        2,
        MAX_AUTO_RESUMES,
        now,
    );
    assert_eq!(
        (reset.status, reset.resume_at, reset.auto_resumes),
        (TaskStatus::Queued, None, 0)
    );
    let kept = plan_resume(
        &failure_of(RunOutcome::SandboxLost, ""),
        Some(TaskStatus::Cancelled),
        2,
        MAX_AUTO_RESUMES,
        now,
    );
    assert_eq!(
        (kept.status, kept.resume_at, kept.auto_resumes),
        (TaskStatus::Cancelled, None, 2)
    );
}

#[test]
fn the_agent_completing_puts_the_task_in_review() {
    assert_eq!(task_status_for(RunOutcome::Completed), TaskStatus::InReview);
    assert_eq!(run_state_for(RunOutcome::Completed), RunState::Succeeded);
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
            created_by: None,
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
