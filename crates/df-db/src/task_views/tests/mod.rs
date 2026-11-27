//! Tests of the read models; the tests are in the topic modules.

use super::*;
use df_core::TaskStatus;

mod list;
mod tree;

async fn seed(pool: &PgPool) -> (TaskId, Uuid) {
    let project = Uuid::now_v7();
    let persona = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'DF', 'Dark Factory', 'https://g/o/r.git', 'img')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query("INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, model, model_provider_id) VALUES ($1, 'Linus', 'linus', 'linus@example.org', 'p', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    let (task, _) = crate::create_task(
        pool,
        crate::NewTask {
            project_id: df_core::ProjectId::from_uuid(project),
            persona_id: df_core::PersonaId::from_uuid(persona),
            title: Some("Fix login".into()),
            description: String::new(),
            issue_number: Some(7),
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    let run = Uuid::now_v7();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(run)
        .bind(task.id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    (task.id, run)
}

async fn insert_event(
    pool: &PgPool,
    task: TaskId,
    run: Uuid,
    seq: i64,
    kind: &str,
    payload: serde_json::Value,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
         VALUES ($1, $2, $3, now(), $4, $5) RETURNING id",
    )
    .bind(task.as_uuid())
    .bind(run)
    .bind(seq)
    .bind(kind)
    .bind(payload)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A gateway usage record; without a model its usage is unknown.
async fn insert_usage(
    pool: &PgPool,
    run: Uuid,
    model: Option<&str>,
    input: i64,
    output: i64,
    cache_read: i64,
) {
    sqlx::query(
        "INSERT INTO model_usage (request_id, run_id, recorded_at, model, wire_api, input_tokens, output_tokens, cache_read_tokens, usage_known)
         VALUES ($1, $2, now(), $3, 'anthropic-messages', $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(run)
    .bind(model)
    .bind(input)
    .bind(output)
    .bind(cache_read)
    .bind(model.is_some())
    .execute(pool)
    .await
    .unwrap();
}
