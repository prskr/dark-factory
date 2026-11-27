use df_core::mcp::McpCallOutcome;
use df_core::webhooks::WebhookWake;
use df_core::{Principal, UserId};
use df_core::{RewritePolicy, SubtaskMerge};

use super::*;
use crate::catalog::{CredentialRefInput, ProjectInput, save_credential_ref, save_project};
use crate::model_providers::{ModelProviderInput, save_model_provider};
use crate::model_usage::Inserted;

fn actor() -> (Principal, Vec<String>) {
    (
        Principal::human(UserId::new(), "Peter"),
        vec!["board-member".to_string(), "admin".to_string()],
    )
}

async fn credential(pool: &PgPool, actor: Actor<'_>, name: &str) -> Uuid {
    save_credential_ref(
        pool,
        actor,
        None,
        &CredentialRefInput {
            name: name.into(),
            secret_name: "mcp".into(),
            secret_key: "key".into(),
        },
    )
    .await
    .unwrap()
    .id
    .as_uuid()
}

fn project_input(key: &str, credential: Option<Uuid>) -> ProjectInput {
    ProjectInput {
        key: key.into(),
        name: "Dark Factory".into(),
        repo_url: "https://git.example.org/o/r.git".into(),
        issue_tracker_url: None,
        image: "img:latest".into(),
        base_branch: "main".into(),
        adapter_command: Vec::new(),
        forge_credential_id: credential,
        webhook_wake: WebhookWake::Members,
        branch_rewrite_policy: RewritePolicy::default(),
        agent_branch_globs: df_core::default_agent_branch_globs(),
        subtask_merge: SubtaskMerge::default(),
    }
}

fn input(name: &str, credential: Uuid) -> McpServerInput {
    McpServerInput {
        name: name.into(),
        url: "https://mcp.context7.com/mcp".into(),
        credential_ref_id: credential,
        scheme: McpCredentialScheme::Header {
            name: "CONTEXT7_API_KEY".into(),
            prefix: String::new(),
        },
        private_network: false,
    }
}

async fn project(pool: &PgPool, actor: Actor<'_>, key: &str) -> Uuid {
    save_project(pool, actor, None, &project_input(key, None))
        .await
        .unwrap()
        .id
}

async fn audit_actions(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn server_is_stored_with_its_scheme_and_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;

    let saved = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();

    assert_eq!(saved.config_version, 1);
    assert_eq!(saved.input, input("context7", key));
    assert!(saved.projects.is_empty());
    let query = McpServerInput {
        url: "https://design.example.com/mcp/stream?file=abc".into(),
        scheme: McpCredentialScheme::Query {
            parameter: "token".into(),
        },
        ..input("penpot", key)
    };
    let penpot = save_mcp_server(&pool, actor, None, &query).await.unwrap();
    let bearer = McpServerInput {
        scheme: McpCredentialScheme::Bearer,
        private_network: true,
        ..input("wiki", key)
    };
    let wiki = save_mcp_server(&pool, actor, None, &bearer).await.unwrap();
    let listed = list_mcp_servers(&pool).await.unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|s| s.input.name.as_str())
            .collect::<Vec<_>>(),
        ["context7", "penpot", "wiki"]
    );
    assert_eq!(listed[1].input, query);
    assert_eq!(
        get_mcp_server(&pool, wiki.id).await.unwrap().unwrap().input,
        bearer
    );
    assert_eq!(penpot.input, query);
    assert_eq!(
        audit_actions(&pool).await,
        [
            "credential_ref.create",
            "mcp_server.create",
            "mcp_server.create",
            "mcp_server.create"
        ]
    );
}

#[tokio::test]
async fn editing_a_server_increases_its_config_version_and_audits_before_and_after() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let other = credential(&pool, actor, "other-key").await;
    let saved = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();

    let updated = save_mcp_server(&pool, actor, Some(saved.id), &input("context7", other))
        .await
        .unwrap();

    assert_eq!(updated.config_version, 2);
    assert_eq!(updated.input.credential_ref_id, other);
    let details: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'mcp_server.update'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["before"]["credential_ref_id"], key.to_string());
    assert_eq!(details[0]["after"]["credential_ref_id"], other.to_string());
    assert_eq!(details[0]["after"]["scheme"], "header");
}

#[tokio::test]
async fn the_name_of_a_server_cannot_change_and_stays_unique() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let first = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();

    let renamed = save_mcp_server(&pool, actor, Some(first.id), &input("docs", key)).await;
    assert!(
        matches!(renamed, Err(DbError::McpServer(McpServerConflict::Name(_)))),
        "{renamed:?}"
    );
    let duplicate = save_mcp_server(&pool, actor, None, &input("context7", key)).await;
    assert!(
        matches!(
            duplicate,
            Err(DbError::McpServer(McpServerConflict::Name(_)))
        ),
        "{duplicate:?}"
    );
}

