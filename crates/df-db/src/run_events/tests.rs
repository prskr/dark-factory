//! Tests of the run applier (container-backed): locking, writing, effects.

use std::time::Duration;

use df_core::run_transition::RunEnd;
use df_core::task_transition::{DoneBy, StopScope};
use df_core::{InstanceSubscription, LifecycleEventsConfig};
use jiff::SignedDuration;
use sqlx::PgPool;

use super::*;
use crate::notify::Notifier;
use crate::tasks::tests::{audit_count, status_of, task_with_run};

/// A task in `status` with a run in `state`, and the run's id.
async fn run_in(pool: &PgPool, status: TaskStatus, state: &str) -> (TaskId, RunId) {
    let task = task_with_run(pool, status, state).await;
    let run: Uuid = sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    (task.id, RunId::from_uuid(run))
}

async fn apply(
    pool: &PgPool,
    run_id: RunId,
    event: RunEvent,
    env: &RunEnv<'_>,
) -> Option<AppliedRun> {
    let mut tx = pool.begin().await.unwrap();
    let applied = apply_run_event(&mut tx, run_id, event, env).await.unwrap();
    tx.commit().await.unwrap();
    applied
}

async fn apply_plain(pool: &PgPool, run_id: RunId, event: RunEvent) -> AppliedRun {
    apply(pool, run_id, event, &RunEnv::none()).await.unwrap()
}

/// State and finalize reason of the run, as stored.
async fn stored(pool: &PgPool, run_id: RunId) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, finalize_reason FROM runs WHERE id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn failure(run_id: RunId, outcome: RunOutcome) -> RunEvent {
    RunEvent::Ended(RunEnd::failure(run_id, outcome, "why"))
}

fn instance_sinks() -> EventSinks {
    EventSinks::new(&LifecycleEventsConfig {
        signing_key_file: Some("/k/tls.key".into()),
        subscriptions: vec![InstanceSubscription {
            url: "https://hooks.example.org/x".into(),
            events: Vec::new(),
        }],
    })
}

async fn lifecycle_types(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT type FROM lifecycle_events ORDER BY occurred_at, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_event_is_decided_on_the_row_it_locked() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "running").await;

    // Another transaction moves the run to `finalizing` and still holds its row.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("UPDATE runs SET state = 'finalizing', finalize_reason = 'stop' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&mut *holder)
        .await
        .unwrap();

    let applying = tokio::spawn({
        let pool = pool.clone();
        async move { apply_plain(&pool, run_id, RunEvent::Finalize(FinalizeReason::Budget)).await }
    });
    // The applier waits for the row lock.
    df_test_postgres::wait_for_lock_wait(&pool).await;
    holder.commit().await.unwrap();

    let applied = applying.await.unwrap();
    assert!(
        !applied.changed,
        "the run was `finalizing` when it got the lock"
    );
    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("stop".into()))
    );
}

#[tokio::test]
async fn an_event_for_an_unknown_run_applies_to_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    assert_eq!(
        apply(&pool, RunId::new(), RunEvent::Connected, &RunEnv::none()).await,
        None
    );
}

#[tokio::test]
async fn the_events_of_a_start_write_state_and_lifecycle_columns() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "pending").await;

    let applied = apply_plain(&pool, run_id, RunEvent::ProvisioningStarted).await;
    assert_eq!(applied.state, RunState::Provisioning);
    let started = RunEvent::SandboxStarted {
        pod_ip: "10.1.2.3".into(),
    };
    assert!(apply_plain(&pool, run_id, started).await.changed);
    let applied = apply_plain(&pool, run_id, RunEvent::Connected).await;
    assert_eq!(applied.state, RunState::Preparing);
    // Repeated: nothing changes.
    assert!(
        !apply_plain(&pool, run_id, RunEvent::Connected)
            .await
            .changed
    );

    let (pod_ip, provisioning, sandbox, connected): (String, bool, bool, bool) = sqlx::query_as(
        "SELECT pod_ip, provisioning_started_at IS NOT NULL, sandbox_started_at IS NOT NULL,
                connected_at IS NOT NULL FROM runs WHERE id = $1",
    )
    .bind(run_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pod_ip, "10.1.2.3");
    assert!(provisioning && sandbox && connected);
    assert_eq!(stored(&pool, run_id).await.0, "preparing");
}

