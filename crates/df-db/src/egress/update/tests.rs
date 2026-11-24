//! Container-backed tests: need a PostgreSQL container (testcontainers).

use df_core::{Principal, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::egress::{add_deny_rule, add_entry, delete_entry, list_entries};
use crate::notify::Notifier;

fn actor_fixture() -> (Principal, Vec<String>) {
    (
        Principal::human(UserId::new(), "Peter"),
        vec!["board-member".to_string()],
    )
}

fn host(host: &str, port: u16) -> EntryKind {
    EntryKind::HttpHost {
        host: host.into(),
        port,
    }
}

fn deny(host: &str) -> EntryKind {
    EntryKind::HttpDeny { host: host.into() }
}

fn service(name: &str, ports: &[u16]) -> EntryKind {
    EntryKind::ClusterService {
        namespace: "nativelink".into(),
        service: name.into(),
        ports: ports.to_vec(),
        allow_dns: false,
    }
}

async fn project(pool: &PgPool, key: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ($1, $2, $2, 'https://example.test/r.git', 'img:latest')",
    )
    .bind(id)
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn task(pool: &PgPool, project: Uuid, number: i64) -> Uuid {
    crate::model_providers::tests::seed_provider(pool).await;
    let persona = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, model, model_provider_id)
         VALUES ($1, $1::text, $1::text, $1::text || '@example.org', 'x', 'test-model', (SELECT id FROM model_providers LIMIT 1))",
    )
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ($1, $2, $3, (SELECT key FROM projects WHERE id = $2) || '-' || $3::text, 't',
                 'user', $4, '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(id)
    .bind(project)
    .bind(number)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn create_host(pool: &PgPool, scope: HostScope, host: &str, port: u16) -> EntryRecord {
    let (principal, roles) = actor_fixture();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let mut tx = pool.begin().await.unwrap();
    let record = create_http_host_entry(&mut tx, scope, host, port, actor, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    record
}

struct Who(Principal, Vec<String>);

impl Who {
    fn new() -> Self {
        let (principal, roles) = actor_fixture();
        Self(principal, roles)
    }

    fn actor(&self) -> Actor<'_> {
        Actor {
            principal: &self.0,
            roles: &self.1,
        }
    }
}

/// What the listener delivered before a marker that is published after the action under test:
/// notifications arrive in commit order.
async fn notifications_before_marker(
    pool: &PgPool,
    notifications: &mut tokio::sync::broadcast::Receiver<crate::Notification>,
) -> Vec<crate::Notification> {
    let marker = crate::Notification::TaskChanged {
        task_id: df_core::TaskId::new(),
    };
    crate::notify::publish(pool, &marker).await.unwrap();
    let mut before = Vec::new();
    loop {
        let received =
            tokio::time::timeout(std::time::Duration::from_secs(5), notifications.recv())
                .await
                .expect("the marker never arrived")
                .unwrap();
        if received == marker {
            return before;
        }
        before.push(received);
    }
}

fn policy_publications(published: &[crate::Notification]) -> usize {
    published
        .iter()
        .filter(|n| **n == crate::Notification::EgressPolicy)
        .count()
}

async fn audit(pool: &PgPool, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_host_typo_is_fixed_in_place_audited_with_both_values_and_published() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let typo = create_host(&pool, HostScope::Project(df), "pypi.orgg", 443).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let who = Who::new();
    let actor = who.actor();

    let edited = update_entry(
        &pool,
        actor,
        typo.id,
        HostScope::Project(df),
        &host("pypi.org", 443),
    )
    .await
    .unwrap();

    assert_eq!(edited.id, typo.id);
    assert_eq!(edited.project_key.as_deref(), Some("DF"));
    assert_eq!(list_entries(&pool).await.unwrap(), [edited]);
    let updates = audit(&pool, "egress_allowlist.update").await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["before"]["host"], "pypi.orgg");
    assert_eq!(updates[0]["after"]["host"], "pypi.org");
    assert_eq!(updates[0]["after"]["project_key"], "DF");
    let published = notifications_before_marker(&pool, &mut notifications).await;
    assert_eq!(policy_publications(&published), 1, "{published:?}");
}

#[tokio::test]
async fn the_scope_of_a_rule_moves_between_instance_and_projects() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let ops = project(&pool, "OPS").await;
    let rule = create_host(&pool, HostScope::Project(df), "github.com", 443).await;
    let who = Who::new();
    let actor = who.actor();

    let widened = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Instance,
        &host("github.com", 443),
    )
    .await
    .unwrap();
    assert_eq!((widened.project_key, widened.task_key), (None, None));
    let narrowed = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Project(ops),
        &host("github.com", 443),
    )
    .await
    .unwrap();

    assert_eq!(narrowed.project_key.as_deref(), Some("OPS"));
    assert_eq!(list_entries(&pool).await.unwrap(), [narrowed]);
}

