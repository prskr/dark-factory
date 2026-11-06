//! Tests of the volume retention.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use df_core::testing::{FakeSandboxProvider, SandboxCall};
use df_core::{ProviderError, TaskId};
use sqlx::PgPool;
use tokio::task::JoinSet;

use super::*;
use crate::Scheduler;
use crate::testing::Env;
use crate::testing::gated::{Gate, GatedProvider};

fn days_ago(days: i64) -> DateTime<Utc> {
    Utc::now() - TimeDelta::days(days)
}

async fn set_status(pool: &PgPool, task_id: TaskId, status: &str) {
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
}

/// A `succeeded` run that ended at `ended_at`.
async fn ended_run(env: &Env, task_id: TaskId, number: i64, ended_at: DateTime<Utc>) {
    let run_id = env.insert_run(task_id, number, "succeeded").await;
    sqlx::query("UPDATE runs SET started_at = $2, ended_at = $2 WHERE id = $1")
        .bind(run_id.as_uuid())
        .bind(ended_at)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
}

async fn volume_deleted_at(pool: &PgPool, task_id: TaskId) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT volume_deleted_at FROM tasks WHERE id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn run_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn deleted_volumes(provider: &FakeSandboxProvider) -> Vec<String> {
    provider
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            SandboxCall::DeleteTaskVolume(volume) => Some(volume.volume_name),
            _ => None,
        })
        .collect()
}

/// Starts and provisions the queued runs, as one scheduler tick does.
async fn scheduler_tick(scheduler: &Scheduler) {
    let mut provisions = JoinSet::new();
    scheduler.start_in_background(&mut provisions).await;
    while provisions.join_next().await.is_some() {}
}

#[tokio::test]
async fn closed_tasks_lose_their_volume_after_the_grace_period_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let df2 = env.second_task().await;
    set_status(&pool, env.task.id, "completed").await;
    set_status(&pool, df2, "cancelled").await;
    ended_run(&env, env.task.id, 1, days_ago(8)).await;
    ended_run(&env, df2, 1, days_ago(8)).await;
    let retention = Retention::new(env.ctx.clone());

    assert_eq!(retention.scan(Utc::now()).await.unwrap(), 2);
    assert_eq!(retention.scan(Utc::now()).await.unwrap(), 0);

    assert_eq!(deleted_volumes(&env.provider), ["task-df-1", "task-df-2"]);
    assert!(volume_deleted_at(&pool, env.task.id).await.is_some());
    assert!(volume_deleted_at(&pool, df2).await.is_some());
}

#[tokio::test]
async fn merged_task_loses_its_volume_without_waiting_for_the_grace_period() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let df2 = env.second_task().await;
    set_status(&pool, env.task.id, "completed").await;
    ended_run(&env, env.task.id, 1, Utc::now()).await;
    sqlx::query("UPDATE tasks SET pr_merged_at = now() WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    // A merged task with an active run keeps its volume until the run ended.
    sqlx::query("UPDATE tasks SET pr_merged_at = now() WHERE id = $1")
        .bind(df2.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    env.insert_run(df2, 1, "running").await;

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(deleted_volumes(&env.provider), ["task-df-1"]);
}

#[tokio::test]
async fn volumes_within_their_retention_are_kept() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let df2 = env.second_task().await;
    set_status(&pool, env.task.id, "completed").await;
    set_status(&pool, df2, "waiting").await;
    ended_run(&env, env.task.id, 1, days_ago(6)).await;
    ended_run(&env, df2, 1, days_ago(29)).await;

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        0
    );
    assert!(deleted_volumes(&env.provider).is_empty());
}

#[tokio::test]
async fn task_without_runs_is_never_a_candidate() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    set_status(&pool, env.task.id, "completed").await;
    env.ctx.config = Arc::new(crate::ChannelConfig {
        volumes: df_core::config::VolumesConfig {
            retain_after_close_seconds: 0,
            idle_ttl_seconds: 0,
        },
        ..crate::ChannelConfig::default()
    });

    let scanned = Retention::new(env.ctx.clone())
        .scan(Utc::now() + TimeDelta::days(1))
        .await
        .unwrap();

    assert_eq!(scanned, 0);
    assert!(deleted_volumes(&env.provider).is_empty());
}

