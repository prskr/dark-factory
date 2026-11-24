use super::*;
use df_core::UserId;

fn actor() -> (Principal, Vec<String>) {
    (
        Principal::human(UserId::new(), "Peter"),
        vec!["board-member".to_string()],
    )
}

fn project(key: &str, credential: Option<Uuid>) -> ProjectInput {
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

fn persona(prompt: &str, provider: Uuid) -> PersonaInput {
    PersonaInput {
        handle: "linus".into(),
        display_name: "Linus".into(),
        commit_email: "linus@example.org".into(),
        description: String::new(),
        system_prompt: prompt.into(),
        harness: "claude-code".into(),
        model: "test-model".into(),
        model_provider_id: provider,
    }
}

async fn audit_details(pool: &PgPool, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn credential_refs_and_personas_are_audited_with_before_and_after() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };

    let cred = save_credential_ref(
        &pool,
        actor,
        None,
        &CredentialRefInput {
            name: "claude-subscription".into(),
            secret_name: "claude".into(),
            secret_key: "token".into(),
        },
    )
    .await
    .unwrap();
    assert!(list_credential_refs(&pool).await.unwrap().contains(&cred));
    let created = audit_details(&pool, "credential_ref.create").await;
    assert_eq!(created[0]["after"]["secret_name"], "claude");
    assert!(created[0]["before"].is_null());

    let p = save_persona(&pool, actor, None, &persona("old", provider))
        .await
        .unwrap();
    save_persona(&pool, actor, Some(p.id), &persona("new", provider))
        .await
        .unwrap();
    let updated = audit_details(&pool, "persona.update").await;
    assert_eq!(updated[0]["before"]["system_prompt"], "old");
    assert_eq!(updated[0]["after"]["system_prompt"], "new");
    assert_eq!(
        get_persona(&pool, p.id)
            .await
            .unwrap()
            .unwrap()
            .system_prompt,
        "new"
    );

    assert_eq!(updated[0]["after"]["handle"], "linus");
    assert_eq!(updated[0]["after"]["commit_email"], "linus@example.org");
}

#[tokio::test]
async fn a_taken_handle_or_email_names_the_other_persona_and_changes_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    save_persona(&pool, actor, None, &persona("x", provider))
        .await
        .unwrap();
    let ada = || PersonaInput {
        handle: "ada".into(),
        display_name: "Ada".into(),
        commit_email: "ada@example.org".into(),
        ..persona("x", provider)
    };

    let same_handle = PersonaInput {
        commit_email: "other@example.org".into(),
        handle: "linus".into(),
        ..ada()
    };
    let same_email = PersonaInput {
        commit_email: "linus@example.org".into(),
        ..ada()
    };

    for (input, field) in [
        (same_handle, PersonaField::Handle),
        (same_email, PersonaField::CommitEmail),
    ] {
        let saved = save_persona(&pool, actor, None, &input).await;
        assert!(
            matches!(&saved, Err(DbError::PersonaFieldTaken { field: f, holder }) if *f == field && holder == "linus"),
            "{saved:?}"
        );
    }
    assert_eq!(list_personas(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn personas_may_share_a_display_name_and_are_listed_by_handle() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    for handle in ["reviewer-go", "reviewer"] {
        let input = PersonaInput {
            handle: handle.into(),
            display_name: "Reviewer".into(),
            commit_email: format!("{handle}@example.org"),
            ..persona("x", provider)
        };
        save_persona(&pool, actor, None, &input).await.unwrap();
    }

    let handles: Vec<_> = list_personas(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.handle)
        .collect();
    let found = persona_id_by_handle(&pool, "reviewer").await.unwrap();

    assert_eq!(handles, ["reviewer", "reviewer-go"]);
    assert!(found.is_some());
    assert_eq!(persona_id_by_handle(&pool, "Reviewer").await.unwrap(), None);
}

#[tokio::test]
async fn saving_rejects_an_invalid_identity_and_stores_the_normalized_one() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let invalid = [
        PersonaInput {
            display_name: "Bot <root@example.org>".into(),
            ..persona("x", provider)
        },
        PersonaInput {
            handle: "Code Reviewer".into(),
            ..persona("x", provider)
        },
        PersonaInput {
            commit_email: "no at sign".into(),
            ..persona("x", provider)
        },
    ];
    for input in invalid {
        let saved = save_persona(&pool, actor, None, &input).await;
        assert!(
            matches!(saved, Err(DbError::InvalidPersonaIdentity(_))),
            "{saved:?}"
        );
    }
    assert!(list_personas(&pool).await.unwrap().is_empty());

    let mixed = PersonaInput {
        display_name: "  Linus ".into(),
        commit_email: "Linus@Example.ORG".into(),
        ..persona("x", provider)
    };
    let saved = save_persona(&pool, actor, None, &mixed).await.unwrap();

    assert_eq!(
        (saved.display_name.as_str(), saved.commit_email.as_str()),
        ("Linus", "linus@example.org")
    );
}

