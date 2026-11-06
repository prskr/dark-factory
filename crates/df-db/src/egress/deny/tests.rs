use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::egress::{allowlist_covers, http_host_entries};

async fn insert_deny(
    pool: &PgPool,
    scope: &str,
    project_id: Option<Uuid>,
    host: Option<&str>,
    port: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO egress_allowlist (id, scope, project_id, kind, host, port)
         VALUES ($1, $2, $3, 'http-deny', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(scope)
    .bind(project_id)
    .bind(host)
    .bind(port)
    .execute(pool)
    .await
    .map(|_done| ())
}

async fn project_and_task(pool: &PgPool) -> (Uuid, Uuid) {
    let (project, persona, task) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ($1, 'DF', 'DF', 'https://example.test/r.git', 'img:latest')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query("INSERT INTO personas (id, name, system_prompt, model, model_provider_id) VALUES ($1, 'p', 'x', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ($1, $2, 7, 'DF-7', 't', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(task)
    .bind(project)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    (project, task)
}

// Container-backed tests: need a PostgreSQL container (testcontainers).
#[tokio::test]
async fn a_deny_row_needs_a_host_no_port_and_instance_scope() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project, _task) = project_and_task(&pool).await;

    insert_deny(&pool, "instance", None, Some("*.datadoghq.com"), None)
        .await
        .unwrap();
    for (scope, project_id, host, port) in [
        ("instance", None, Some("a.example"), Some(443)),
        ("project", Some(project), Some("a.example"), None),
        ("instance", None, None, None),
    ] {
        assert!(
            insert_deny(&pool, scope, project_id, host, port)
                .await
                .is_err(),
            "{scope} {host:?} {port:?} accepted"
        );
    }
}

#[tokio::test]
async fn deny_rules_match_exact_and_wildcard_hosts_on_any_port() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    insert_deny(&pool, "instance", None, Some("*.datadoghq.com"), None)
        .await
        .unwrap();
    insert_deny(&pool, "instance", None, Some("telemetry.example"), None)
        .await
        .unwrap();

    for (host, expected) in [
        (
            "http-intake.logs.us5.datadoghq.com",
            Some("*.datadoghq.com"),
        ),
        ("telemetry.example", Some("telemetry.example")),
        ("datadoghq.com", None),
        ("pypi.org", None),
    ] {
        assert_eq!(
            deny_rule_for(&pool, host).await.unwrap().as_deref(),
            expected,
            "{host}"
        );
    }
}

#[tokio::test]
async fn deny_rules_are_invisible_to_allowlist_readers() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (_project, task) = project_and_task(&pool).await;
    insert_deny(&pool, "instance", None, Some("telemetry.example"), None)
        .await
        .unwrap();

    assert!(http_host_entries(&pool).await.unwrap().is_empty());
    let mut tx = pool.begin().await.unwrap();
    for port in [80, 443] {
        assert!(
            !allowlist_covers(&mut tx, task, "telemetry.example", port)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn an_allow_rule_of_a_task_still_covers_a_host_that_a_deny_rule_matches() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project, df7) = project_and_task(&pool).await;
    let other = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ($1, $2, 8, 'DF-8', 't', 'user', (SELECT id FROM personas LIMIT 1),
                 '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(other)
    .bind(project)
    .execute(&pool)
    .await
    .unwrap();
    let host = "http-intake.logs.us5.datadoghq.com";
    let principal = df_core::Principal::human(df_core::UserId::new(), "Peter");
    let actor = Actor {
        principal: &principal,
        roles: &[],
    };
    let mut tx = pool.begin().await.unwrap();
    create_deny_rule(&mut tx, "*.datadoghq.com", actor, None)
        .await
        .unwrap();
    crate::egress::create_http_host_entry(
        &mut tx,
        crate::egress::HostScope::Task(df7),
        host,
        443,
        actor,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    assert!(allowlist_covers(&mut tx, df7, host, 443).await.unwrap());
    assert!(!allowlist_covers(&mut tx, other, host, 443).await.unwrap());
    drop(tx);
    let entries = http_host_entries(&pool).await.unwrap();
    assert_eq!(entries.len(), 1, "the snapshot holds the allow rule only");
    assert_eq!(
        deny_rule_for(&pool, host).await.unwrap().as_deref(),
        Some("*.datadoghq.com")
    );
}
