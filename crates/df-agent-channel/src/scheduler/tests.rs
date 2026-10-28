//! Tests of the run scheduler.

use super::*;
use crate::runs::get_run;
use crate::testing::Env;
use df_core::testing::SandboxCall;
use df_core::{ProviderError, TaskStatus};
use sqlx::PgPool;

/// One pass of the production code: start and provision what is queued, then enforce
/// timeouts.
async fn tick(scheduler: &Scheduler, now: DateTime<Utc>) -> Result<(), ChannelError> {
    let mut provisions = JoinSet::new();
    scheduler.start_in_background(&mut provisions).await;
    while provisions.join_next().await.is_some() {}
    scheduler.enforce_timeouts(now).await
}

async fn task_status(pool: &PgPool, env: &Env) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn only_run(pool: &PgPool) -> RunRow {
    let runs = list_runs(pool).await;
    assert_eq!(runs.len(), 1, "expected exactly one run");
    runs.into_iter().next().unwrap()
}

async fn list_runs(pool: &PgPool) -> Vec<RunRow> {
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM runs ORDER BY number")
        .fetch_all(pool)
        .await
        .unwrap();
    let mut runs = Vec::new();
    for id in ids {
        runs.push(
            get_run(pool, df_core::RunId::from_uuid(id))
                .await
                .unwrap()
                .unwrap(),
        );
    }
    runs
}

#[tokio::test]
async fn new_task_gets_a_run_and_a_sandbox() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();

    let run = only_run(&pool).await;
    assert_eq!((run.number, run.state), (1, RunState::Provisioning));
    assert_eq!(task_status(&pool, &env).await, "running");
    let calls = env.provider.calls();
    assert!(matches!(&calls[0], SandboxCall::EnsureTaskVolume(v) if v.volume_name == "task-df-1"));
    let spec = calls
        .iter()
        .find_map(|c| match c {
            SandboxCall::Provision(spec) => Some(spec),
            _ => None,
        })
        .expect("a provision call");
    assert_eq!(spec.run_id, run.id);
    assert_eq!(spec.image, "registry.example.org/df/project:latest");
    assert_eq!(spec.harness.as_deref(), Some("claude-code"));
    assert_eq!(spec.labels["dark-factory.io/task-key"], "DF-1");
}

#[tokio::test]
async fn two_schedulers_create_exactly_one_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let a = Scheduler::new(env.ctx.clone());
    let b = Scheduler::new(env.other_replica().await);
    let now = Utc::now();
    let (ra, rb) = tokio::join!(tick(&a, now), tick(&b, now));
    ra.unwrap();
    rb.unwrap();
    assert_eq!(list_runs(&pool).await.len(), 1);
    let provisions = env
        .provider
        .calls()
        .into_iter()
        .filter(|c| matches!(c, SandboxCall::Provision(_)))
        .count();
    assert_eq!(provisions, 1);
}

#[tokio::test]
async fn task_with_an_active_run_is_not_started_again() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    env.insert_run(env.task.id, 1, "running").await;
    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();
    assert_eq!(list_runs(&pool).await.len(), 1);
}

#[tokio::test]
async fn project_with_an_adapter_command_gets_no_harness_mount() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("UPDATE projects SET adapter_command = ARRAY['/opt/harness/bin/fake-acp-agent']")
        .execute(&pool)
        .await
        .unwrap();
    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();

    let spec = env
        .provider
        .calls()
        .into_iter()
        .find_map(|c| match c {
            SandboxCall::Provision(spec) => Some(spec),
            _ => None,
        })
        .expect("a provision call");
    assert_eq!(spec.harness, None);
}

#[tokio::test]
async fn missing_harness_image_ends_the_run_provisioning_failed_naming_the_harness() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    env.provider
        .push_provision_result(Err(ProviderError::InvalidSpec(
            "no image is configured for harness `claude-code`".into(),
        )));
    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();

    let run = only_run(&pool).await;
    assert_eq!(run.outcome, Some(RunOutcome::ProvisioningFailed));
    assert!(run.detail.contains("claude-code"), "{}", run.detail);
}

#[tokio::test]
async fn provider_rejection_fails_the_run_and_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    env.provider
        .push_provision_result(Err(ProviderError::Backend("quota exceeded".into())));
    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();

    let run = only_run(&pool).await;
    assert_eq!(run.state, RunState::Failed);
    assert_eq!(run.outcome, Some(RunOutcome::ProvisioningFailed));
    assert!(run.detail.contains("quota exceeded"));
    assert_eq!(task_status(&pool, &env).await, TaskStatus::Failed.as_str());
    assert!(
        env.provider
            .calls()
            .contains(&SandboxCall::Teardown(run.id))
    );
}