#[tokio::test]
async fn the_database_check_rejects_invalid_handles_and_emails_written_around_the_api() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let invalid = [
        ("", "a@example.org"),
        ("-a", "a@example.org"),
        ("Code Reviewer", "a@example.org"),
        ("a_b", "a@example.org"),
        ("ok", "no at sign"),
    ];
    for (handle, email) in invalid {
        let inserted = sqlx::query(
            "INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, model, model_provider_id)
             VALUES ($1, 'x', $2, $3, 'p', 'test-model', $4)",
        )
        .bind(Uuid::now_v7())
        .bind(handle)
        .bind(email)
        .bind(provider)
        .execute(&pool)
        .await;
        assert!(inserted.is_err(), "{handle:?} {email:?}");
    }
    let too_long = sqlx::query(
        "INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, model, model_provider_id)
         VALUES ($1, 'x', $2, 'a@example.org', 'p', 'test-model', $3)",
    )
    .bind(Uuid::now_v7())
    .bind("a".repeat(40))
    .bind(provider)
    .execute(&pool)
    .await;
    assert!(too_long.is_err());
}

#[tokio::test]
async fn a_persona_model_the_provider_lacks_is_a_model_conflict() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let mut input = persona("x", provider);
    input.model = "no-such-model".into();

    let saved = save_persona(&pool, actor, None, &input).await;

    assert!(matches!(saved, Err(DbError::ModelConflict(_))), "{saved:?}");
}

#[tokio::test]
async fn the_branch_rewrite_policy_defaults_to_with_lease_and_changes_are_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    assert_eq!(p.branch_rewrite_policy, RewritePolicy::WithLease);

    let mut input = project("DF", None);
    input.branch_rewrite_policy = RewritePolicy::Never;
    save_project(&pool, actor, Some(p.id), &input)
        .await
        .unwrap();

    let stored = get_project_by_key(&pool, "DF").await.unwrap().unwrap();
    assert_eq!(stored.branch_rewrite_policy, RewritePolicy::Never);
    let updated = audit_details(&pool, "project.update").await;
    assert_eq!(updated[0]["before"]["branch_rewrite_policy"], "with-lease");
    assert_eq!(updated[0]["after"]["branch_rewrite_policy"], "never");
}

#[tokio::test]
async fn subtask_merge_defaults_to_human_and_changes_are_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    assert_eq!(p.subtask_merge, SubtaskMerge::Human);

    let mut input = project("DF", None);
    input.subtask_merge = SubtaskMerge::ParentAgent;
    save_project(&pool, actor, Some(p.id), &input)
        .await
        .unwrap();

    let stored = get_project_by_key(&pool, "DF").await.unwrap().unwrap();
    assert_eq!(stored.subtask_merge, SubtaskMerge::ParentAgent);
    let updated = audit_details(&pool, "project.update").await;
    assert_eq!(updated[0]["before"]["subtask_merge"], "human");
    assert_eq!(updated[0]["after"]["subtask_merge"], "parent-agent");
}