#[tokio::test]
async fn idle_task_loses_its_volume_and_the_next_message_runs_on_a_fresh_one() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    set_status(&pool, env.task.id, "waiting").await;
    ended_run(&env, env.task.id, 1, days_ago(31)).await;

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        1
    );
    assert!(volume_deleted_at(&pool, env.task.id).await.is_some());

    df_db::queue_message(&pool, env.task.id, env.human(), vec![])
        .await
        .unwrap();
    scheduler_tick(&Scheduler::new(env.ctx.clone())).await;

    let calls = env.provider.calls();
    assert!(matches!(&calls[0], SandboxCall::DeleteTaskVolume(v) if v.volume_name == "task-df-1"));
    assert!(matches!(&calls[1], SandboxCall::EnsureTaskVolume(v) if v.volume_name == "task-df-1"));
    assert!(matches!(&calls[2], SandboxCall::Provision(spec) if spec.volume_name == "task-df-1"));
    assert_eq!(run_count(&pool).await, 2);
    assert_eq!(volume_deleted_at(&pool, env.task.id).await, None);
}

#[tokio::test]
async fn task_with_an_active_run_keeps_its_volume() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    set_status(&pool, env.task.id, "running").await;
    ended_run(&env, env.task.id, 1, days_ago(40)).await;
    env.insert_run(env.task.id, 2, "running").await;
    sqlx::query("UPDATE runs SET started_at = $1 WHERE number = 2")
        .bind(days_ago(31))
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        Retention::new(env.ctx.clone())
            .scan(Utc::now())
            .await
            .unwrap(),
        0
    );
    assert!(deleted_volumes(&env.provider).is_empty());
    assert_eq!(volume_deleted_at(&pool, env.task.id).await, None);
}

#[tokio::test]
async fn failed_deletion_keeps_the_task_unmarked_and_the_scan_goes_on() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let df2 = env.second_task().await;
    for task_id in [env.task.id, df2] {
        set_status(&pool, task_id, "waiting").await;
        ended_run(&env, task_id, 1, days_ago(31)).await;
    }
    env.provider
        .push_delete_volume_result(Err(ProviderError::Backend("storage down".into())));

    let deleted = Retention::new(env.ctx.clone())
        .scan(Utc::now())
        .await
        .unwrap();

    assert_eq!(deleted, 1);
    assert_eq!(deleted_volumes(&env.provider), ["task-df-1", "task-df-2"]);
    assert_eq!(volume_deleted_at(&pool, env.task.id).await, None);
    assert!(volume_deleted_at(&pool, df2).await.is_some());
}

/// The deletions of a scan stop at the shutdown: the one in progress commits, no other starts.
#[tokio::test]
async fn shutdown_during_a_deletion_commits_it_and_ends_the_loop() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    let df2 = env.second_task().await;
    for task_id in [env.task.id, df2] {
        set_status(&pool, task_id, "waiting").await;
        ended_run(&env, task_id, 1, days_ago(31)).await;
    }
    let (entered, release) = GatedProvider::install(&mut env, Gate::DeleteTaskVolume);
    let (_leader_tx, leader_rx) = watch::channel(true);
    let (stop_tx, stop_rx) = watch::channel(false);
    let retention = Retention::new(env.ctx.clone());
    let running = tokio::spawn(async move { retention.run(leader_rx, stop_rx).await });
    entered.notified().await;

    stop_tx.send_replace(true);
    release.notify_one();
    running.await.unwrap();

    assert_eq!(deleted_volumes(&env.provider), ["task-df-1"]);
    assert!(volume_deleted_at(&pool, env.task.id).await.is_some());
    assert_eq!(volume_deleted_at(&pool, df2).await, None);
}