#[tokio::test]
async fn a_task_rule_moves_to_its_project_or_stays_with_its_task_but_never_to_another_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let (df7, df8) = (task(&pool, df, 7).await, task(&pool, df, 8).await);
    let rule = create_host(&pool, HostScope::Task(df7), "pypi.org", 443).await;
    assert_eq!(rule.task_project_key.as_deref(), Some("DF"));
    let who = Who::new();
    let actor = who.actor();

    let kept = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Task(df7),
        &host("pypi.org", 80),
    )
    .await
    .unwrap();
    assert_eq!(kept.task_key.as_deref(), Some("DF-7"));
    let other = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Task(df8),
        &host("pypi.org", 80),
    )
    .await;
    assert!(matches!(other, Err(DbError::InvalidData(_))), "{other:?}");
    let moved = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Project(df),
        &host("pypi.org", 80),
    )
    .await
    .unwrap();

    assert_eq!(moved.project_key.as_deref(), Some("DF"));
    assert_eq!(moved.task_key, None);
    assert_eq!(list_entries(&pool).await.unwrap(), [moved]);
}

#[tokio::test]
async fn list_entries_names_the_project_of_a_task_rule() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let df7 = task(&pool, df, 7).await;
    create_host(&pool, HostScope::Task(df7), "pypi.org", 443).await;

    let listed = list_entries(&pool).await.unwrap();

    assert_eq!(listed[0].task_key.as_deref(), Some("DF-7"));
    assert_eq!(listed[0].task_project_key.as_deref(), Some("DF"));
    assert_eq!(listed[0].project_key, None);
}

#[tokio::test]
async fn the_kind_and_the_scope_limits_of_a_kind_cannot_be_changed_by_an_edit() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let who = Who::new();
    let actor = who.actor();
    let df = project(&pool, "DF").await;
    let df7 = task(&pool, df, 7).await;
    let allow = create_host(&pool, HostScope::Instance, "github.com", 443).await;
    let rule = add_deny_rule(&pool, "*.datadoghq.com", actor)
        .await
        .unwrap();
    let cluster = add_entry(&pool, actor, None, &service("scheduler", &[8980]))
        .await
        .unwrap();
    let denied = [
        (allow.id, HostScope::Instance, deny("github.com")),
        (allow.id, HostScope::Instance, service("scheduler", &[1])),
        (rule.id, HostScope::Instance, host("a.example", 443)),
        (rule.id, HostScope::Project(df), deny("*.datadoghq.com")),
        (
            cluster.id,
            HostScope::Task(df7),
            service("scheduler", &[8980]),
        ),
        (cluster.id, HostScope::Instance, deny("a.example")),
    ];

    for (id, scope, kind) in denied {
        let result = update_entry(&pool, actor, id, scope, &kind).await;
        assert!(
            matches!(result, Err(DbError::InvalidData(_))),
            "{kind:?}: {result:?}"
        );
    }

    assert!(audit(&pool, "egress_allowlist.update").await.is_empty());
}

#[tokio::test]
async fn an_edit_that_duplicates_another_rule_conflicts_and_changes_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let who = Who::new();
    let actor = who.actor();
    let df = project(&pool, "DF").await;
    create_host(&pool, HostScope::Instance, "github.com", 443).await;
    let project_rule = create_host(&pool, HostScope::Project(df), "github.com", 443).await;
    let first = add_deny_rule(&pool, "*.datadoghq.com", actor)
        .await
        .unwrap();
    add_deny_rule(&pool, "*.datadoghq.co", actor).await.unwrap();
    let a = add_entry(&pool, actor, None, &service("a", &[80]))
        .await
        .unwrap();
    add_entry(&pool, actor, None, &service("b", &[80]))
        .await
        .unwrap();
    let before = list_entries(&pool).await.unwrap();

    let widened = update_entry(
        &pool,
        actor,
        project_rule.id,
        HostScope::Instance,
        &host("github.com", 443),
    )
    .await;
    let denied = update_entry(
        &pool,
        actor,
        first.id,
        HostScope::Instance,
        &deny("*.datadoghq.co"),
    )
    .await;
    let renamed = update_entry(
        &pool,
        actor,
        a.id,
        HostScope::Instance,
        &service("b", &[80]),
    )
    .await;

    for (result, message) in [
        (widened, "this entry already exists"),
        (denied, "this deny rule already exists"),
        (renamed, "this entry already exists"),
    ] {
        assert!(
            matches!(&result, Err(DbError::Conflict(text)) if text == message),
            "{result:?}"
        );
    }
    assert_eq!(list_entries(&pool).await.unwrap(), before);
    // Saving a rule without a change is not a duplicate of itself.
    update_entry(
        &pool,
        actor,
        project_rule.id,
        HostScope::Project(df),
        &host("github.com", 443),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn editing_a_missing_rule_is_not_found() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let who = Who::new();
    let actor = who.actor();

    let result = update_entry(
        &pool,
        actor,
        Uuid::now_v7(),
        HostScope::Instance,
        &host("pypi.org", 443),
    )
    .await;

    assert!(matches!(result, Err(DbError::NotFound(_))), "{result:?}");
}