#[tokio::test]
async fn a_server_enabled_for_a_project_cannot_be_deleted_and_the_error_names_the_projects() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let server = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();
    let (df, doc) = (
        project(&pool, actor, "DF").await,
        project(&pool, actor, "DOC").await,
    );
    set_project_mcp_servers(&pool, actor, df, &[server.id])
        .await
        .unwrap();
    set_project_mcp_servers(&pool, actor, doc, &[server.id])
        .await
        .unwrap();

    let refused = delete_mcp_server(&pool, actor, server.id)
        .await
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "conflict: MCP server context7 is enabled for DF, DOC; disable it there first."
    );
    assert_eq!(
        get_mcp_server(&pool, server.id)
            .await
            .unwrap()
            .unwrap()
            .projects,
        ["DF", "DOC"]
    );

    set_project_mcp_servers(&pool, actor, df, &[])
        .await
        .unwrap();
    set_project_mcp_servers(&pool, actor, doc, &[])
        .await
        .unwrap();
    delete_mcp_server(&pool, actor, server.id).await.unwrap();
    assert!(get_mcp_server(&pool, server.id).await.unwrap().is_none());
    assert!(
        audit_actions(&pool)
            .await
            .contains(&"mcp_server.delete".to_string())
    );
}

#[tokio::test]
async fn the_database_refuses_to_delete_a_credential_reference_a_server_uses() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let server = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();

    // Deleting credential references is not offered; the foreign key refuses it anyway.
    let refused = sqlx::query("DELETE FROM credential_refs WHERE id = $1")
        .bind(key)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(
        matches!(&refused, sqlx::Error::Database(db) if db.is_foreign_key_violation()),
        "{refused:?}"
    );
    let kept = get_mcp_server(&pool, server.id).await.unwrap().unwrap();
    assert_eq!(kept.input.credential_ref_id, key);
}

#[tokio::test]
async fn a_server_cannot_use_a_forge_or_model_provider_credential() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let forge = credential(&pool, actor, "forge-bot").await;
    let llm = credential(&pool, actor, "llm-key").await;
    save_project(&pool, actor, None, &project_input("DF", Some(forge)))
        .await
        .unwrap();
    save_model_provider(&pool, actor, None, &provider("Work", llm))
        .await
        .unwrap();

    let forge_refused = save_mcp_server(&pool, actor, None, &input("forgejo", forge))
        .await
        .unwrap_err();
    assert_eq!(
        forge_refused.to_string(),
        "conflict: credential `forge-bot` is the forge credential of project DF"
    );
    let llm_refused = save_mcp_server(&pool, actor, None, &input("llm", llm))
        .await
        .unwrap_err();
    assert_eq!(
        llm_refused.to_string(),
        "conflict: credential `llm-key` is the credential of model provider Work"
    );
    assert!(list_mcp_servers(&pool).await.unwrap().is_empty());
}

fn provider(name: &str, credential: Uuid) -> ModelProviderInput {
    ModelProviderInput {
        name: name.into(),
        credential_ref_id: credential,
        scheme: df_core::model_provider::CredentialScheme::Bearer,
        credential_header: None,
        credential_prefix: None,
        headers: Vec::new(),
        apis: vec![df_core::model_provider::ProviderApi {
            wire_api: df_core::model_provider::WireApi::OpenAiChat,
            upstream_url: "https://api.example.org/v1".into(),
        }],
        models: Vec::new(),
    }
}

#[tokio::test]
async fn a_project_or_model_provider_cannot_take_a_credential_a_server_uses() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();

    let project = save_project(&pool, actor, None, &project_input("DF", Some(key)))
        .await
        .unwrap_err();
    assert_eq!(
        project.to_string(),
        "conflict: credential `context7-key` is used by MCP server context7"
    );
    let provider = save_model_provider(&pool, actor, None, &provider("Work", key))
        .await
        .unwrap_err();
    assert_eq!(
        provider.to_string(),
        "conflict: credential `context7-key` is used by MCP server context7"
    );
}

#[tokio::test]
async fn enabling_servers_for_a_project_is_audited_and_nothing_is_enabled_by_default() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let context7 = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();
    let wiki = save_mcp_server(&pool, actor, None, &input("wiki", key))
        .await
        .unwrap();
    let df = project(&pool, actor, "DF").await;
    assert!(project_mcp_server_ids(&pool, df).await.unwrap().is_empty());

    set_project_mcp_servers(&pool, actor, df, &[context7.id, wiki.id])
        .await
        .unwrap();
    set_project_mcp_servers(&pool, actor, df, &[wiki.id])
        .await
        .unwrap();

    assert_eq!(project_mcp_server_ids(&pool, df).await.unwrap(), [wiki.id]);
    let details: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT details FROM audit_log WHERE action = 'project.mcp_servers_update' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(details.len(), 2);
    assert_eq!(
        details[1]["before"],
        serde_json::json!(["context7", "wiki"])
    );
    assert_eq!(details[1]["after"], serde_json::json!(["wiki"]));
    let unknown = set_project_mcp_servers(&pool, actor, df, &[Uuid::now_v7()]).await;
    assert!(matches!(unknown, Err(DbError::NotFound(_))), "{unknown:?}");
}

