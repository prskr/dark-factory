use super::*;

async fn run(pool: &PgPool) -> Uuid {
    let provider = crate::model_providers::tests::seed_provider(pool).await;
    let (project, persona, task, run) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'DF', 'P', 'https://g/o/r.git', 'img')")
        .bind(project)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO personas (id, name, system_prompt, model, model_provider_id) VALUES ($1, 'p', 'x', 'test-model', $2)")
        .bind(persona)
        .bind(provider)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by) VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')")
        .bind(task)
        .bind(project)
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number, state, outcome, ended_at) VALUES ($1, $2, 1, 'succeeded', 'completed', now() - interval '1 minute')")
        .bind(run)
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
    run
}

fn record(run_id: Uuid) -> UsageRecord {
    UsageRecord {
        request_id: Uuid::now_v7(),
        run_id,
        recorded_at: Utc::now(),
        model: Some("claude-sonnet-5-5".into()),
        wire_api: WireApi::AnthropicMessages,
        upstream_status: Some(200),
        usage_known: true,
        input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

#[tokio::test]
async fn a_retried_report_is_stored_once() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run(&pool).await;
    let record = record(run);

    assert_eq!(insert_usage(&pool, &record).await.unwrap(), Inserted::New);
    assert_eq!(
        insert_usage(&pool, &record).await.unwrap(),
        Inserted::Duplicate
    );

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM model_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn a_record_for_a_run_that_ended_a_minute_ago_is_stored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run(&pool).await;

    assert_eq!(
        insert_usage(&pool, &record(run)).await.unwrap(),
        Inserted::New
    );
}

#[tokio::test]
async fn a_record_for_an_unknown_run_is_not_stored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    let outcome = insert_usage(&pool, &record(Uuid::now_v7())).await.unwrap();

    assert_eq!(outcome, Inserted::UnknownRun);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM model_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn a_request_without_usage_is_stored_as_unknown() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run(&pool).await;
    let unknown = UsageRecord {
        model: None,
        upstream_status: None,
        usage_known: false,
        input_tokens: 0,
        output_tokens: 0,
        ..record(run)
    };

    insert_usage(&pool, &unknown).await.unwrap();

    let known: bool = sqlx::query_scalar("SELECT usage_known FROM model_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!known);
}
