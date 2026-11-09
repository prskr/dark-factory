//! Tests of `interrupt_turn` (`specs/run-orchestration/spec.md`, "Interrupting a turn from the
//! control plane").

use std::time::Duration;

use df_core::TaskStatus;
use sqlx::PgPool;
use uuid::Uuid;

use super::tests::{audit_count, human, status_of, task_with_run};
use super::*;
use crate::notify::Notifier;

/// Stores a turn event (`turn_started` or `turn_ended`) of `turn` as the runtime's `seq`-th event.
async fn turn_event(pool: &PgPool, task: TaskId, run: Uuid, seq: i64, kind: &str, turn: &str) {
    sqlx::query(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
         VALUES ($1, $2, $3, now(), $4, $5)",
    )
    .bind(task.as_uuid())
    .bind(run)
    .bind(seq)
    .bind(kind)
    .bind(serde_json::json!({"schemaVersion": 1, "turnId": turn}))
    .execute(pool)
    .await
    .unwrap();
}

async fn run_of(pool: &PgPool, task: TaskId) -> Uuid {
    sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1")
        .bind(task.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn interrupting_a_running_turn_is_audited_and_notified_with_the_actor() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::Running, "running").await;
    let run = run_of(&pool, task.id).await;
    turn_event(&pool, task.id, run, 1, "turn_ended", "turn_1").await;
    turn_event(&pool, task.id, run, 2, "turn_started", "turn_2").await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut events = notifier.subscribe();

    interrupt_turn(&pool, task.id, human()).await.unwrap();

    let interrupt = Notification::RunInterrupt {
        run_id: RunId::from_uuid(run),
        by: "Ada".into(),
        turn_id: "turn_2".into(),
    };
    loop {
        let seen = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the interrupt notification arrives")
            .unwrap();
        if seen == interrupt {
            break;
        }
    }
    assert_eq!(audit_count(&pool, "run.interrupt").await, 1);
    let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE id = $1")
        .bind(run)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "running");
    // The task keeps its status.
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Running);
}

#[tokio::test]
async fn interrupting_without_a_running_turn_changes_nothing() {
    for run_state in ["awaiting_input", "preparing", "finalizing", "succeeded"] {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task_with_run(&pool, TaskStatus::Running, run_state).await;
        let err = interrupt_turn(&pool, task.id, human()).await;
        assert!(matches!(err, Err(DbError::NoTurnRunning)), "{run_state}");
        assert_eq!(audit_count(&pool, "run.interrupt").await, 0, "{run_state}");
        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Running);
    }
}

#[tokio::test]
async fn a_running_run_whose_turn_has_ended_or_not_started_cannot_be_interrupted() {
    // After `TurnEnded` the run stays `running` while the runtime pushes; before the first
    // `TurnStarted` is stored there is no turn to name.
    for ended in [true, false] {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task_with_run(&pool, TaskStatus::Running, "running").await;
        let run = run_of(&pool, task.id).await;
        if ended {
            turn_event(&pool, task.id, run, 1, "turn_started", "turn_1").await;
            turn_event(&pool, task.id, run, 2, "turn_ended", "turn_1").await;
        }
        let err = interrupt_turn(&pool, task.id, human()).await;
        assert!(matches!(err, Err(DbError::NoTurnRunning)), "ended: {ended}");
        assert_eq!(
            audit_count(&pool, "run.interrupt").await,
            0,
            "ended: {ended}"
        );
    }
}
