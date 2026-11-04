use df_core::{Principal, UserId};

use super::*;
use crate::catalog::{CredentialRefInput, PersonaInput, save_credential_ref, save_persona};

fn actor() -> (Principal, Vec<String>) {
    (
        Principal::human(UserId::new(), "Peter"),
        vec!["board-member".to_string()],
    )
}

async fn credential(pool: &PgPool, actor: Actor<'_>, name: &str) -> Uuid {
    save_credential_ref(
        pool,
        actor,
        None,
        &CredentialRefInput {
            name: name.into(),
            secret_name: "llm".into(),
            secret_key: "key".into(),
        },
    )
    .await
    .unwrap()
    .id
    .as_uuid()
}

fn input(name: &str, credential: Uuid) -> ModelProviderInput {
    ModelProviderInput {
        name: name.into(),
        credential_ref_id: credential,
        scheme: CredentialScheme::XApiKey,
        credential_header: None,
        credential_prefix: None,
        headers: vec![HeaderTemplate {
            name: "x-session".into(),
            template: "{{ task_id | sha256 }}".into(),
        }],
        apis: vec![
            ProviderApi {
                wire_api: WireApi::AnthropicMessages,
                upstream_url: "https://api.anthropic.com".into(),
            },
            ProviderApi {
                wire_api: WireApi::OpenAiChat,
                upstream_url: "https://api.example.org/v1".into(),
            },
        ],
    }
}

fn persona(provider: Uuid) -> PersonaInput {
    PersonaInput {
        name: "Linus".into(),
        system_prompt: "p".into(),
        harness: "claude-code".into(),
        model: None,
        model_provider_id: provider,
    }
}

#[tokio::test]
async fn provider_is_stored_with_its_apis_and_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = credential(&pool, actor, "anthropic-key").await;

    let saved = save_model_provider(&pool, actor, None, &input("Work", credential))
        .await
        .unwrap();
    let loaded = get_model_provider(&pool, saved.id).await.unwrap().unwrap();
    assert_eq!(loaded.input, input("Work", credential));

    let mut changed = input("Work", credential);
    changed.apis.truncate(1);
    changed.scheme = CredentialScheme::Bearer;
    save_model_provider(&pool, actor, Some(saved.id), &changed)
        .await
        .unwrap();
    assert_eq!(list_model_providers(&pool).await.unwrap().len(), 1);
    let reloaded = get_model_provider(&pool, saved.id).await.unwrap().unwrap();
    assert_eq!(reloaded.wire_apis(), [WireApi::AnthropicMessages]);

    let details: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT details FROM audit_log WHERE action LIKE 'model_provider.%' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(details.len(), 2);
    assert_eq!(details[1]["before"]["scheme"], "x-api-key");
    assert_eq!(details[1]["after"]["scheme"], "bearer");
}

#[tokio::test]
async fn provider_names_are_unique() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = credential(&pool, actor, "k").await;
    save_model_provider(&pool, actor, None, &input("Work", credential))
        .await
        .unwrap();
    let err = save_model_provider(&pool, actor, None, &input("Work", credential)).await;
    assert!(matches!(err, Err(DbError::Conflict(_))));
}

#[tokio::test]
async fn used_provider_cannot_be_deleted_and_the_error_names_the_persona() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = credential(&pool, actor, "k").await;
    let provider = save_model_provider(&pool, actor, None, &input("Work", credential))
        .await
        .unwrap();
    save_persona(&pool, actor, None, &persona(provider.id))
        .await
        .unwrap();

    let Err(DbError::Conflict(message)) = delete_model_provider(&pool, actor, provider.id).await
    else {
        panic!("expected a conflict");
    };
    assert!(message.contains("Linus"), "{message}");
    // The foreign key refuses it as well, without the application check.
    let raw = sqlx::query("DELETE FROM model_providers WHERE id = $1")
        .bind(provider.id)
        .execute(&pool)
        .await;
    assert!(raw.is_err());
    assert_eq!(personas_using(&pool, provider.id).await.unwrap(), ["Linus"]);
}

#[tokio::test]
async fn unused_provider_is_deleted_with_its_apis() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = credential(&pool, actor, "k").await;
    let provider = save_model_provider(&pool, actor, None, &input("Work", credential))
        .await
        .unwrap();

    delete_model_provider(&pool, actor, provider.id)
        .await
        .unwrap();

    assert!(
        get_model_provider(&pool, provider.id)
            .await
            .unwrap()
            .is_none()
    );
    let apis: i64 = sqlx::query_scalar("SELECT count(*) FROM model_provider_apis")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(apis, 0);
}