#[tokio::test]
async fn started_event_moves_the_run_to_connecting_with_the_pod_ip() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    tick(&scheduler, Utc::now()).await.unwrap();
    let run = only_run(&pool).await;

    let event = SandboxEvent::Started {
        run_id: run.id,
        pod_ip: "10.0.0.7".parse().unwrap(),
    };
    scheduler.handle_event(event.clone()).await.unwrap();
    scheduler.handle_event(event).await.unwrap(); // idempotent

    let run = only_run(&pool).await;
    assert_eq!(run.state, RunState::Connecting);
    assert_eq!(run.pod_ip.as_deref(), Some("10.0.0.7"));
}

#[tokio::test]
async fn gone_event_fails_a_running_run_and_keeps_the_volume() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    let scheduler = Scheduler::new(env.ctx.clone());

    scheduler
        .handle_event(SandboxEvent::Gone {
            run_id,
            reason: "pod deleted".into(),
        })
        .await
        .unwrap();

    let run = get_run(&pool, run_id).await.unwrap().unwrap();
    assert_eq!(
        (run.state, run.outcome),
        (RunState::Failed, Some(RunOutcome::SandboxLost))
    );
    assert_eq!(task_status(&pool, &env).await, "failed");
    // Only the sandbox is deleted, never the task volume.
    assert!(
        env.provider
            .calls()
            .iter()
            .all(|c| !matches!(c, SandboxCall::EnsureTaskVolume(_)))
    );

    // A second event (e.g. from another replica) changes nothing.
    scheduler
        .handle_event(SandboxEvent::Gone {
            run_id,
            reason: "again".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        env.provider
            .calls()
            .iter()
            .filter(|c| matches!(c, SandboxCall::Teardown(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn sandbox_that_never_connects_fails_with_connect_timeout() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let scheduler = Scheduler::new(env.ctx.clone());
    let now = Utc::now();
    tick(&scheduler, now).await.unwrap();

    scheduler
        .enforce_timeouts(now + chrono::Duration::seconds(599))
        .await
        .unwrap();
    assert_eq!(only_run(&pool).await.state, RunState::Provisioning);

    scheduler
        .enforce_timeouts(now + chrono::Duration::seconds(601))
        .await
        .unwrap();
    let run = only_run(&pool).await;
    assert_eq!(
        (run.state, run.outcome),
        (RunState::Failed, Some(RunOutcome::ConnectTimeout))
    );
    assert_eq!(task_status(&pool, &env).await, "failed");
    assert!(
        env.provider
            .calls()
            .contains(&SandboxCall::Teardown(run.id))
    );
}

#[tokio::test]
async fn run_past_budget_and_both_graces_is_torn_down() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    let scheduler = Scheduler::new(env.ctx.clone());
    let started = get_run(&pool, run_id).await.unwrap().unwrap().started_at;

    // 2 h budget + 2 x 5 min grace
    scheduler
        .enforce_timeouts(started + chrono::Duration::seconds(7200 + 599))
        .await
        .unwrap();
    assert_eq!(
        get_run(&pool, run_id).await.unwrap().unwrap().state,
        RunState::Running
    );

    scheduler
        .enforce_timeouts(started + chrono::Duration::seconds(7200 + 601))
        .await
        .unwrap();
    let run = get_run(&pool, run_id).await.unwrap().unwrap();
    assert_eq!(
        (run.state, run.outcome),
        (RunState::Failed, Some(RunOutcome::Failed))
    );
}

#[test]
fn cancel_comes_after_budget_plus_one_grace() {
    let cfg = RunsConfig::default();
    let started = Utc::now();
    let deadline = cancel_deadline(started, &cfg).unwrap();
    assert_eq!(deadline - started, chrono::Duration::seconds(7500));
}

#[test]
fn huge_budgets_mean_no_deadline_instead_of_a_panic() {
    let cfg = RunsConfig {
        max_wall_clock_seconds: u64::MAX,
        connect_timeout_seconds: u64::MAX,
        cancel_grace_seconds: u64::MAX,
        keep_alive_seconds: 600,
    };
    assert_eq!(cancel_deadline(Utc::now(), &cfg), None);
    assert_eq!(hard_stop(Utc::now(), &cfg), None);
    assert!(!is_before(after(Utc::now(), u64::MAX), Utc::now()));
}

#[tokio::test]
async fn enforcing_timeouts_with_unbounded_budgets_does_not_panic() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.config = std::sync::Arc::new(crate::ChannelConfig {
        runs: RunsConfig {
            max_wall_clock_seconds: u64::MAX,
            connect_timeout_seconds: u64::MAX,
            cancel_grace_seconds: u64::MAX,
            keep_alive_seconds: 600,
        },
        ..crate::ChannelConfig::default()
    });
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    Scheduler::new(env.ctx.clone())
        .enforce_timeouts(Utc::now() + chrono::Duration::days(3650))
        .await
        .unwrap();
    let run = get_run(&pool, run_id).await.unwrap().unwrap();
    assert_eq!(run.state, RunState::Running);
}

#[tokio::test]
async fn wall_clock_budget_counts_from_the_runtime_connect() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "running").await;
    let started = get_run(&pool, run_id).await.unwrap().unwrap().started_at;
    // The sandbox needed an hour to come up.
    sqlx::query("UPDATE runs SET connected_at = $2 WHERE id = $1")
        .bind(run_id.as_uuid())
        .bind(started + chrono::Duration::hours(1))
        .execute(&pool)
        .await
        .unwrap();
    let scheduler = Scheduler::new(env.ctx.clone());
    let state = || async { get_run(&pool, run_id).await.unwrap().unwrap().state };

    let before_stop = started + chrono::Duration::seconds(3600 + 7200 + 599);
    scheduler.enforce_timeouts(before_stop).await.unwrap();
    assert_eq!(state().await, RunState::Running);
    let after_stop = started + chrono::Duration::seconds(3600 + 7200 + 601);
    scheduler.enforce_timeouts(after_stop).await.unwrap();
    assert_eq!(state().await, RunState::Failed);
}