#[tokio::test]
async fn run_start_waits_until_the_volume_deletion_has_committed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    // A message was queued after a month of idleness, just as the scan picked the task.
    ended_run(&env, env.task.id, 1, days_ago(31)).await;
    let (entered, release) = GatedProvider::install(&mut env, Gate::DeleteTaskVolume);
    let retention = Retention::new(env.ctx.clone());
    let scan = tokio::spawn(async move { retention.scan(Utc::now()).await });
    entered.notified().await;

    let scheduler = Scheduler::new(env.ctx.clone());
    scheduler_tick(&scheduler).await;
    assert_eq!(
        run_count(&pool).await,
        1,
        "no run while the volume is deleted"
    );

    release.notify_one();
    assert_eq!(scan.await.unwrap().unwrap(), 1);
    scheduler_tick(&scheduler).await;

    assert_eq!(run_count(&pool).await, 2);
    let calls = env.provider.calls();
    assert!(matches!(&calls[0], SandboxCall::DeleteTaskVolume(_)));
    assert!(matches!(&calls[1], SandboxCall::EnsureTaskVolume(v) if v.volume_name == "task-df-1"));
    assert_eq!(volume_deleted_at(&pool, env.task.id).await, None);
}

#[tokio::test]
async fn deletion_timeout_rolls_back_and_releases_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    ended_run(&env, env.task.id, 1, days_ago(31)).await;
    // Never released: the deletion hangs.
    let _gate = GatedProvider::install(&mut env, Gate::DeleteTaskVolume);
    let retention = Retention {
        delete_timeout: Duration::from_millis(50),
        ..Retention::new(env.ctx.clone())
    };

    assert_eq!(retention.scan(Utc::now()).await.unwrap(), 0);

    assert_eq!(volume_deleted_at(&pool, env.task.id).await, None);
    scheduler_tick(&Scheduler::new(env.ctx.clone())).await;
    assert_eq!(run_count(&pool).await, 2, "the task row is unlocked again");
}

#[tokio::test(start_paused = true)]
async fn scan_runs_once_per_hour_while_leader() {
    let (leader_tx, leader_rx) = watch::channel(true);
    let (_stop_tx, stop_rx) = watch::channel(false);
    let scans = Arc::new(AtomicUsize::new(0));
    let loop_handle = tokio::spawn({
        let scans = scans.clone();
        every_period_while_leader(SCAN_INTERVAL, leader_rx, stop_rx, move || {
            scans.fetch_add(1, Ordering::SeqCst);
            async {}
        })
    });
    let minutes = |m: u64| tokio::time::sleep(Duration::from_secs(m * 60));

    minutes(1).await;
    assert_eq!(scans.load(Ordering::SeqCst), 1, "first scan at once");
    minutes(58).await;
    assert_eq!(scans.load(Ordering::SeqCst), 1, "none within the hour");
    minutes(2).await;
    assert_eq!(scans.load(Ordering::SeqCst), 2, "second scan after an hour");

    leader_tx.send_replace(false);
    minutes(120).await;
    assert_eq!(
        scans.load(Ordering::SeqCst),
        2,
        "no scan without leadership"
    );

    drop(leader_tx);
    minutes(60).await;
    assert!(loop_handle.is_finished(), "the loop ends with the election");
    assert_eq!(loop_handle.await.unwrap(), LoopEnd::ElectionEnded);
}

#[tokio::test(start_paused = true)]
async fn the_wait_between_scans_ends_at_the_stop() {
    let (_leader_tx, leader_rx) = watch::channel(true);
    let (stop_tx, stop_rx) = watch::channel(false);
    let loop_handle = tokio::spawn(every_period_while_leader(
        SCAN_INTERVAL,
        leader_rx,
        stop_rx,
        || async {},
    ));
    tokio::time::sleep(Duration::from_secs(60)).await;

    stop_tx.send_replace(true);

    let ended = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    assert_eq!(ended.unwrap().unwrap(), LoopEnd::Stopped);
}
