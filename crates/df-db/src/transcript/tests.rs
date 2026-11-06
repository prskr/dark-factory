//! Tests of the transcript writes of the control plane (container-backed).

use df_proto::transcript::v1::{
    BranchPushed, ForgeActivity, PushVerdict, event::Kind, forge_activity,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::*;

/// Seeds a project, persona and task; returns the task and a function-local run factory input.
async fn seed_task(pool: &PgPool) -> TaskId {
    let (project, persona, task) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, 'DF', 'DF', 'https://forge.example.test/o/r.git', 'img', 'main')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id) VALUES ($1, 'Linus', '', 'claude-code', 'test-model', (SELECT id FROM model_providers LIMIT 1))",
    )
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(task)
    .bind(project)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    TaskId::from_uuid(task)
}

async fn seed_run(pool: &PgPool, task: TaskId, number: i32) -> RunId {
    let run = Uuid::now_v7();
    sqlx::query("INSERT INTO runs (id, task_id, number) VALUES ($1, $2, $3)")
        .bind(run)
        .bind(task.as_uuid())
        .bind(number)
        .execute(pool)
        .await
        .unwrap();
    RunId::from_uuid(run)
}

fn pushed() -> Event {
    forge_event(ForgeActivity {
        kind: Some(forge_activity::Kind::BranchPushed(BranchPushed {
            r#ref: "refs/heads/factory/task/DF-1".into(),
            old_sha: "0".repeat(40),
            new_sha: "a".repeat(40),
            verdict: PushVerdict::Accepted.into(),
            reason: String::new(),
        })),
    })
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transcript_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn transcript_row_needs_exactly_one_of_seq_and_source_key() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;
    let insert = |seq: Option<i64>, key: Option<&'static str>| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO transcript_events (task_id, run_id, seq, source_key, occurred_at, kind, payload)
                 VALUES ($1, $2, $3, $4, now(), 'x', '{}')",
            )
            .bind(task.as_uuid())
            .bind(run.as_uuid())
            .bind(seq)
            .bind(key)
            .execute(&pool)
            .await
        }
    };

    assert!(insert(Some(1), None).await.is_ok());
    assert!(insert(None, Some("k")).await.is_ok());
    assert!(insert(Some(2), Some("k2")).await.is_err(), "both set");
    assert!(insert(None, None).await.is_err(), "neither set");
    assert!(insert(None, Some("k")).await.is_err(), "duplicate key");
}

#[tokio::test]
async fn control_event_is_stored_once_per_source_key_as_forge_activity() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let first = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    let again = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(first.is_some());
    assert_eq!(again, None);
    assert_eq!(count(&pool).await, 1);
    let (kind, key, seq, payload): (String, Option<String>, Option<i64>, serde_json::Value) =
        sqlx::query_as("SELECT kind, source_key, seq, payload FROM transcript_events")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (kind.as_str(), key.as_deref(), seq),
        ("forge_activity", Some("push:1"), None)
    );
    let stored: Event = serde_json::from_value(payload).unwrap();
    assert!(matches!(stored.kind, Some(Kind::ForgeActivity(_))));
}

#[tokio::test]
async fn control_event_publishes_its_id_only_when_inserted() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener
        .listen(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let id = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let first = listener.recv().await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(first.payload()).unwrap();
    assert_eq!(payload["id"], id);
    // The duplicate published nothing: the next notification is a marker we publish ourselves.
    sqlx::query("SELECT pg_notify($1, 'marker')")
        .bind(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(listener.recv().await.unwrap().payload(), "marker");
}

#[tokio::test]
async fn control_event_waits_for_the_task_lock_so_ids_become_visible_in_order() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let writer = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
            let id = append_control_event(&mut tx, &lock, "push:1", &pushed())
                .await
                .unwrap();
            tx.commit().await.unwrap();
            id
        })
    };
    // Wait on the condition: a backend waits for a lock.
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        assert!(
            !writer.is_finished(),
            "writer did not wait for the task lock"
        );
        tokio::task::yield_now().await;
    }
    assert_eq!(count(&pool).await, 0);
    blocker.commit().await.unwrap();

    assert!(writer.await.unwrap().is_some());
    assert_eq!(count(&pool).await, 1);
}