#[tokio::test]
async fn a_finalize_stores_its_reason_and_an_end_stores_the_outcome() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Waiting, "awaiting_input").await;

    let applied = apply_plain(&pool, run_id, RunEvent::Finalize(FinalizeReason::Idle)).await;
    assert!(applied.changed);
    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("idle".into()))
    );

    let mut end = RunEnd::failure(run_id, RunOutcome::Cancelled, "task marked done");
    end.task_branch_sha = "abc".into();
    let sinks = EventSinks::default();
    let env = RunEnv::new(&sinks, 3, SignedDuration::from_mins(2));
    let applied = apply(&pool, run_id, RunEvent::Ended(end), &env)
        .await
        .unwrap();
    assert_eq!(applied.state, RunState::Cancelled);
    assert!(applied.task_status.is_some());
    let (outcome, detail, sha, ended): (String, String, String, bool) = sqlx::query_as(
        "SELECT outcome, detail, task_branch_sha, ended_at IS NOT NULL FROM runs WHERE id = $1",
    )
    .bind(run_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (outcome.as_str(), detail.as_str(), sha.as_str(), ended),
        ("cancelled", "task marked done", "abc", true)
    );
    // The reason stays with the run it explains.
    assert_eq!(stored(&pool, run_id).await.1.as_deref(), Some("idle"));
    // Ended once: a second end changes nothing.
    let again = apply_plain(&pool, run_id, failure(run_id, RunOutcome::Failed)).await;
    assert!(!again.changed);
}

#[tokio::test]
async fn a_stop_stamps_the_run_and_a_terminal_run_conflicts() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "running").await;

    let stop = RunEvent::StopRequested(StopScope::ActiveOnly);
    assert!(apply_plain(&pool, run_id, stop.clone()).await.changed);
    let stamped: bool =
        sqlx::query_scalar("SELECT stop_requested_at IS NOT NULL FROM runs WHERE id = $1")
            .bind(run_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(stamped);
    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("stop".into()))
    );

    sqlx::query("UPDATE runs SET state = 'failed' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = apply_run_event(&mut tx, run_id, stop, &RunEnv::none()).await;
    assert!(matches!(result, Err(DbError::Conflict(_))), "{result:?}");
}

#[tokio::test]
async fn a_turn_event_raises_the_task_event_in_the_same_transaction() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task_id, run_id) = run_in(&pool, TaskStatus::Running, "running").await;

    let mut tx = pool.begin().await.unwrap();
    apply_run_event(&mut tx, run_id, RunEvent::TurnEnded, &RunEnv::none())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(stored(&pool, run_id).await.0, "running");
    assert_eq!(status_of(&pool, task_id).await, TaskStatus::Running);

    apply_plain(&pool, run_id, RunEvent::TurnEnded).await;
    assert_eq!(stored(&pool, run_id).await.0, "awaiting_input");
    assert_eq!(status_of(&pool, task_id).await, TaskStatus::Waiting);
    apply_plain(&pool, run_id, RunEvent::TurnStarted).await;
    assert_eq!(status_of(&pool, task_id).await, TaskStatus::Running);
}

