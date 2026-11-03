//! Tests of the project subscription storage and its audit entries.

use df_core::{Principal, UserId};
use serde_json::{Value, json};

use super::*;

async fn project(pool: &PgPool, key: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, $2, $2, $3, 'img')",
    )
    .bind(id)
    .bind(key)
    .bind(format!("https://forge.example.test/{key}.git"))
    .execute(pool)
    .await
    .unwrap();
    id
}

fn admin() -> (Principal, Vec<String>) {
    (
        Principal::human(UserId::new(), "Ada"),
        vec!["board-member".into(), "admin".into()],
    )
}

fn input(url: &str, events: &[EventType]) -> SubscriptionInput {
    SubscriptionInput {
        url: url.into(),
        events: events.to_vec(),
    }
}

async fn audit_entries(pool: &PgPool) -> Vec<(String, String, Value)> {
    sqlx::query_as(
        "SELECT action, resource_id, details FROM audit_log \
         WHERE resource_type = 'event_subscription' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn create_update_and_delete_are_audited_with_url_and_events() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let (principal, roles) = admin();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };

    let id = save(
        &pool,
        actor,
        df,
        None,
        &input(
            "https://hooks.example.org/a",
            &[EventType::SandboxDestroyed],
        ),
    )
    .await
    .unwrap();
    let stored = get(&pool, df, id).await.unwrap().unwrap();
    assert_eq!(stored.url, "https://hooks.example.org/a");
    assert_eq!(stored.events, [EventType::SandboxDestroyed]);

    save(
        &pool,
        actor,
        df,
        Some(id),
        &input("https://hooks.example.org/b", &[]),
    )
    .await
    .unwrap();
    let listed = list(&pool, df).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].url, "https://hooks.example.org/b");
    assert!(listed[0].events.is_empty());

    delete(&pool, actor, df, id).await.unwrap();
    assert!(list(&pool, df).await.unwrap().is_empty());

    let entries = audit_entries(&pool).await;
    let actions: Vec<&str> = entries.iter().map(|(a, _, _)| a.as_str()).collect();
    assert_eq!(
        actions,
        [
            "event_subscription.create",
            "event_subscription.update",
            "event_subscription.delete"
        ]
    );
    assert!(entries.iter().all(|(_, r, _)| *r == id.to_string()));
    assert_eq!(
        entries[1].2,
        json!({
            "id": id,
            "project": "DF",
            "before": {"url": "https://hooks.example.org/a", "events": ["sandbox.destroyed"]},
            "after": {"url": "https://hooks.example.org/b", "events": []},
        })
    );
    assert_eq!(entries[2].2["after"], Value::Null);
    assert_eq!(entries[2].2["before"]["url"], "https://hooks.example.org/b");
}

#[tokio::test]
async fn another_projects_subscription_is_not_found() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let web = project(&pool, "WEB").await;
    let (principal, roles) = admin();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let id = save(
        &pool,
        actor,
        df,
        None,
        &input("https://h.example.org/", &[]),
    )
    .await
    .unwrap();

    assert!(get(&pool, web, id).await.unwrap().is_none());
    assert!(list(&pool, web).await.unwrap().is_empty());
    let update = save(
        &pool,
        actor,
        web,
        Some(id),
        &input("https://x.example.org/", &[]),
    )
    .await;
    assert!(matches!(update, Err(DbError::NotFound(_))), "{update:?}");
    let removed = delete(&pool, actor, web, id).await;
    assert!(matches!(removed, Err(DbError::NotFound(_))), "{removed:?}");
    assert_eq!(
        get(&pool, df, id).await.unwrap().unwrap().url,
        "https://h.example.org/"
    );
    assert_eq!(audit_entries(&pool).await.len(), 1);
}

#[tokio::test]
async fn a_load_returns_the_last_status_and_refuses_an_unknown_event_type() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO event_subscriptions (id, project_id, url, events, last_attempt_at, last_status)
         VALUES ($1, $2, 'https://h.example.org/', '{volume.destroyed}', now(), 503)",
    )
    .bind(id)
    .bind(df)
    .execute(&pool)
    .await
    .unwrap();
    let loaded = get(&pool, df, id).await.unwrap().unwrap();
    assert_eq!(loaded.events, [EventType::VolumeDestroyed]);
    assert_eq!(loaded.last_status, Some(503));

    sqlx::query("UPDATE event_subscriptions SET events = '{volume.renamed}' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let loaded = get(&pool, df, id).await;
    assert!(matches!(loaded, Err(DbError::Parse(_))), "{loaded:?}");
    let listed = list(&pool, df).await;
    assert!(matches!(listed, Err(DbError::Parse(_))), "{listed:?}");
}
