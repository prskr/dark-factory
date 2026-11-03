//! Tests of the lifecycle events the scheduler, the run end and the volume deletions emit.

use chrono::{DateTime, TimeDelta, Utc};
use df_core::testing::SandboxCall;
use df_core::{ProviderError, RunId, RunOutcome, SandboxEvent};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::task::JoinSet;

use crate::retention::Retention;
use crate::runs::{RunEnd, end_run};
use crate::testing::{Env, recorded_events};
use crate::{ChannelConfig, Scheduler};

/// One scheduler pass: start and provision what is queued.
async fn tick(scheduler: &Scheduler) {
    let mut provisions = JoinSet::new();
    scheduler.start_in_background(&mut provisions).await;
    while provisions.join_next().await.is_some() {}
}

async fn env(pool: &PgPool) -> Env {
    Env::new(pool.clone()).await.with_events()
}

fn types(events: &[Value]) -> Vec<&str> {
    events.iter().filter_map(|e| e["type"].as_str()).collect()
}

fn of_type<'a>(events: &'a [Value], event_type: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["type"] == event_type).collect()
}

fn run_of(event: &Value) -> &Value {
    &event["data"]["run"]["id"]
}

async fn only_run_id(pool: &PgPool) -> RunId {
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM runs")
        .fetch_one(pool)
        .await
        .unwrap();
    RunId::from_uuid(id)
}

async fn run_times(pool: &PgPool, run_id: RunId) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT provisioning_started_at, sandbox_started_at FROM runs WHERE id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn volume_provisioned_at(env: &Env) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT volume_provisioned_at FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
}

/// Ends the runs of the task and queues a message, so the next tick starts another run.
async fn queue_another_run(env: &Env) {
    sqlx::query("UPDATE runs SET state = 'succeeded', ended_at = now() WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    df_db::queue_message(&env.ctx.pool, env.task.id, env.human(), vec![])
        .await
        .unwrap();
}

/// A run in `state` whose provisioning began and, if `started`, whose sandbox is up.
async fn begun_run(env: &Env, state: &str, started: bool) -> RunId {
    let run_id = env.insert_run(env.task.id, 1, state).await;
    sqlx::query(
        "UPDATE runs SET provisioning_started_at = now() - interval '2 minutes',
             sandbox_started_at = CASE WHEN $2 THEN now() - interval '1 minute' END
         WHERE id = $1",
    )
    .bind(run_id.as_uuid())
    .bind(started)
    .execute(&env.ctx.pool)
    .await
    .unwrap();
    run_id
}

fn config_of(env: &Env) -> &ChannelConfig {
    &env.ctx.config
}

#[tokio::test]
async fn first_schedule_of_a_task_emits_volume_provisioned_with_the_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    tick(&Scheduler::new(env.ctx.clone())).await;

    let run_id = only_run_id(&pool).await;
    let events = recorded_events(&pool).await;
    let provisioned = of_type(&events, "volume.provisioned");
    assert_eq!(provisioned.len(), 1);
    assert_eq!(run_of(provisioned[0]), &json!(run_id.as_uuid()));
    assert!(volume_provisioned_at(&env).await.is_some());
}

#[tokio::test]
async fn a_second_run_of_a_task_emits_no_volume_provisioned() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler).await;
    queue_another_run(&env).await;
    tick(&scheduler).await;

    let events = recorded_events(&pool).await;
    assert_eq!(of_type(&events, "sandbox.provisioning").len(), 2);
    assert_eq!(of_type(&events, "volume.provisioned").len(), 1);
}

#[tokio::test]
async fn a_backfilled_task_emits_no_volume_provisioned() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    sqlx::query("UPDATE tasks SET volume_provisioned_at = now() WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    tick(&Scheduler::new(env.ctx.clone())).await;

    let events = recorded_events(&pool).await;
    assert_eq!(types(&events), ["sandbox.provisioning"]);
}

#[tokio::test]
async fn provisioning_is_announced_once_before_the_volume_and_the_sandbox() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler).await;
    tick(&scheduler).await;

    let run_id = only_run_id(&pool).await;
    assert!(run_times(&pool, run_id).await.0.is_some());
    let events = recorded_events(&pool).await;
    assert_eq!(
        types(&events),
        ["sandbox.provisioning", "volume.provisioned"]
    );
    assert_eq!(run_of(&events[0]), &json!(run_id.as_uuid()));
    assert!(matches!(
        env.provider.calls().first(),
        Some(SandboxCall::EnsureTaskVolume(_))
    ));
}

#[tokio::test]
async fn sandbox_start_emits_provisioned_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler).await;
    let run_id = only_run_id(&pool).await;

    let started = SandboxEvent::Started {
        run_id,
        pod_ip: "10.0.0.7".parse().unwrap(),
    };
    scheduler.handle_event(started.clone()).await.unwrap();
    scheduler.handle_event(started).await.unwrap();

    assert!(run_times(&pool, run_id).await.1.is_some());
    let events = recorded_events(&pool).await;
    let provisioned = of_type(&events, "sandbox.provisioned");
    assert_eq!(provisioned.len(), 1);
    assert_eq!(run_of(provisioned[0]), &json!(run_id.as_uuid()));
}

#[tokio::test]
async fn failed_provisioning_emits_provisioning_failed_with_the_error_and_no_destroyed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    env.provider
        .push_provision_result(Err(ProviderError::Backend("api unavailable".into())));
    tick(&Scheduler::new(env.ctx.clone())).await;

    let events = recorded_events(&pool).await;
    assert_eq!(
        types(&events),
        [
            "sandbox.provisioning",
            "volume.provisioned",
            "sandbox.provisioning_failed"
        ]
    );
    let error = events[2]["data"]["error"].as_str().unwrap();
    assert!(error.contains("api unavailable"), "{error}");
}