#[tokio::test]
async fn cluster_services_and_deny_rules_are_edited_audited_and_published() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let who = Who::new();
    let actor = who.actor();
    let df = project(&pool, "DF").await;
    let cluster = add_entry(&pool, actor, None, &service("scheduler", &[8980]))
        .await
        .unwrap();
    let rule = add_deny_rule(&pool, "*.datadoghq.co", actor).await.unwrap();
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let dns_service = EntryKind::ClusterService {
        namespace: "nativelink".into(),
        service: "scheduler".into(),
        ports: vec![8980, 8981],
        allow_dns: true,
    };

    let service_edit = update_entry(
        &pool,
        actor,
        cluster.id,
        HostScope::Project(df),
        &dns_service,
    )
    .await
    .unwrap();
    let deny_edit = update_entry(
        &pool,
        actor,
        rule.id,
        HostScope::Instance,
        &deny("*.datadoghq.com"),
    )
    .await
    .unwrap();

    assert_eq!(service_edit.kind, dns_service);
    assert_eq!(service_edit.project_key.as_deref(), Some("DF"));
    assert_eq!(deny_edit.kind, deny("*.datadoghq.com"));
    let published = notifications_before_marker(&pool, &mut notifications).await;
    assert_eq!(
        policy_publications(&published),
        2,
        "one per edit: {published:?}"
    );
    let denied = audit(&pool, "egress_deny_rule.update").await;
    assert_eq!(denied[0]["before"]["host"], "*.datadoghq.co");
    assert_eq!(denied[0]["after"]["host"], "*.datadoghq.com");
    let services = audit(&pool, "egress_allowlist.update").await;
    assert_eq!(services[0]["before"]["ports"], serde_json::json!([8980]));
    assert_eq!(
        services[0]["after"]["ports"],
        serde_json::json!([8980, 8981])
    );
}

#[tokio::test]
async fn creating_and_deleting_a_deny_rule_is_published_to_the_gateways() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let who = Who::new();
    let actor = who.actor();
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let created = add_deny_rule(&pool, "*.datadoghq.com", actor)
        .await
        .unwrap();
    let after_create = notifications_before_marker(&pool, &mut notifications).await;
    delete_entry(&pool, actor, created.id).await.unwrap();
    let after_delete = notifications_before_marker(&pool, &mut notifications).await;

    assert_eq!(policy_publications(&after_create), 1, "{after_create:?}");
    assert_eq!(policy_publications(&after_delete), 1, "{after_delete:?}");
}

#[tokio::test]
async fn make_global_replaces_the_project_rules_of_a_host_by_one_instance_rule() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let ops = project(&pool, "OPS").await;
    let df7 = task(&pool, df, 7).await;
    let host_name = "registry.npmjs.org";
    create_host(&pool, HostScope::Project(df), host_name, 443).await;
    create_host(&pool, HostScope::Project(ops), host_name, 443).await;
    let task_rule = create_host(&pool, HostScope::Task(df7), host_name, 443).await;
    let other_port = create_host(&pool, HostScope::Project(df), host_name, 80).await;
    let wildcard = create_host(&pool, HostScope::Project(ops), "*.npmjs.org", 443).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let who = Who::new();
    let actor = who.actor();

    let global = make_global(&pool, actor, host_name, 443).await.unwrap();

    assert_eq!(
        (global.project_key.clone(), global.task_key.clone()),
        (None, None)
    );
    let mut rest: Vec<_> = list_entries(&pool).await.unwrap();
    rest.retain(|e| e.id != global.id);
    let mut expected = vec![task_rule, other_port, wildcard];
    expected.sort_by_key(|e| e.id);
    rest.sort_by_key(|e| e.id);
    assert_eq!(rest, expected, "task rules, other ports and wildcards stay");
    assert_eq!(audit(&pool, "egress_allowlist.delete").await.len(), 2);
    let created = audit(&pool, "egress_allowlist.create").await;
    assert_eq!(created.last().unwrap()["host"], host_name);
    let published = notifications_before_marker(&pool, &mut notifications).await;
    assert_eq!(policy_publications(&published), 1, "{published:?}");
}

#[tokio::test]
async fn make_global_rolls_back_when_an_instance_rule_exists_or_no_project_rule() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    create_host(&pool, HostScope::Project(df), "github.com", 443).await;
    create_host(&pool, HostScope::Instance, "github.com", 443).await;
    let before = list_entries(&pool).await.unwrap();
    let who = Who::new();
    let actor = who.actor();

    let conflict = make_global(&pool, actor, "github.com", 443).await;
    let none = make_global(&pool, actor, "pypi.org", 443).await;

    assert!(
        matches!(conflict, Err(DbError::Conflict(_))),
        "{conflict:?}"
    );
    assert!(matches!(none, Err(DbError::NotFound(_))), "{none:?}");
    assert_eq!(list_entries(&pool).await.unwrap(), before);
    assert!(audit(&pool, "egress_allowlist.delete").await.is_empty());
}