#[tokio::test]
async fn run_changed_is_published_only_for_a_change() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task_id, run_id) = run_in(&pool, TaskStatus::Running, "running").await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notes = notifier.subscribe();

    // No change: the run is `running` already.
    assert!(
        !apply_plain(&pool, run_id, RunEvent::TurnStarted)
            .await
            .changed
    );
    assert!(
        apply_plain(&pool, run_id, RunEvent::TurnEnded)
            .await
            .changed
    );
    // The marker follows both applications in the same channel order.
    publish(&pool, &Notification::TaskMessageQueued { task_id })
        .await
        .unwrap();

    let mut run_changed = 0;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), notes.recv())
            .await
            .unwrap()
            .unwrap()
        {
            Notification::RunChanged { .. } => run_changed += 1,
            Notification::TaskMessageQueued { .. } => break,
            _ => {}
        }
    }
    assert_eq!(run_changed, 1);
}

#[tokio::test]
async fn the_sandbox_lifecycle_events_follow_the_run() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "pending").await;
    let sinks = instance_sinks();
    let env = RunEnv::new(&sinks, 3, SignedDuration::from_mins(2));

    apply(&pool, run_id, RunEvent::ProvisioningStarted, &env).await;
    let started = RunEvent::SandboxStarted {
        pod_ip: "ip".into(),
    };
    apply(&pool, run_id, started, &env).await;
    apply(&pool, run_id, failure(run_id, RunOutcome::Cancelled), &env).await;

    assert_eq!(
        lifecycle_types(&pool).await,
        [
            "sandbox.provisioning",
            "sandbox.provisioned",
            "sandbox.destroyed"
        ]
    );
}

#[tokio::test]
async fn a_run_whose_provisioning_failed_closes_with_provisioning_failed() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "pending").await;
    let sinks = instance_sinks();
    let env = RunEnv::new(&sinks, 3, SignedDuration::from_mins(2));

    apply(&pool, run_id, RunEvent::ProvisioningStarted, &env).await;
    let failed = failure(run_id, RunOutcome::ProvisioningFailed);
    apply(&pool, run_id, failed, &env).await;

    assert_eq!(
        lifecycle_types(&pool).await,
        ["sandbox.provisioning", "sandbox.provisioning_failed"]
    );
}

#[tokio::test]
async fn a_run_that_never_began_provisioning_enqueues_no_closing_event() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_, run_id) = run_in(&pool, TaskStatus::Running, "pending").await;
    let sinks = instance_sinks();
    let env = RunEnv::new(&sinks, 3, SignedDuration::from_mins(2));

    apply(&pool, run_id, failure(run_id, RunOutcome::Cancelled), &env).await;

    assert!(lifecycle_types(&pool).await.is_empty());
}

#[tokio::test]
async fn marking_a_task_done_stores_done_as_the_reason_and_raises_no_further_task_event() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task_id, run_id) = run_in(&pool, TaskStatus::Waiting, "awaiting_input").await;

    let mut tx = pool.begin().await.unwrap();
    apply_task_event(
        &mut tx,
        task_id,
        TaskEvent::MarkedDone(DoneBy::Board),
        &Principal::system("test"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("done".into()))
    );
    assert_eq!(status_of(&pool, task_id).await, TaskStatus::Completed);
    // One audit entry, the board's: the run event raised nothing on the task.
    assert_eq!(audit_count(&pool, "task.done").await, 1);
}

#[tokio::test]
async fn marking_a_task_done_takes_over_a_run_finalizing_for_idle() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task_id, run_id) = run_in(&pool, TaskStatus::Waiting, "finalizing").await;
    sqlx::query("UPDATE runs SET finalize_reason = 'idle' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    apply_task_event(
        &mut tx,
        task_id,
        TaskEvent::MarkedDone(DoneBy::Board),
        &Principal::system("test"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("done".into()))
    );
}

#[tokio::test]
async fn cancelling_a_task_stores_stop_as_the_reason_of_its_run() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task_id, run_id) = run_in(&pool, TaskStatus::Running, "running").await;

    crate::cancel_task(&pool, task_id, Principal::system("test"))
        .await
        .unwrap();

    assert_eq!(
        stored(&pool, run_id).await,
        ("finalizing".into(), Some("stop".into()))
    );
}
