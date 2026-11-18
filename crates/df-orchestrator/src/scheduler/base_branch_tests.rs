//! The base branch of a task with a base task is created when its first run starts
//! (`task-tree` "Base branch created on first start").

use super::*;
use crate::testing::Env;
use df_core::testing::{ForgeCall, SandboxCall};
use df_core::{IntegrationError, TaskId, UpstreamError};
use df_db::TaskRow;
use sqlx::PgPool;

async fn tick(scheduler: &Scheduler) {
    let mut provisions = JoinSet::new();
    scheduler.tick(&mut provisions, Utc::now()).await.unwrap();
    while provisions.join_next().await.is_some() {}
}

async fn set_status(pool: &PgPool, task: TaskId, status: &str) {
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(task.as_uuid())
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
}

/// `DF-2` based on `DF-1`; only `DF-2` is due.
async fn based_task(pool: &PgPool, env: &Env) -> TaskRow {
    set_status(pool, env.task.id, "in_review").await;
    env.task_based_on(&env.task).await
}

fn creations(env: &Env) -> Vec<(String, String)> {
    env.forge
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            ForgeCall::CreateBranch { name, from, .. } => Some((name, from)),
            _ => None,
        })
        .collect()
}

fn provisioned(env: &Env) -> bool {
    env.provider
        .calls()
        .iter()
        .any(|call| matches!(call, SandboxCall::Provision(_)))
}

async fn run_of(pool: &PgPool, task: TaskId) -> RunRow {
    let id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1 ORDER BY number DESC LIMIT 1")
            .bind(task.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    get_run(pool, RunId::from_uuid(id)).await.unwrap().unwrap()
}

// Scenario "Coordinator without commits".
#[tokio::test]
async fn the_first_run_creates_the_base_tasks_branch_from_its_own_base() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let based = based_task(&pool, &env).await;

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert_eq!(
        creations(&env),
        [("factory/task/DF-1".to_string(), "main".to_string())]
    );
    assert!(provisioned(&env));
    assert_eq!(run_of(&pool, based.id).await.state, RunState::Provisioning);
}

// Scenario "Branch already pushed".
#[tokio::test]
async fn an_existing_base_branch_is_left_as_it_is() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let based = based_task(&pool, &env).await;
    env.forge
        .push_create_branch_result(Err(IntegrationError::Conflict("branch exists".into())));

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert!(provisioned(&env));
    assert_eq!(run_of(&pool, based.id).await.state, RunState::Provisioning);
}

#[tokio::test]
async fn a_forge_error_ends_the_run_failed_with_the_forges_reason() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let based = based_task(&pool, &env).await;
    env.forge
        .push_create_branch_result(Err(IntegrationError::Upstream(UpstreamError {
            status: 502,
            message: "bad gateway".into(),
        })));

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert!(!provisioned(&env));
    let run = run_of(&pool, based.id).await;
    assert_eq!(
        (run.state, run.outcome),
        (RunState::Failed, Some(RunOutcome::Failed))
    );
    assert!(
        run.detail.starts_with("base branch:") && run.detail.contains("bad gateway"),
        "{}",
        run.detail
    );
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(based.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "failed");
}

#[tokio::test]
async fn a_start_after_a_forge_failure_creates_the_branch() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let based = based_task(&pool, &env).await;
    env.forge
        .push_create_branch_result(Err(IntegrationError::Upstream(UpstreamError {
            status: 502,
            message: "bad gateway".into(),
        })));
    tick(&Scheduler::new(env.ctx.clone())).await;
    assert_eq!(run_of(&pool, based.id).await.state, RunState::Failed);
    assert!(!provisioned(&env));

    // A message resumes the failed task: the next start asks the forge again.
    let actor = env.human();
    df_db::queue_message(
        &pool,
        based.id,
        actor,
        vec![df_db::text_block("retry".into(), false)],
    )
    .await
    .unwrap();
    tick(&Scheduler::new(env.ctx.clone())).await;

    assert_eq!(run_of(&pool, based.id).await.number, 2);
    assert_eq!(creations(&env).len(), 2);
    assert!(provisioned(&env));
}

#[tokio::test]
async fn a_done_base_task_needs_no_branch() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let based = based_task(&pool, &env).await;
    set_status(&pool, env.task.id, "completed").await;

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert!(creations(&env).is_empty());
    assert!(provisioned(&env));
    assert_eq!(run_of(&pool, based.id).await.state, RunState::Provisioning);
}

#[tokio::test]
async fn a_task_without_base_task_creates_no_branch() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert!(creations(&env).is_empty());
    assert!(provisioned(&env));
}
