//! Tests of the tool call claims (container-backed).

use serde_json::json;
use sqlx::PgPool;

use super::*;
use crate::tasks::tests::{insert_run, seed_project_and_persona};

async fn run_of_a_new_task(pool: &PgPool) -> RunId {
    let (project_id, persona_id) = seed_project_and_persona(pool, &"DF".parse().unwrap(), "Linus")
        .await
        .unwrap();
    let (task, _) = crate::create_task(
        pool,
        crate::NewTask {
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
    .unwrap();
    insert_run(pool, task.id).await
}

#[tokio::test]
async fn the_first_claim_wins_and_a_repeat_is_refused() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run_of_a_new_task(&pool).await;
    let call = Uuid::now_v7();

    assert!(
        claim(&pool, call, run, "report_progress", &json!({"a": 1}))
            .await
            .unwrap()
    );
    assert!(
        !claim(&pool, call, run, "report_progress", &json!({"a": 2}))
            .await
            .unwrap()
    );

    let stored = get(&pool, call).await.unwrap();
    assert_eq!(stored.run_id, run);
    assert_eq!(stored.result, None);
    assert!(!stored.abandoned);
}

#[tokio::test]
async fn the_first_result_is_kept() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run_of_a_new_task(&pool).await;
    let call = Uuid::now_v7();
    claim(&pool, call, run, "t", &json!({})).await.unwrap();

    store_result(&pool, call, &json!({"answer": 1}))
        .await
        .unwrap();
    store_result(&pool, call, &json!({"answer": 2}))
        .await
        .unwrap();

    assert_eq!(
        get(&pool, call).await.unwrap().result,
        Some(json!({"answer": 1}))
    );
}

#[tokio::test]
async fn an_old_unanswered_claim_is_abandoned() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run_of_a_new_task(&pool).await;
    let call = Uuid::now_v7();
    claim(&pool, call, run, "t", &json!({})).await.unwrap();
    sqlx::query("UPDATE tool_calls SET created_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();

    assert!(get(&pool, call).await.unwrap().abandoned);
}

#[tokio::test]
async fn an_unclaimed_call_is_an_error() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    assert!(get(&pool, Uuid::now_v7()).await.is_err());
}