#[tokio::test]
async fn ending_a_started_run_emits_destroyed_with_outcome_and_times() {
    for outcome in [
        RunOutcome::Completed,
        RunOutcome::ConnectTimeout,
        RunOutcome::SandboxLost,
    ] {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = env(&pool).await;
        let run_id = begun_run(&env, "running", true).await;
        end_run(
            &pool,
            &RunEnd::failure(run_id, outcome, ""),
            config_of(&env),
        )
        .await
        .unwrap();

        let events = recorded_events(&pool).await;
        assert_eq!(types(&events), ["sandbox.destroyed"], "{outcome:?}");
        let data = &events[0]["data"];
        assert_eq!(data["outcome"], json!(outcome), "{outcome:?}");
        assert!(data["started_at"].is_string(), "{outcome:?}");
        assert!(data["ended_at"].is_string(), "{outcome:?}");
        assert_eq!(run_of(&events[0]), &json!(run_id.as_uuid()));
    }
}

#[tokio::test]
async fn a_run_cancelled_while_provisioning_is_destroyed_without_a_start() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let run_id = begun_run(&env, "provisioning", false).await;
    let end = RunEnd::failure(run_id, RunOutcome::Cancelled, "");
    end_run(&pool, &end, config_of(&env)).await.unwrap();

    let events = recorded_events(&pool).await;
    assert_eq!(types(&events), ["sandbox.destroyed"]);
    assert_eq!(events[0]["data"]["started_at"], Value::Null);
}

#[tokio::test]
async fn a_run_that_was_never_picked_up_emits_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let run_id = env.insert_run(env.task.id, 1, "pending").await;
    let end = RunEnd::failure(run_id, RunOutcome::Cancelled, "");
    end_run(&pool, &end, config_of(&env)).await.unwrap();

    assert_eq!(recorded_events(&pool).await, Vec::<Value>::new());
}

#[tokio::test]
async fn every_announced_sandbox_gets_exactly_one_closing_event() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    // Run 1: the sandbox comes up and is lost, reported twice.
    tick(&scheduler).await;
    let first = only_run_id(&pool).await;
    let pod_ip = "10.0.0.7".parse().unwrap();
    scheduler
        .handle_event(SandboxEvent::Started {
            run_id: first,
            pod_ip,
        })
        .await
        .unwrap();
    for reason in ["pod deleted", "again"] {
        let gone = SandboxEvent::Gone {
            run_id: first,
            reason: reason.into(),
        };
        scheduler.handle_event(gone).await.unwrap();
    }
    // Run 2: the provider rejects the sandbox.
    queue_another_run(&env).await;
    env.provider
        .push_provision_result(Err(ProviderError::Backend("api unavailable".into())));
    tick(&scheduler).await;

    let events = recorded_events(&pool).await;
    let announced: Vec<&Value> = of_type(&events, "sandbox.provisioning")
        .into_iter()
        .map(run_of)
        .collect();
    assert_eq!(announced.len(), 2);
    for run in announced {
        let closing = events
            .iter()
            .filter(|e| {
                (e["type"] == "sandbox.destroyed" || e["type"] == "sandbox.provisioning_failed")
                    && run_of(e) == run
            })
            .count();
        assert_eq!(closing, 1, "run {run}");
    }
}

#[tokio::test]
async fn retention_emits_volume_destroyed_and_the_next_run_provisions_again() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler).await;
    sqlx::query(
        "UPDATE runs SET state = 'succeeded', started_at = now() - interval '40 days',
             ended_at = now() - interval '40 days'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET status = 'completed'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        1
    );

    let events = recorded_events(&pool).await;
    let destroyed = of_type(&events, "volume.destroyed");
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0]["data"]["reason"], "retention");
    assert_eq!(destroyed[0]["data"].get("run"), None);
    assert_eq!(volume_provisioned_at(&env).await, None);
    queue_another_run(&env).await;
    tick(&scheduler).await;
    assert_eq!(
        of_type(&recorded_events(&pool).await, "volume.provisioned").len(),
        2
    );
}

#[tokio::test]
async fn reset_workspace_emits_volume_destroyed_and_the_next_run_provisions_again() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler).await;
    queue_another_run(&env).await;
    df_db::volumes::reset_workspace(
        &pool,
        env.ctx.provider.as_ref(),
        &env.ctx.config.events,
        env.task.id,
        env.human(),
        df_db::volumes::DELETE_TIMEOUT,
    )
    .await
    .unwrap();

    let events = recorded_events(&pool).await;
    let destroyed = of_type(&events, "volume.destroyed");
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0]["data"]["reason"], "reset");
    assert_eq!(destroyed[0]["data"].get("run"), None);
    assert_eq!(volume_provisioned_at(&env).await, None);
    tick(&scheduler).await;
    assert_eq!(
        of_type(&recorded_events(&pool).await, "volume.provisioned").len(),
        2
    );
}

#[tokio::test]
async fn deleting_a_volume_that_was_never_provisioned_emits_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = env(&pool).await;
    let ended = Utc::now() - TimeDelta::days(40);
    let run_id = env.insert_run(env.task.id, 1, "succeeded").await;
    sqlx::query("UPDATE runs SET started_at = $2, ended_at = $2 WHERE id = $1")
        .bind(run_id.as_uuid())
        .bind(ended)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(recorded_events(&pool).await, Vec::<Value>::new());
}
