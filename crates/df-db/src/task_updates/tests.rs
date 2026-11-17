//! Tests of the task field updates (container-backed).

use sqlx::PgPool;

use super::*;
use crate::notify::Notifier;
use crate::tasks::tests::seed_project_and_persona;
use crate::{NewTask, TaskRow, create_task, get_task};

async fn new_task(pool: &PgPool) -> TaskRow {
    let (project_id, persona_id) = seed_project_and_persona(pool, &"DF".parse().unwrap(), "Linus")
        .await
        .unwrap();
    create_task(
        pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("T".into()),
            description: String::new(),
            issue_number: None,
            created_by: df_core::Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap()
    .0
}

async fn task(pool: &PgPool, id: TaskId) -> TaskRow {
    get_task(pool, id).await.unwrap().unwrap()
}

#[tokio::test]
async fn pr_draft_fields_keep_the_unset_value_and_report_the_pr_number() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let t = new_task(&pool).await;

    assert_eq!(
        set_pr_draft_fields(&pool, t.id, Some("Title"), None)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        set_pr_draft_fields(&pool, t.id, None, Some("Body"))
            .await
            .unwrap(),
        None
    );
    let stored = task(&pool, t.id).await;
    assert_eq!(
        (stored.pr_title.as_deref(), stored.pr_body.as_deref()),
        (Some("Title"), Some("Body"))
    );

    let mut tx = pool.begin().await.unwrap();
    record_pr(&mut tx, t.id, 7, "https://forge/pr/7", false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        set_pr_draft_fields(&pool, t.id, Some("Newer"), None)
            .await
            .unwrap(),
        Some(7)
    );
}

#[tokio::test]
async fn a_status_line_is_stored_and_published() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let t = new_task(&pool).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut rx = notifier.subscribe();

    set_status_line(&pool, t.id, "compiling").await.unwrap();

    assert_eq!(task(&pool, t.id).await.status_line, "compiling");
    assert_eq!(
        rx.recv().await.unwrap(),
        Notification::TaskChanged { task_id: t.id }
    );
}

#[tokio::test]
async fn marking_a_task_diverged_publishes_task_changed_on_commit() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let t = new_task(&pool).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut rx = notifier.subscribe();

    let mut tx = pool.begin().await.unwrap();
    mark_diverged(&mut tx, t.id).await.unwrap();
    tx.commit().await.unwrap();

    assert!(task(&pool, t.id).await.diverged);
    assert_eq!(
        rx.recv().await.unwrap(),
        Notification::TaskChanged { task_id: t.id }
    );
}

#[tokio::test]
async fn a_second_pr_does_not_overwrite_the_first() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let t = new_task(&pool).await;

    let mut tx = pool.begin().await.unwrap();
    let first = record_pr(&mut tx, t.id, 1, "https://forge/pr/1", false)
        .await
        .unwrap();
    let second = record_pr(&mut tx, t.id, 2, "https://forge/pr/2", true)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(first);
    assert!(!second);
    let stored = task(&pool, t.id).await;
    assert_eq!(
        (
            stored.pr_number,
            stored.pr_url.as_deref(),
            stored.pr_attached
        ),
        (Some(1), Some("https://forge/pr/1"), false)
    );
}

#[tokio::test]
async fn the_task_pr_lock_is_held_until_its_transaction_ends() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let t = new_task(&pool).await;

    let held = lock_task_pr(&pool, t.id).await.unwrap();
    let mut other = pool.begin().await.unwrap();
    let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("task-pr:{}", t.id))
        .fetch_one(&mut *other)
        .await
        .unwrap();
    assert!(!got, "the lock is held");

    held.rollback().await.unwrap();
    let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("task-pr:{}", t.id))
        .fetch_one(&mut *other)
        .await
        .unwrap();
    assert!(got, "the lock is free again");
}