#[tokio::test]
async fn a_project_inserted_without_agent_branches_gets_the_dependency_bot_globs() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image) \
         VALUES ($1, 'OLD', 'Old', 'https://example.test/r.git', 'img')",
    )
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap();

    let project = get_project_by_key(&pool, "OLD").await.unwrap().unwrap();

    assert_eq!(project.agent_branch_globs, ["renovate/**", "dependabot/**"]);
}

#[tokio::test]
async fn agent_branch_globs_default_are_validated_and_changes_are_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    assert_eq!(p.agent_branch_globs, ["renovate/**", "dependabot/**"]);

    let mut invalid = project("DF", None);
    invalid.agent_branch_globs = vec!["renovate/[".into()];
    let rejected = save_project(&pool, actor, Some(p.id), &invalid).await;
    assert!(
        matches!(rejected, Err(DbError::InvalidData(_))),
        "{rejected:?}"
    );
    assert_eq!(
        get_project_by_key(&pool, "DF")
            .await
            .unwrap()
            .unwrap()
            .agent_branch_globs,
        ["renovate/**", "dependabot/**"]
    );

    let mut input = project("DF", None);
    input.agent_branch_globs = vec!["renovate/**".into(), "agent/**".into()];
    save_project(&pool, actor, Some(p.id), &input)
        .await
        .unwrap();
    let updated = audit_details(&pool, "project.update").await;
    assert_eq!(
        updated[0]["before"]["agent_branch_globs"],
        serde_json::json!(["renovate/**", "dependabot/**"])
    );
    assert_eq!(
        updated[0]["after"]["agent_branch_globs"],
        serde_json::json!(["renovate/**", "agent/**"])
    );
}

#[tokio::test]
async fn project_adapter_command_round_trips() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };

    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    assert!(p.adapter_command.is_empty());

    let mut input = project("DF", None);
    input.adapter_command = vec!["/opt/harness/bin/fake-acp-agent".into(), "--x".into()];
    let p = save_project(&pool, actor, Some(p.id), &input)
        .await
        .unwrap();
    let stored = get_project_by_key(&pool, "DF").await.unwrap().unwrap();
    assert_eq!(stored.adapter_command, input.adapter_command);
    assert_eq!(p.adapter_command, input.adapter_command);

    input.adapter_command.clear();
    save_project(&pool, actor, Some(p.id), &input)
        .await
        .unwrap();
    let stored = get_project_by_key(&pool, "DF").await.unwrap().unwrap();
    assert!(stored.adapter_command.is_empty());
}

#[tokio::test]
async fn a_changed_push_policy_is_stored_audited_and_announced_with_the_project() {
    use std::time::Duration;

    use crate::notify::{Notification, Notifier};
    use crate::push_checks::{CheckMode, PushPolicy, push_policies, push_policy};

    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let default = PushPolicy::default();
    update_project_with_push_policy(&pool, actor, p.id, &project("DF", None), &default)
        .await
        .unwrap();
    assert!(push_policies(&pool, &[p.id]).await.unwrap().is_empty());
    assert_eq!(push_policy(&pool, p.id).await.unwrap(), default);

    let mut policy = PushPolicy::default();
    policy.protected_globs.push("deploy/**".into());
    policy.secret_mode = CheckMode::Reject;
    let mut renamed = project("DF", None);
    renamed.name = "Factory".into();
    update_project_with_push_policy(&pool, actor, p.id, &renamed, &policy)
        .await
        .unwrap();
    assert_eq!(push_policy(&pool, p.id).await.unwrap(), policy);
    assert_eq!(
        get_project_by_key(&pool, "DF").await.unwrap().unwrap().name,
        "Factory"
    );
    let created = audit_details(&pool, "push_policy.create").await;
    assert_eq!(created.len(), 1);
    assert!(created[0]["before"].is_null());
    assert_eq!(created[0]["after"]["protected_globs"][4], "deploy/**");
    assert_eq!(created[0]["after"]["secret_mode"], "reject");
    tokio::time::timeout(Duration::from_secs(5), async {
        while notifications.recv().await.unwrap() != Notification::EgressPolicy {}
    })
    .await
    .unwrap();

    update_project_with_push_policy(&pool, actor, p.id, &renamed, &policy)
        .await
        .unwrap();
    assert!(audit_details(&pool, "push_policy.update").await.is_empty());
    let mut linear = policy.clone();
    linear.linear_history = true;
    update_project_with_push_policy(&pool, actor, p.id, &renamed, &linear)
        .await
        .unwrap();
    let updated = audit_details(&pool, "push_policy.update").await;
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0]["before"]["linear_history"], false);
    assert_eq!(updated[0]["after"]["linear_history"], true);

    let rekeyed =
        update_project_with_push_policy(&pool, actor, p.id, &project("DFX", None), &default).await;
    assert!(matches!(rekeyed, Err(DbError::Conflict(_))), "{rekeyed:?}");
    assert_eq!(push_policy(&pool, p.id).await.unwrap(), linear);
}

