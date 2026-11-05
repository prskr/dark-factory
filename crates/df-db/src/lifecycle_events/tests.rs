//! Tests of the lifecycle event storage, enqueue and envelope.

use chrono::TimeZone;
use df_core::ProjectId;
use serde_json::json;
use sqlx::PgPool;

use super::*;

const MIGRATION_VERSION: i64 = 20_261_104_000_002;

struct Fixture {
    project: Uuid,
    task: TaskId,
    run: RunId,
}

async fn seed(pool: &PgPool, key: &str) -> Fixture {
    let project_id = ProjectId::new().as_uuid();
    let persona_id = Uuid::now_v7();
    let task_id = TaskId::new();
    let run_id = RunId::new();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, $2, $2, $3, 'img')",
    )
    .bind(project_id)
    .bind(key)
    .bind(format!("https://forge.example.test/{key}.git"))
    .execute(pool)
    .await
    .unwrap();
    insert_persona(pool, persona_id, &format!("Linus {key}")).await;
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id) VALUES ($1, $2, 42, $3, 't', 'user', $4)",
    )
    .bind(task_id.as_uuid())
    .bind(project_id)
    .bind(format!("{key}-42"))
    .bind(persona_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number) VALUES ($1, $2, 3)")
        .bind(run_id.as_uuid())
        .bind(task_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    Fixture {
        project: project_id,
        task: task_id,
        run: run_id,
    }
}

/// A persona with a model provider; the migration test seeds a schema from before model
/// providers existed, where a persona needs none.
async fn insert_persona(pool: &PgPool, id: Uuid, name: &str) {
    let providers: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.model_providers')::text")
            .fetch_one(pool)
            .await
            .unwrap();
    if providers.is_none() {
        sqlx::query("INSERT INTO personas (id, name, system_prompt) VALUES ($1, $2, '')")
            .bind(id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        return;
    }
    let provider = crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, model, model_provider_id) \
         VALUES ($1, $2, '', 'test-model', $3)",
    )
    .bind(id)
    .bind(name)
    .bind(provider)
    .execute(pool)
    .await
    .unwrap();
}

async fn subscribe(pool: &PgPool, project_id: Uuid, events: &[&str]) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO event_subscriptions (id, project_id, url, events) VALUES ($1, $2, 'https://hooks.example.org/x', $3)")
        .bind(id)
        .bind(project_id)
        .bind(events)
        .execute(pool)
        .await
        .unwrap();
    id
}

fn sinks(instance: &[(&str, &[EventType])]) -> EventSinks {
    EventSinks::new(&LifecycleEventsConfig {
        signing_key_file: Some("/k/tls.key".into()),
        subscriptions: instance
            .iter()
            .map(|(url, events)| InstanceSubscription {
                url: (*url).into(),
                events: events.to_vec(),
            })
            .collect(),
    })
}

async fn record(pool: &PgPool, sinks: &EventSinks, event: &Event) -> Option<Uuid> {
    let mut tx = pool.begin().await.unwrap();
    let id = enqueue(&mut tx, sinks, event).await.unwrap();
    tx.commit().await.unwrap();
    id
}

async fn deliveries(pool: &PgPool) -> Vec<DeliverJob> {
    let payloads: Vec<Value> =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE kind = $1 ORDER BY id")
            .bind(DELIVER_KIND)
            .fetch_all(pool)
            .await
            .unwrap();
    payloads
        .into_iter()
        .map(|p| serde_json::from_value(p).unwrap())
        .collect()
}