/// The migration before the one that introduces model providers.
async fn migrate_to_before_providers(pool: &PgPool) {
    let versions: Vec<_> = crate::MIGRATOR.iter().collect();
    let at = versions
        .iter()
        .position(|m| m.description.contains("model providers"))
        .unwrap();
    crate::MIGRATOR
        .run_to(versions[at - 1].version, pool)
        .await
        .unwrap();
}

async fn old_credential(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, 'claude', 'token')")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn old_persona(pool: &PgPool, name: &str, credential: Option<Uuid>) {
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness_credential_id) VALUES ($1, $2, 'p', $3)",
    )
    .bind(Uuid::now_v7())
    .bind(name)
    .bind(credential)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn existing_persona_gets_a_claude_subscription_provider_for_its_credential() {
    let pool = df_test_postgres::empty_pool().await;
    migrate_to_before_providers(&pool).await;
    let credential = old_credential(&pool, "claude").await;
    old_persona(&pool, "Linus", Some(credential)).await;
    old_persona(&pool, "Ada", Some(credential)).await;

    crate::MIGRATOR.run(&pool).await.unwrap();

    let providers = list_model_providers(&pool).await.unwrap();
    assert_eq!(providers.len(), 1, "one provider per credential");
    let provider = &providers[0];
    assert_eq!(provider.input.name, "Claude subscription (claude)");
    assert_eq!(provider.input.scheme, CredentialScheme::ClaudeSubscription);
    assert_eq!(provider.input.credential_ref_id, credential);
    assert_eq!(
        provider.input.apis,
        [ProviderApi {
            wire_api: WireApi::AnthropicMessages,
            upstream_url: "https://api.anthropic.com".into()
        }]
    );
    assert_eq!(
        personas_using(&pool, provider.id).await.unwrap(),
        ["Ada", "Linus"]
    );
}

#[tokio::test]
async fn migration_of_an_empty_instance_creates_no_provider() {
    let pool = df_test_postgres::empty_pool().await;
    migrate_to_before_providers(&pool).await;

    crate::MIGRATOR.run(&pool).await.unwrap();

    assert!(list_model_providers(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn migration_refuses_a_persona_without_credential_and_names_it() {
    let pool = df_test_postgres::empty_pool().await;
    migrate_to_before_providers(&pool).await;
    old_persona(&pool, "Nameless", None).await;

    let err = crate::MIGRATOR.run(&pool).await.unwrap_err();

    assert!(err.to_string().contains("Nameless"), "{err}");
}

#[tokio::test]
async fn run_providers_pair_each_active_run_with_its_harness_and_provider() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = credential(&pool, actor, "k").await;
    let provider = save_model_provider(&pool, actor, None, &input("Work", credential))
        .await
        .unwrap();
    let persona = save_persona(&pool, actor, None, &persona(provider.id))
        .await
        .unwrap();
    let project = crate::catalog::save_project(
        &pool,
        actor,
        None,
        &crate::catalog::ProjectInput {
            key: "DF".into(),
            name: "DF".into(),
            repo_url: "https://git.example.org/o/r.git".into(),
            issue_tracker_url: None,
            image: "img".into(),
            base_branch: "main".into(),
            adapter_command: Vec::new(),
            forge_credential_id: None,
        },
    )
    .await
    .unwrap();
    let (task, _) = crate::create_task(
        &pool,
        crate::NewTask {
            project_id: df_core::ProjectId::from_uuid(project.id),
            persona_id: df_core::PersonaId::from_uuid(persona.id),
            title: Some("T".into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let run = Uuid::now_v7();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(run)
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let found = run_model_providers(&pool, &[run, Uuid::now_v7()])
        .await
        .unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[&run].harness, "claude-code");
    assert_eq!(found[&run].provider.id, provider.id);
}

/// Makes sure the database has a model provider for tests that insert personas with plain SQL
/// (`(SELECT id FROM model_providers LIMIT 1)`).
pub(crate) async fn seed_provider(pool: &PgPool) -> Uuid {
    sqlx::query(
        "INSERT INTO credential_refs (id, name, secret_name, secret_key) \
         VALUES ($1, 'test-llm', 'llm', 'key') ON CONFLICT (name) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme) \
         SELECT $1, 'test-provider', id, 'claude-subscription' FROM credential_refs \
         WHERE name = 'test-llm' ON CONFLICT (name) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url) \
         SELECT id, 'anthropic-messages', 'https://api.anthropic.com' FROM model_providers \
         WHERE name = 'test-provider' ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM model_providers WHERE name = 'test-provider'")
        .fetch_one(pool)
        .await
        .unwrap()
}