#[tokio::test]
async fn servers_of_projects_lists_the_enabled_servers_per_project() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let context7 = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();
    save_mcp_server(&pool, actor, None, &input("billing", key))
        .await
        .unwrap();
    let (df, doc) = (
        project(&pool, actor, "DF").await,
        project(&pool, actor, "DOC").await,
    );
    set_project_mcp_servers(&pool, actor, df, &[context7.id])
        .await
        .unwrap();

    let found = enabled_servers_of_projects(&pool, &[df, doc])
        .await
        .unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[&df].len(), 1);
    assert_eq!(found[&df][0].input.name, "context7");
    assert_eq!(found[&df][0].config_version, 1);
}

#[tokio::test]
async fn every_server_change_publishes_a_policy_notification() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let key = credential(&pool, actor, "context7-key").await;
    let notifier = crate::Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let saved = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();
    let df = project(&pool, actor, "DF").await;
    set_project_mcp_servers(&pool, actor, df, &[saved.id])
        .await
        .unwrap();
    set_project_mcp_servers(&pool, actor, df, &[])
        .await
        .unwrap();
    delete_mcp_server(&pool, actor, saved.id).await.unwrap();

    let mut seen = 0;
    while seen < 4 {
        if notifications.recv().await.unwrap() == Notification::EgressPolicy {
            seen += 1;
        }
    }
}

#[test]
fn debug_output_omits_the_url_query() {
    let server = McpServerInput {
        url: "https://design.example.com/mcp/stream?file=abc&token=sekret".into(),
        ..input("penpot", Uuid::now_v7())
    };
    let record = McpServerRecord {
        id: Uuid::now_v7(),
        input: server.clone(),
        config_version: 1,
        projects: Vec::new(),
    };
    for debug in [format!("{server:?}"), format!("{record:#?}")] {
        assert!(!debug.contains("sekret"), "{debug}");
        assert!(!debug.contains("file=abc"), "{debug}");
        assert!(
            debug.contains("https://design.example.com/mcp/stream"),
            "{debug}"
        );
    }
}

async fn run(pool: &PgPool) -> Uuid {
    let provider = crate::model_providers::tests::seed_provider(pool).await;
    let (project, persona, task, run) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'RUN', 'P', 'https://g/o/r.git', 'img')")
        .bind(project)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, model, model_provider_id) VALUES ($1, 'p', 'p', 'p@example.org', 'x', 'test-model', $2)")
        .bind(persona)
        .bind(provider)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by) VALUES ($1, $2, 1, 'RUN-1', 't', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')")
        .bind(task)
        .bind(project)
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number, state, outcome, ended_at) VALUES ($1, $2, 1, 'succeeded', 'completed', now())")
        .bind(run)
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
    run
}

fn call(run_id: Uuid, tool: &str, seconds: i64) -> McpCallRecord {
    McpCallRecord {
        call_id: Uuid::now_v7(),
        run_id,
        server_name: "context7".into(),
        tool: tool.into(),
        outcome: McpCallOutcome::Ok,
        started_at: jiff::Timestamp::from_second(1_800_000_000 + seconds).unwrap(),
        duration_ms: 1200,
        request_bytes: 400,
        response_bytes: 3100,
    }
}

#[tokio::test]
async fn a_retried_call_report_is_stored_once_and_listed_oldest_first() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run(&pool).await;
    let later = call(run, "get-library-docs", 60);
    let earlier = McpCallRecord {
        outcome: McpCallOutcome::Rejected,
        ..call(run, "resolve-library-id", 0)
    };

    assert_eq!(insert_mcp_call(&pool, &later).await.unwrap(), Inserted::New);
    assert_eq!(
        insert_mcp_call(&pool, &later).await.unwrap(),
        Inserted::Duplicate
    );
    assert_eq!(
        insert_mcp_call(&pool, &earlier).await.unwrap(),
        Inserted::New
    );

    let listed = list_mcp_calls_of_runs(&pool, &[run]).await.unwrap();
    assert_eq!(listed[&run], [earlier, later]);
    assert!(
        list_mcp_calls_of_runs(&pool, &[Uuid::now_v7()])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_call_record_of_an_unknown_run_is_not_stored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    let outcome = insert_mcp_call(&pool, &call(Uuid::now_v7(), "x", 0))
        .await
        .unwrap();

    assert_eq!(outcome, Inserted::UnknownRun);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_calls")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn call_records_are_deleted_with_their_run() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = run(&pool).await;
    insert_mcp_call(&pool, &call(run, "x", 0)).await.unwrap();

    sqlx::query("DELETE FROM runs WHERE id = $1")
        .bind(run)
        .execute(&pool)
        .await
        .unwrap();

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_calls")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn call_records_survive_the_deletion_of_their_server() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let run = run(&pool).await;
    let key = credential(&pool, actor, "context7-key").await;
    let server = save_mcp_server(&pool, actor, None, &input("context7", key))
        .await
        .unwrap();
    insert_mcp_call(&pool, &call(run, "x", 0)).await.unwrap();

    delete_mcp_server(&pool, actor, server.id).await.unwrap();

    assert_eq!(
        list_mcp_calls_of_runs(&pool, &[run]).await.unwrap()[&run].len(),
        1
    );
}
