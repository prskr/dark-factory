//! Chats in the scheduler (`specs/chats/spec.md`): their own limit and archived chats.

use super::*;
use crate::testing::Env;
use df_core::testing::SandboxCall;
use df_core::{TaskId, TaskStatus};
use df_db::chats::{archive_chat, unarchive_chat};
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

async fn runs_of(pool: &PgPool, task: TaskId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM runs WHERE task_id = $1")
        .bind(task.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_queued_chat_gets_a_run_and_a_volume_named_after_its_key() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    set_status(&pool, env.task.id, "waiting").await;
    let chat = env.chat().await;

    tick(&Scheduler::new(env.ctx.clone())).await;

    assert_eq!(runs_of(&pool, chat.id).await, 1);
    let calls = env.provider.calls();
    assert!(
        matches!(&calls[0], SandboxCall::EnsureTaskVolume(v) if v.volume_name == "task-df-2-chat"),
        "{calls:?}"
    );
}

#[tokio::test]
async fn archived_chat_starts_no_run_for_a_child_ended_message_until_it_is_unarchived() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    set_status(&pool, env.task.id, "waiting").await;
    let chat = env.chat().await;
    set_status(&pool, chat.id, "waiting").await;
    archive_chat(&pool, chat.id, env.human()).await.unwrap();
    // A subtask of the chat is marked done while the chat is archived.
    let (subtask, _) = df_db::create_task(
        &pool,
        df_db::NewTask {
            project_id: env.project_id,
            persona_id: env.persona_id,
            title: Some("Sub".into()),
            description: String::new(),
            issue_number: None,
            created_by: df_core::Principal::system("test"),
            parent_id: Some(chat.id),
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    set_status(&pool, subtask.id, "waiting").await;
    df_db::mark_task_done(&pool, subtask.id, env.human())
        .await
        .unwrap();
    let scheduler = Scheduler::new(env.ctx.clone());

    tick(&scheduler).await;

    assert_eq!(runs_of(&pool, chat.id).await, 0);
    let queued = df_db::get_task(&pool, chat.id).await.unwrap().unwrap();
    assert_eq!(queued.status, TaskStatus::Queued, "the message is queued");
    assert!(queued.archived_at.is_some());

    unarchive_chat(&pool, chat.id, env.human()).await.unwrap();
    tick(&scheduler).await;

    assert_eq!(runs_of(&pool, chat.id).await, 1);
}