async fn event_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM lifecycle_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn one_delivery_per_matching_project_and_instance_subscription() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    let other = seed(&pool, "ZZ").await;
    let all = subscribe(&pool, df.project, &[]).await;
    let destroyed = subscribe(&pool, df.project, &["sandbox.destroyed"]).await;
    let provisioned = subscribe(&pool, df.project, &["sandbox.provisioned"]).await;
    subscribe(&pool, other.project, &[]).await;
    let sinks = sinks(&[
        ("http://audit.svc/all", &[]),
        ("http://audit.svc/volumes", &[EventType::VolumeDestroyed]),
    ]);

    let event_id = record(&pool, &sinks, &Event::SandboxProvisioned { run_id: df.run })
        .await
        .unwrap();

    let jobs = deliveries(&pool).await;
    let targets: Vec<Target> = jobs.iter().map(|j| j.target.clone()).collect();
    assert!(jobs.iter().all(|j| j.event_id == event_id));
    assert_eq!(targets.len(), 3, "{targets:?}");
    assert!(targets.contains(&Target::Subscription {
        subscription_id: all
    }));
    assert!(targets.contains(&Target::Subscription {
        subscription_id: provisioned
    }));
    assert!(!targets.contains(&Target::Subscription {
        subscription_id: destroyed
    }));
    assert!(targets.contains(&Target::Instance {
        instance_url: "http://audit.svc/all".into()
    }));

    let (ty, project_id, payload): (String, Uuid, Value) =
        sqlx::query_as("SELECT type, project_id, payload FROM lifecycle_events WHERE id = $1")
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ty, "sandbox.provisioned");
    assert_eq!(project_id, df.project);
    assert_eq!(payload["id"], json!(format!("evt_{event_id}")));
    assert_eq!(payload["data"]["run"]["number"], json!(3));
    assert_eq!(payload["data"]["task"]["key"], json!("DF-42"));
    assert_eq!(payload["data"]["persona"]["name"], json!("Linus DF"));
    assert_eq!(
        payload["data"]["project"]["repo_url"],
        json!("https://forge.example.test/DF.git")
    );
}

#[tokio::test]
async fn nothing_is_written_without_a_matching_receiver() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    let other = seed(&pool, "ZZ").await;
    subscribe(&pool, df.project, &["sandbox.destroyed"]).await;
    subscribe(&pool, other.project, &[]).await;
    let sinks = sinks(&[("http://audit.svc/x", &[EventType::SandboxDestroyed])]);

    let id = record(
        &pool,
        &sinks,
        &Event::VolumeDestroyed {
            task_id: df.task,
            reason: VolumeDestroyedReason::Retention,
        },
    )
    .await;

    assert_eq!(id, None);
    assert!(deliveries(&pool).await.is_empty());
    assert_eq!(event_count(&pool).await, 0);
}

#[tokio::test]
async fn nothing_is_written_with_the_feature_off() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    subscribe(&pool, df.project, &[]).await;

    let id = record(
        &pool,
        &EventSinks::default(),
        &Event::SandboxProvisioning { run_id: df.run },
    )
    .await;

    assert_eq!(id, None);
    assert!(deliveries(&pool).await.is_empty());
    assert_eq!(event_count(&pool).await, 0);
}

#[tokio::test]
async fn a_rolled_back_change_records_no_event() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    subscribe(&pool, df.project, &[]).await;

    let mut tx = pool.begin().await.unwrap();
    let id = enqueue(
        &mut tx,
        &sinks(&[]),
        &Event::SandboxProvisioning { run_id: df.run },
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();

    assert!(id.is_some());
    assert!(deliveries(&pool).await.is_empty());
    assert_eq!(event_count(&pool).await, 0);
}

