//! Tests of the start query for chats: their own limit, archived chats and the release of idle
//! chat runs (container-backed).

use std::time::Duration;

use super::*;
use crate::chats::{NewChat, create_chat};
use crate::notify::Notifier;

/// A chat of the seeded project in `status`.
async fn chat_in(env: &Env, status: &str) -> TaskId {
    let (chat, _) = create_chat(
        &env.pool,
        NewChat {
            project_id: env.project_id,
            persona_id: env.persona_id,
            author: env.human(),
            text: "Plan the release".into(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(chat.id.as_uuid())
        .bind(status)
        .execute(&env.pool)
        .await
        .unwrap();
    chat.id
}

/// A chat whose run waits for the next message since `minutes` minutes.
async fn idle_chat(env: &Env, minutes: i32) -> (TaskId, RunId) {
    let chat = chat_in(env, "waiting").await;
    let run = env.insert_run(chat, 1, "awaiting_input").await;
    sqlx::query("UPDATE runs SET idle_since = now() - make_interval(mins => $2) WHERE id = $1")
        .bind(run.as_uuid())
        .bind(minutes)
        .execute(&env.pool)
        .await
        .unwrap();
    (chat, run)
}

async fn running_chat(env: &Env) {
    let chat = chat_in(env, "running").await;
    env.insert_run(chat, 1, "running").await;
}

fn task_ids(runs: &[RunRow]) -> Vec<TaskId> {
    runs.iter().map(|run| run.task_id).collect()
}

/// The notifications published while `start` ran: everything before a sentinel published after
/// it.
async fn notifications_during(pool: &PgPool, start: impl Future<Output = ()>) -> Vec<Notification> {
    let notifier = Notifier::start(pool).await.unwrap();
    let mut events = notifier.subscribe();
    start.await;
    let sentinel = Notification::TaskChanged {
        task_id: TaskId::new(),
    };
    publish(pool, &sentinel).await.unwrap();
    let mut seen = Vec::new();
    loop {
        let next = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        if next == sentinel {
            return seen;
        }
        seen.push(next);
    }
}

fn released(seen: &[Notification]) -> Vec<RunId> {
    seen.iter()
        .filter_map(|n| match n {
            Notification::ReleaseIdle { run_id } => Some(*run_id),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn task_runs_at_their_limit_do_not_block_a_queued_chat() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    env.insert_run(env.task.id, 1, "running").await;
    let chat = chat_in(&env, "queued").await;

    let started = start_queued_runs(&pool, 10, 1, 2).await.unwrap();

    assert_eq!(task_ids(&started), [chat]);
}

#[tokio::test]
async fn chat_runs_in_a_turn_at_their_limit_keep_a_third_chat_queued_and_release_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    running_chat(&env).await;
    running_chat(&env).await;
    let third = chat_in(&env, "queued").await;

    let mut started = Vec::new();
    let seen = notifications_during(&pool, async {
        started = start_queued_runs(&pool, 10, 4, 2).await.unwrap();
    })
    .await;

    assert!(started.is_empty());
    assert!(released(&seen).is_empty(), "{seen:?}");
    assert_eq!(status_of(&pool, third).await, "queued");
}

#[tokio::test]
async fn a_chat_run_does_not_use_a_task_slot() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    running_chat(&env).await;

    let started = start_queued_runs(&pool, 10, 1, 2).await.unwrap();

    assert_eq!(task_ids(&started), [env.task.id]);
}

#[tokio::test]
async fn a_queued_chat_without_a_free_slot_releases_the_chat_run_idle_the_longest() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    idle_chat(&env, 2).await;
    let (_, longest) = idle_chat(&env, 8).await;
    let third = chat_in(&env, "queued").await;

    let mut started = Vec::new();
    let seen = notifications_during(&pool, async {
        started = start_queued_runs(&pool, 10, 4, 2).await.unwrap();
    })
    .await;

    assert!(
        started.is_empty(),
        "the slot is free only after the release"
    );
    assert_eq!(released(&seen), [longest]);
    assert_eq!(status_of(&pool, third).await, "queued");
}

#[tokio::test]
async fn a_released_chat_keeps_its_status_and_the_queued_chat_takes_its_slot() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    let (idle, run) = idle_chat(&env, 8).await;
    let queued = chat_in(&env, "queued").await;
    sqlx::query("UPDATE runs SET state = 'succeeded', outcome = 'end_turn' WHERE id = $1")
        .bind(run.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let started = start_queued_runs(&pool, 10, 4, 1).await.unwrap();

    assert_eq!(task_ids(&started), [queued]);
    assert_eq!(status_of(&pool, idle).await, "waiting");
}

#[tokio::test]
async fn an_archived_chat_with_a_queued_message_starts_no_run_until_it_is_unarchived() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    let chat = chat_in(&env, "queued").await;
    sqlx::query("UPDATE tasks SET archived_at = now(), archived_by = '{}'::jsonb WHERE id = $1")
        .bind(chat.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    assert!(start_queued_runs(&pool, 10, 4, 2).await.unwrap().is_empty());

    crate::chats::unarchive_chat(&pool, chat, env.human())
        .await
        .unwrap();
    let started = start_queued_runs(&pool, 10, 4, 2).await.unwrap();
    assert_eq!(task_ids(&started), [chat]);
}

#[tokio::test]
async fn an_idle_task_run_is_not_released_for_a_queued_chat() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    park_seed_task(&env).await;
    let idle_task = due_task(&env, "idle", "waiting", "0 seconds").await;
    env.insert_run(idle_task, 1, "awaiting_input").await;
    running_chat(&env).await;
    chat_in(&env, "queued").await;

    let seen = notifications_during(&pool, async {
        start_queued_runs(&pool, 10, 4, 1).await.unwrap();
    })
    .await;

    assert!(released(&seen).is_empty(), "{seen:?}");
}
