//! The migration `provider_models` (iter3-opencode-harness design D4).

use sqlx::PgPool;
use uuid::Uuid;

const VERSION: i64 = 20_261_105_000_001;

/// A database migrated to just before the provider models, with a Claude subscription provider
/// and the persona "Linus" (model `model`, or none).
async fn before(model: Option<&str>) -> (PgPool, Uuid) {
    let pool = df_test_postgres::empty_pool().await;
    let previous = crate::MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .filter(|version| *version < VERSION)
        .max()
        .unwrap();
    crate::MIGRATOR.run_to(previous, &pool).await.unwrap();
    let credential = Uuid::now_v7();
    let provider = Uuid::now_v7();
    sqlx::query("INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, 'c', 's', 'k')")
        .bind(credential)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme) \
         VALUES ($1, 'Claude subscription', $2, 'claude-subscription')",
    )
    .bind(provider)
    .bind(credential)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url) \
         VALUES ($1, 'anthropic-messages', 'https://api.anthropic.com')",
    )
    .bind(provider)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id) \
         VALUES ($1, 'Linus', 'p', 'claude-code', $2, $3)",
    )
    .bind(Uuid::now_v7())
    .bind(model)
    .bind(provider)
    .execute(&pool)
    .await
    .unwrap();
    (pool, provider)
}

#[tokio::test]
async fn migrated_persona_model_becomes_a_provider_model_the_persona_references() {
    let (pool, provider) = before(Some("claude-sonnet-5-5")).await;
    crate::MIGRATOR.run(&pool).await.unwrap();

    let model: (String, String, Option<i64>) = sqlx::query_as(
        "SELECT model_id, wire_api, context_window_tokens FROM model_provider_models \
         WHERE provider_id = $1",
    )
    .bind(provider)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        model,
        (
            "claude-sonnet-5-5".into(),
            "anthropic-messages".into(),
            None
        )
    );
    let removal = sqlx::query("DELETE FROM model_provider_models WHERE provider_id = $1")
        .bind(provider)
        .execute(&pool)
        .await;
    assert!(removal.is_err(), "the persona still references the model");
}

#[tokio::test]
async fn persona_without_a_model_fails_the_migration_naming_it() {
    let (pool, _) = before(None).await;
    let error = crate::MIGRATOR.run(&pool).await.unwrap_err();
    assert!(error.to_string().contains("Linus"), "{error}");
}

#[tokio::test]
async fn model_with_one_limit_is_rejected() {
    let (pool, provider) = before(Some("m")).await;
    crate::MIGRATOR.run(&pool).await.unwrap();
    let result = sqlx::query(
        "INSERT INTO model_provider_models (provider_id, model_id, wire_api, context_window_tokens) \
         VALUES ($1, 'other', 'anthropic-messages', 1000)",
    )
    .bind(provider)
    .execute(&pool)
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn model_on_an_api_the_provider_lacks_is_rejected() {
    let (pool, provider) = before(Some("m")).await;
    crate::MIGRATOR.run(&pool).await.unwrap();
    let result = sqlx::query(
        "INSERT INTO model_provider_models (provider_id, model_id, wire_api) \
         VALUES ($1, 'other', 'openai-chat')",
    )
    .bind(provider)
    .execute(&pool)
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn personas_accept_the_opencode_harness() {
    let (pool, provider) = before(Some("m")).await;
    crate::MIGRATOR.run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id) \
         VALUES ($1, 'Ada', 'p', 'opencode', 'm', $2)",
    )
    .bind(Uuid::now_v7())
    .bind(provider)
    .execute(&pool)
    .await
    .unwrap();
}