#[tokio::test]
async fn deleting_a_project_removes_its_subscriptions() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project_id = ProjectId::new().as_uuid();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'GO', 'g', 'r', 'i')",
    )
    .bind(project_id)
    .execute(&pool)
    .await
    .unwrap();
    subscribe(&pool, project_id, &[]).await;

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM event_subscriptions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn migration_backfills_the_volume_marker_of_tasks_with_an_existing_volume() {
    let pool = df_test_postgres::empty_pool().await;
    for migration in crate::MIGRATOR
        .iter()
        .filter(|m| m.version < MIGRATION_VERSION && m.migration_type.is_up_migration())
    {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
    }
    let with_run = seed(&pool, "DF").await;
    let deleted = seed(&pool, "DE").await;
    sqlx::query("UPDATE tasks SET volume_deleted_at = now() WHERE id = $1")
        .bind(deleted.task.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let without_run = seed(&pool, "NR").await;
    sqlx::query("DELETE FROM runs WHERE id = $1")
        .bind(without_run.run.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let ours = crate::MIGRATOR
        .iter()
        .find(|m| m.version == MIGRATION_VERSION)
        .unwrap();
    sqlx::raw_sql(ours.sql.clone())
        .execute(&pool)
        .await
        .unwrap();

    for (task, backfilled) in [(&with_run, true), (&deleted, false), (&without_run, false)] {
        let marker: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT volume_provisioned_at FROM tasks WHERE id = $1")
                .bind(task.task.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(marker.is_some(), backfilled);
    }
    let (provisioning, sandbox): (Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT provisioning_started_at, sandbox_started_at FROM runs WHERE id = $1",
    )
    .bind(with_run.run.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((provisioning, sandbox), (None, None));
}

fn subject(run: bool) -> Subject {
    Subject {
        project_id: Uuid::from_u128(1),
        project_key: "DF".into(),
        repo_url: "https://forge.example.org/df.git".into(),
        task_id: Uuid::from_u128(2),
        task_key: "DF-42".into(),
        persona_id: Uuid::from_u128(3),
        persona_name: "Linus".into(),
        run: run.then_some((Uuid::from_u128(4), 3)),
    }
}

#[test]
fn envelope_of_each_event_type() {
    let at = Utc.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
    let run_id = RunId::from_uuid(Uuid::from_u128(4));
    let id = Uuid::from_u128(9);
    let common = json!({
        "project": {
            "id": "00000000-0000-0000-0000-000000000001",
            "key": "DF",
            "repo_url": "https://forge.example.org/df.git",
        },
        "task": { "id": "00000000-0000-0000-0000-000000000002", "key": "DF-42" },
        "persona": { "id": "00000000-0000-0000-0000-000000000003", "name": "Linus" },
    });
    let run = json!({ "id": "00000000-0000-0000-0000-000000000004", "number": 3 });
    let cases = [
        (
            Event::VolumeProvisioned { run_id },
            "volume.provisioned",
            json!({ "run": run }),
        ),
        (
            Event::VolumeDestroyed {
                task_id: TaskId::from_uuid(Uuid::from_u128(2)),
                reason: VolumeDestroyedReason::Reset,
            },
            "volume.destroyed",
            json!({ "reason": "reset" }),
        ),
        (
            Event::SandboxProvisioning { run_id },
            "sandbox.provisioning",
            json!({ "run": run }),
        ),
        (
            Event::SandboxProvisioned { run_id },
            "sandbox.provisioned",
            json!({ "run": run }),
        ),
        (
            Event::SandboxProvisioningFailed {
                run_id,
                error: "image pull failed".into(),
            },
            "sandbox.provisioning_failed",
            json!({ "run": run, "error": "image pull failed" }),
        ),
        (
            Event::SandboxDestroyed {
                run_id,
                outcome: RunOutcome::Cancelled,
                started_at: None,
                ended_at: at,
            },
            "sandbox.destroyed",
            json!({
                "run": run,
                "outcome": "cancelled",
                "started_at": null,
                "ended_at": "2026-11-04T10:00:00.000Z",
            }),
        ),
    ];
    for (event, ty, extra) in cases {
        let mut data = common.clone();
        data.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let expected = json!({
            "id": "evt_00000000-0000-0000-0000-000000000009",
            "type": ty,
            "timestamp": "2026-11-04T10:00:00.000Z",
            "data": data,
        });
        let has_run = event.run_id().is_some();
        assert_eq!(
            envelope(id, at, &subject(has_run), &event),
            expected,
            "{ty}"
        );
    }
}

#[test]
fn destroyed_envelope_carries_the_sandbox_start() {
    let at = Utc.with_ymd_and_hms(2026, 11, 4, 10, 0, 0).unwrap();
    let event = Event::SandboxDestroyed {
        run_id: RunId::from_uuid(Uuid::from_u128(4)),
        outcome: RunOutcome::Completed,
        started_at: Some(at),
        ended_at: at,
    };
    let body = envelope(Uuid::from_u128(9), at, &subject(true), &event);
    assert_eq!(
        body["data"]["started_at"],
        json!("2026-11-04T10:00:00.000Z")
    );
    assert_eq!(body["data"]["outcome"], json!("completed"));
}

#[test]
fn deliver_job_payload_shapes() {
    let event_id = Uuid::from_u128(9);
    let job = DeliverJob {
        event_id,
        target: Target::Subscription {
            subscription_id: Uuid::from_u128(5),
        },
    };
    assert_eq!(
        serde_json::to_value(&job).unwrap(),
        json!({
            "event_id": "00000000-0000-0000-0000-000000000009",
            "target": { "subscription_id": "00000000-0000-0000-0000-000000000005" },
        })
    );
    let instance: DeliverJob = serde_json::from_value(json!({
        "event_id": event_id,
        "target": { "instance_url": "http://audit.svc/x" },
    }))
    .unwrap();
    assert_eq!(
        instance.target,
        Target::Instance {
            instance_url: "http://audit.svc/x".into()
        }
    );
}

#[tokio::test]
async fn a_delivery_resolves_its_target_until_the_event_or_subscription_is_gone() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    let subscription_id = subscribe(&pool, df.project, &[]).await;
    let event_id = record(
        &pool,
        &sinks(&[("http://audit.svc/all", &[])]),
        &Event::SandboxProvisioning { run_id: df.run },
    )
    .await
    .unwrap();
    let to_subscription = DeliverJob {
        event_id,
        target: Target::Subscription { subscription_id },
    };
    let to_instance = DeliverJob {
        event_id,
        target: Target::Instance {
            instance_url: "http://audit.svc/all".into(),
        },
    };

    let delivery = load_delivery(&pool, &to_subscription)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.url, "https://hooks.example.org/x");
    assert_eq!(delivery.payload["id"], json!(format!("evt_{event_id}")));
    assert_eq!(
        delivery.payload["timestamp"],
        json!(rfc3339(&delivery.occurred_at))
    );
    let delivery = load_delivery(&pool, &to_instance).await.unwrap().unwrap();
    assert_eq!(delivery.url, "http://audit.svc/all");

    sqlx::query("DELETE FROM event_subscriptions WHERE id = $1")
        .bind(subscription_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(load_delivery(&pool, &to_subscription).await.unwrap(), None);
    assert!(load_delivery(&pool, &to_instance).await.unwrap().is_some());

    sqlx::query("DELETE FROM lifecycle_events")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(load_delivery(&pool, &to_instance).await.unwrap(), None);
}

#[tokio::test]
async fn an_attempt_records_its_status_or_error_on_the_subscription() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    let subscription_id = subscribe(&pool, df.project, &[]).await;
    let last = || async {
        let row: (bool, Option<i16>, Option<String>) = sqlx::query_as(
            "SELECT last_attempt_at > now() - interval '1 minute', last_status, last_error
             FROM event_subscriptions WHERE id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        row
    };

    record_attempt(&pool, subscription_id, Some(503), None)
        .await
        .unwrap();
    assert_eq!(last().await, (true, Some(503), None));

    record_attempt(&pool, subscription_id, None, Some("timed out"))
        .await
        .unwrap();
    assert_eq!(last().await, (true, None, Some("timed out".into())));
}

#[tokio::test]
async fn cleanup_deletes_events_no_job_refers_to() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = seed(&pool, "DF").await;
    let sinks = sinks(&[("http://audit.svc/all", &[])]);
    let kept = record(
        &pool,
        &sinks,
        &Event::SandboxProvisioning { run_id: df.run },
    )
    .await
    .unwrap();
    let orphan = record(&pool, &sinks, &Event::SandboxProvisioned { run_id: df.run })
        .await
        .unwrap();
    sqlx::query("DELETE FROM outbox WHERE payload->>'event_id' = $1")
        .bind(orphan.to_string())
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(delete_unreferenced(&pool).await.unwrap(), 1);

    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM lifecycle_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, [kept]);
}
