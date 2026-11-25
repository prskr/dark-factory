use super::*;
use crate::catalog::{PersonaInput, save_persona};
use df_core::UserId;
use df_core::signing_key::SigningKey;
use df_core::testing::FakeSecretStore;
use secrecy::ExposeSecret;

async fn seeded() -> (PgPool, FakeSecretStore, Uuid, Principal) {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let principal = Principal::human(UserId::new(), "Peter");
    let roles = vec!["board-member".to_string()];
    let persona = save_persona(
        &pool,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        None,
        &PersonaInput {
            handle: "linus".into(),
            display_name: "Linus".into(),
            commit_email: "linus@example.org".into(),
            description: String::new(),
            system_prompt: "p".into(),
            harness: "claude-code".into(),
            model: "test-model".into(),
            model_provider_id: provider,
        },
    )
    .await
    .unwrap();
    (pool, FakeSecretStore::new(), persona.id, principal)
}

async fn stored(pool: &PgPool, id: Uuid) -> (Option<String>, Option<String>) {
    sqlx::query_as("SELECT signing_public_key, signing_fingerprint FROM personas WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit_details(pool: &PgPool, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_new_persona_has_no_key_and_sync_creates_it_once() {
    let (pool, secrets, id, _) = seeded().await;
    assert_eq!(stored(&pool, id).await, (None, None));

    let first = sync_signing_key(&pool, &secrets, id).await.unwrap();
    let again = sync_signing_key(&pool, &secrets, id).await.unwrap();

    assert_eq!(first, again);
    assert_eq!(
        stored(&pool, id).await,
        (
            Some(first.public_key.clone()),
            Some(first.fingerprint.clone())
        )
    );
    let created = secrets.created();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].0, secret_ref(PersonaId::from_uuid(id)));
    assert_eq!(created[0].1, secret_labels());
    let private = secrets.get(&created[0].0).await.unwrap();
    assert_eq!(
        SigningKey::from_openssh(&private).unwrap().public_key(),
        first.public_key
    );
    assert!(
        audit_details(&pool, "persona.signing_key.reconcile")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn rotation_replaces_secret_and_columns_and_audits_both_fingerprints() {
    let (pool, secrets, id, principal) = seeded().await;
    let old = sync_signing_key(&pool, &secrets, id).await.unwrap();
    let roles = vec!["board-member".to_string()];

    let new = rotate_signing_key(
        &pool,
        &secrets,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        id,
    )
    .await
    .unwrap();

    assert_ne!(new, old);
    assert_eq!(
        stored(&pool, id).await.0.as_deref(),
        Some(new.public_key.as_str())
    );
    let private = secrets
        .get(&secret_ref(PersonaId::from_uuid(id)))
        .await
        .unwrap();
    assert_eq!(
        SigningKey::from_openssh(&private).unwrap().fingerprint(),
        new.fingerprint
    );
    let audit = audit_details(&pool, "persona.signing_key.rotate").await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["old_fingerprint"], old.fingerprint);
    assert_eq!(audit[0]["new_fingerprint"], new.fingerprint);
    assert!(!audit[0].to_string().contains("PRIVATE"));
}

#[tokio::test]
async fn a_failed_secret_write_leaves_the_key_audit_and_database_as_they_were() {
    let (pool, secrets, id, principal) = seeded().await;
    let old = sync_signing_key(&pool, &secrets, id).await.unwrap();
    secrets.fail_replace(Some(SecretError::Backend("forbidden".into())));
    let roles = Vec::new();

    let err = rotate_signing_key(
        &pool,
        &secrets,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        id,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("forbidden"), "{err}");
    assert_eq!(
        stored(&pool, id).await.0.as_deref(),
        Some(old.public_key.as_str())
    );
    assert!(
        audit_details(&pool, "persona.signing_key.rotate")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn sync_takes_over_the_key_of_the_secret_when_the_database_holds_another_one() {
    let (pool, secrets, id, _) = seeded().await;
    let old = sync_signing_key(&pool, &secrets, id).await.unwrap();
    // A rotation replaced the Secret and then failed to store the public key.
    let replaced = SigningKey::generate().unwrap();
    secrets
        .replace(
            &secret_ref(PersonaId::from_uuid(id)),
            replaced.private_openssh(),
        )
        .await
        .unwrap();

    let synced = sync_signing_key(&pool, &secrets, id).await.unwrap();

    assert_eq!(synced.public_key, replaced.public_key());
    assert_eq!(stored(&pool, id).await.0, Some(replaced.public_key()));
    let audit = audit_details(&pool, "persona.signing_key.reconcile").await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["old_fingerprint"], old.fingerprint);
    assert_eq!(audit[0]["new_fingerprint"], replaced.fingerprint());
    assert_eq!(
        secrets
            .get(&secret_ref(PersonaId::from_uuid(id)))
            .await
            .unwrap()
            .expose_secret(),
        replaced.private_openssh().expose_secret()
    );
}

#[tokio::test]
async fn only_runs_that_have_not_ended_count_as_active_runs_of_the_persona() {
    let (pool, _secrets, id, _) = seeded().await;
    let project = df_core::ProjectId::new();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, 'DF', 'Dark Factory', 'https://git.example.org/o/r.git', 'img:latest', 'main')",
    )
    .bind(project.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let mut tasks = Vec::new();
    for title in ["Fix", "Fix more"] {
        let (task, _) = crate::tasks::create_task(
            &pool,
            crate::tasks::NewTask {
                project_id: project,
                persona_id: PersonaId::from_uuid(id),
                title: Some(title.into()),
                description: String::new(),
                issue_number: None,
                created_by: Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
        tasks.push(task.id);
    }
    assert_eq!(active_run_count(&pool, id).await.unwrap(), 0);

    for (task, number, state) in [
        (tasks[0], 1, "succeeded"),
        (tasks[0], 2, "running"),
        (tasks[1], 1, "pending"),
    ] {
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, $3, $4)")
            .bind(Uuid::now_v7())
            .bind(task.as_uuid())
            .bind(number)
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
    }

    assert_eq!(active_run_count(&pool, id).await.unwrap(), 2);
}

#[tokio::test]
async fn an_unknown_persona_is_not_found() {
    let (pool, secrets, _, _) = seeded().await;
    let err = sync_signing_key(&pool, &secrets, Uuid::now_v7())
        .await
        .unwrap_err();
    assert!(matches!(err, KeyError::Db(DbError::NotFound(_))), "{err}");
    assert!(secrets.created().is_empty());
}