#[tokio::test]
async fn hidden_globs_round_trip_with_an_audit_entry_and_reject_alternation() {
    use crate::push_checks::{PushPolicy, push_policy};

    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();

    let policy = PushPolicy {
        hidden_globs: vec!["talos/**".into(), "configs/*".into()],
        ..PushPolicy::default()
    };
    update_project_with_push_policy(&pool, actor, p.id, &project("DF", None), &policy)
        .await
        .unwrap();
    assert_eq!(push_policy(&pool, p.id).await.unwrap(), policy);
    let created = audit_details(&pool, "push_policy.create").await;
    assert!(created[0]["before"].is_null());
    assert_eq!(created[0]["after"]["hidden_globs"][0], "talos/**");

    let mut changed = policy.clone();
    changed.hidden_globs = vec!["secret/**".into()];
    update_project_with_push_policy(&pool, actor, p.id, &project("DF", None), &changed)
        .await
        .unwrap();
    let updated = audit_details(&pool, "push_policy.update").await;
    assert_eq!(updated[0]["before"]["hidden_globs"][0], "talos/**");
    assert_eq!(updated[0]["after"]["hidden_globs"][0], "secret/**");

    let mut invalid = changed.clone();
    invalid.hidden_globs = vec!["{a,b}/**".into()];
    let saved =
        update_project_with_push_policy(&pool, actor, p.id, &project("DF", None), &invalid).await;
    assert!(matches!(saved, Err(DbError::InvalidData(_))), "{saved:?}");
    assert_eq!(push_policy(&pool, p.id).await.unwrap(), changed);
}

#[tokio::test]
async fn project_key_is_immutable_after_creation() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let provider = crate::model_providers::tests::seed_provider(&pool).await;
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };

    let p = save_project(&pool, actor, None, &project("DF", None))
        .await
        .unwrap();
    assert!(!p.has_tasks);
    // The key never changes after creation, with or without tasks.
    let renamed = save_project(&pool, actor, Some(p.id), &project("DFX", None)).await;
    assert!(matches!(renamed, Err(DbError::Conflict(_))), "{renamed:?}");
    assert!(get_project_by_key(&pool, "DFX").await.unwrap().is_none());
    assert_eq!(
        get_project_by_key(&pool, "DF").await.unwrap().unwrap().id,
        p.id
    );

    let persona = save_persona(&pool, actor, None, &persona("p", provider))
        .await
        .unwrap();
    crate::create_task(
        &pool,
        crate::NewTask {
            project_id: df_core::ProjectId::from_uuid(p.id),
            persona_id: df_core::PersonaId::from_uuid(persona.id),
            title: Some("t".into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();

    let rejected = save_project(&pool, actor, Some(p.id), &project("OTHER", None)).await;
    assert!(
        matches!(rejected, Err(DbError::Conflict(_))),
        "{rejected:?}"
    );
    let same_key = save_project(&pool, actor, Some(p.id), &project("DF", None))
        .await
        .unwrap();
    assert!(same_key.has_tasks);
    assert_eq!(
        get_project_by_key(&pool, "DF").await.unwrap().unwrap().id,
        p.id
    );
}
