//! The migration `persona_identity`: handles, display names and emails of existing personas
//! (`specs/persona-identity/spec.md`, "Identity migration").

use sqlx::PgPool;

const VERSION: i64 = 20_261_124_000_001;

/// A database migrated to just before the persona identity, with a model provider.
async fn before() -> (PgPool, uuid::Uuid) {
    let pool = df_test_postgres::empty_pool().await;
    let previous = crate::MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .filter(|version| *version < VERSION)
        .max()
        .unwrap();
    crate::MIGRATOR.run_to(previous, &pool).await.unwrap();
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    (pool, provider)
}

/// Inserts personas named `names` in this creation order, migrates to the latest version.
async fn migrated_with(names: &[&str]) -> PgPool {
    let (pool, provider) = before().await;
    for (i, name) in names.iter().enumerate() {
        sqlx::query(
            "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id, created_at)
             VALUES ($1, $2, 'p', 'claude-code', 'test-model', $3, now() + make_interval(secs => $4))",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(name)
        .bind(provider)
        .bind(f64::from(u32::try_from(i).unwrap()))
        .execute(&pool)
        .await
        .unwrap();
    }
    crate::MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn identities(pool: &PgPool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT handle, display_name, commit_email FROM personas ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn handles(names: &[&str]) -> Vec<String> {
    let pool = migrated_with(names).await;
    identities(&pool).await.into_iter().map(|i| i.0).collect()
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[tokio::test]
async fn a_name_with_spaces_becomes_a_slug_handle_and_a_placeholder_email() {
    let pool = migrated_with(&["Code Reviewer"]).await;

    assert_eq!(
        identities(&pool).await,
        [(
            "code-reviewer".to_owned(),
            "Code Reviewer".to_owned(),
            "code-reviewer@dark-factory.localhost".to_owned()
        )]
    );
}

#[tokio::test]
async fn colliding_slugs_get_suffixes_in_creation_order() {
    assert_eq!(handles(&["Ada", "ADA!"]).await, owned(&["ada", "ada-2"]));
}

#[tokio::test]
async fn a_suffixed_handle_that_collides_with_another_slug_gets_the_next_suffix() {
    assert_eq!(
        handles(&["Ada", "ADA!", "Ada-2"]).await,
        owned(&["ada", "ada-2", "ada-2-2"])
    );
}

#[tokio::test]
async fn a_long_slug_is_cut_so_the_suffix_fits() {
    let long = "a".repeat(39);

    let handles = handles(&[&long, &format!("{long}!")]).await;

    assert_eq!(handles, [long, format!("{}-2", "a".repeat(37))]);
}

#[tokio::test]
async fn a_name_without_usable_characters_becomes_persona() {
    assert_eq!(
        handles(&["🤖", "🤖🤖"]).await,
        owned(&["persona", "persona-2"])
    );
}

#[tokio::test]
async fn a_name_that_is_no_valid_display_name_is_cleaned() {
    let pool = migrated_with(&["<Bot>", "<<>>", &format!("N{}", "x".repeat(120))]).await;

    let rows = identities(&pool).await;

    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), ("bot", "Bot"));
    assert_eq!(
        (rows[1].0.as_str(), rows[1].1.as_str()),
        ("persona", "persona")
    );
    assert_eq!(rows[2].1.chars().count(), 100);
}

#[tokio::test]
async fn migrated_runs_carry_the_identity_of_their_persona() {
    let (pool, provider) = before().await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id)
         VALUES ('0192a000-0000-7000-8000-000000000001', 'Linus', 'p', 'claude-code', 'test-model', $1)",
    )
    .bind(provider)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ('0192a000-0000-7000-8000-000000000002', 'DF', 'DF', 'https://example.org/r.git', 'i')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ('0192a000-0000-7000-8000-000000000003', '0192a000-0000-7000-8000-000000000002', 1, 'DF-1', 't', 'user',
                 '0192a000-0000-7000-8000-000000000001', '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number) VALUES (gen_random_uuid(), '0192a000-0000-7000-8000-000000000003', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    crate::MIGRATOR.run(&pool).await.unwrap();

    let identity: (String, String) = sqlx::query_as("SELECT commit_name, commit_email FROM runs")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        identity,
        ("Linus".into(), "linus@dark-factory.localhost".into())
    );
}