#[tokio::test]
async fn bookkeeping_failure_after_the_run_was_created_ends_it_as_provisioning_failed() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    // `load_run_context` joins `credential_refs`; without it, provisioning cannot start.
    sqlx::query("ALTER TABLE credential_refs RENAME TO credential_refs_gone")
        .execute(&pool)
        .await
        .unwrap();

    tick(&Scheduler::new(env.ctx.clone()), Utc::now())
        .await
        .unwrap();

    let run = only_run(&pool).await;
    assert_eq!(run.state, RunState::Failed);
    assert_eq!(run.outcome, Some(RunOutcome::ProvisioningFailed));
    assert_eq!(task_status(&pool, &env).await, "failed");
}

#[tokio::test]
async fn no_sandbox_is_created_for_a_run_that_ended_meanwhile() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "pending").await;
    let stale = get_run(&pool, run_id).await.unwrap().unwrap();
    // Another replica ended the run while this one was busy with the volume.
    sqlx::query("UPDATE runs SET state = 'failed' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    Scheduler::new(env.ctx.clone())
        .provision(stale)
        .await
        .unwrap();

    assert!(
        env.provider
            .calls()
            .iter()
            .all(|c| !matches!(c, SandboxCall::Provision(_)))
    );
}

#[tokio::test]
async fn an_unreadable_run_does_not_stop_the_timeouts_of_the_others() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let broken = env.insert_run(env.task.id, 1, "running").await;
    let second = df_db::create_task(
        &pool,
        df_db::NewTask {
            project_id: env.project_id,
            persona_id: env.persona_id,
            title: Some("Second".into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap()
    .0;
    let healthy = env.insert_run(second.id, 1, "running").await;
    sqlx::query("ALTER TABLE runs DROP CONSTRAINT runs_state_check")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE runs SET state = 'bogus' WHERE id = $1")
        .bind(broken.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    Scheduler::new(env.ctx.clone())
        .enforce_timeouts(Utc::now() + chrono::Duration::days(1))
        .await
        .unwrap();

    let run = get_run(&pool, healthy).await.unwrap().unwrap();
    assert_eq!(run.state, RunState::Failed);
}

#[tokio::test]
async fn run_loop_starts_runs_on_notification_and_applies_events() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let scheduler = Scheduler::new(env.ctx.clone());
    let handle = tokio::spawn(async move {
        scheduler
            .run(async {
                let _ = stop_rx.await;
            })
            .await;
    });

    // The loop's first interval tick starts the run; wait for the provision call.
    let run_id = loop {
        if let Some(SandboxCall::Provision(spec)) = env
            .provider
            .calls()
            .into_iter()
            .find(|c| matches!(c, SandboxCall::Provision(_)))
        {
            break spec.run_id;
        }
        tokio::task::yield_now().await;
    };
    env.provider.emit_event(SandboxEvent::Started {
        run_id,
        pod_ip: "10.0.0.9".parse().unwrap(),
    });
    loop {
        let run = get_run(&pool, run_id).await.unwrap().unwrap();
        if run.state == RunState::Connecting {
            break;
        }
        tokio::task::yield_now().await;
    }

    // A task created later starts on its notification, not on the 15 s tick.
    let second = df_db::create_task(
        &pool,
        df_db::NewTask {
            project_id: env.project_id,
            persona_id: env.persona_id,
            title: Some("Second".into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap()
    .0;
    loop {
        let provisioned = env
            .provider
            .calls()
            .into_iter()
            .filter(|c| matches!(c, SandboxCall::Provision(_)))
            .count();
        if provisioned == 2 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM tasks WHERE id = $1")
            .bind(second.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap(),
        "running"
    );
    let _ = stop_tx.send(());
    handle.await.unwrap();
}
