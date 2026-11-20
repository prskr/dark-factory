//! Tests of the transcript writes of the control plane (container-backed).

use df_proto::transcript::v1::{
    ArtifactKind, ArtifactWritten, BranchPushed, FactoryActivity, ForgeActivity, PushVerdict,
    event::Kind, factory_activity, forge_activity,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::blobs::PgBlobStore;
use crate::tasks::tests::seed_project_and_persona;

/// Seeds a project, persona and task; returns the task and a function-local run factory input.
async fn seed_task(pool: &PgPool) -> TaskId {
    let (project, persona, task) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, 'DF', 'DF', 'https://forge.example.test/o/r.git', 'img', 'main')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id) VALUES ($1, 'Linus', '', 'claude-code', 'test-model', (SELECT id FROM model_providers LIMIT 1))",
    )
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, created_by)
         VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(task)
    .bind(project)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    TaskId::from_uuid(task)
}

async fn seed_run(pool: &PgPool, task: TaskId, number: i32) -> RunId {
    let run = Uuid::now_v7();
    sqlx::query("INSERT INTO runs (id, task_id, number) VALUES ($1, $2, $3)")
        .bind(run)
        .bind(task.as_uuid())
        .bind(number)
        .execute(pool)
        .await
        .unwrap();
    RunId::from_uuid(run)
}

fn pushed() -> Event {
    forge_event(ForgeActivity {
        kind: Some(forge_activity::Kind::BranchPushed(BranchPushed {
            r#ref: "refs/heads/factory/task/DF-1".into(),
            old_sha: "0".repeat(40),
            new_sha: "a".repeat(40),
            verdict: PushVerdict::Accepted.into(),
            reason: String::new(),
        })),
    })
}

/// One control-plane event in its own writer, published at once.
async fn append_control_event(
    tx: &mut Transaction<'_, Postgres>,
    lock: &RunTaskLock,
    source_key: &str,
    event: &Event,
) -> Result<Option<i64>, DbError> {
    let mut writer = TranscriptWriter::new(lock);
    let id = writer.append_control(tx, source_key, event).await?;
    writer.finish(tx).await?;
    Ok(id)
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM transcript_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn transcript_row_needs_exactly_one_of_seq_and_source_key() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;
    let insert = |seq: Option<i64>, key: Option<&'static str>| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO transcript_events (task_id, run_id, seq, source_key, occurred_at, kind, payload)
                 VALUES ($1, $2, $3, $4, now(), 'x', '{}')",
            )
            .bind(task.as_uuid())
            .bind(run.as_uuid())
            .bind(seq)
            .bind(key)
            .execute(&pool)
            .await
        }
    };

    assert!(insert(Some(1), None).await.is_ok());
    assert!(insert(None, Some("k")).await.is_ok());
    assert!(insert(Some(2), Some("k2")).await.is_err(), "both set");
    assert!(insert(None, None).await.is_err(), "neither set");
    assert!(insert(None, Some("k")).await.is_err(), "duplicate key");
}

#[tokio::test]
async fn control_event_is_stored_once_per_source_key_as_forge_activity() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let first = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    let again = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(first.is_some());
    assert_eq!(again, None);
    assert_eq!(count(&pool).await, 1);
    let (kind, key, seq, payload): (String, Option<String>, Option<i64>, Value) =
        sqlx::query_as("SELECT kind, source_key, seq, payload FROM transcript_events")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (kind.as_str(), key.as_deref(), seq),
        ("forge_activity", Some("push:1"), None)
    );
    let stored: Event = serde_json::from_value(payload).unwrap();
    assert!(matches!(stored.kind, Some(Kind::ForgeActivity(_))));
}

#[tokio::test]
async fn control_event_publishes_its_id_only_when_inserted() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener
        .listen(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let id = append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    append_control_event(&mut tx, &lock, "push:1", &pushed())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let first = listener.recv().await.unwrap();
    let payload: Value = serde_json::from_str(first.payload()).unwrap();
    assert_eq!(payload["id"], id);
    // The duplicate published nothing: the next notification is a marker we publish ourselves.
    sqlx::query("SELECT pg_notify($1, 'marker')")
        .bind(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(listener.recv().await.unwrap().payload(), "marker");
}

#[tokio::test]
async fn control_event_waits_for_the_task_lock_so_ids_become_visible_in_order() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let writer = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
            let id = append_control_event(&mut tx, &lock, "push:1", &pushed())
                .await
                .unwrap();
            tx.commit().await.unwrap();
            id
        })
    };
    // Wait on the condition: a backend waits for a lock.
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        assert!(
            !writer.is_finished(),
            "writer did not wait for the task lock"
        );
        tokio::task::yield_now().await;
    }
    assert_eq!(count(&pool).await, 0);
    blocker.commit().await.unwrap();

    assert!(writer.await.unwrap().is_some());
    assert_eq!(count(&pool).await, 1);
}

fn artifact_written() -> Event {
    factory_event(FactoryActivity {
        kind: Some(factory_activity::Kind::ArtifactWritten(ArtifactWritten {
            name: "findings.md".into(),
            kind: ArtifactKind::Text.into(),
            mime_type: "text/markdown".into(),
            size: 12,
        })),
    })
}

#[tokio::test]
async fn factory_activity_is_stored_once_per_source_key_and_published() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let first = append_control_event(&mut tx, &lock, "artifact:1", &artifact_written())
        .await
        .unwrap();
    let again = append_control_event(&mut tx, &lock, "artifact:1", &artifact_written())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(first.is_some());
    assert_eq!(again, None);
    let (kind, key, payload): (String, Option<String>, Value) =
        sqlx::query_as("SELECT kind, source_key, payload FROM transcript_events")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (kind.as_str(), key.as_deref()),
        ("factory_activity", Some("artifact:1"))
    );
    let stored: Event = serde_json::from_value(payload).unwrap();
    assert!(matches!(stored.kind, Some(Kind::FactoryActivity(_))));
}

#[tokio::test]
async fn control_event_of_another_kind_is_refused() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = seed_task(&pool).await;
    let run = seed_run(&pool, task, 1).await;
    let event = Event {
        kind: Some(Kind::Notice(df_proto::transcript::v1::Notice::default())),
        ..Default::default()
    };

    let mut tx = pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, run, task).await.unwrap();
    let refused = append_control_event(&mut tx, &lock, "x", &event).await;

    assert!(matches!(refused, Err(DbError::InvalidData(_))));
}

/// A task with its undelivered assignment prompt and a first run.
struct Fixture {
    pool: PgPool,
    task: TaskId,
    prompt: df_core::PromptId,
    run: RunId,
}

impl Fixture {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (project_id, persona_id) =
            seed_project_and_persona(&pool, &"DF".parse().unwrap(), "Linus")
                .await
                .unwrap();
        let (task, prompt) = create_task_of(&pool, project_id, persona_id, "Mine").await;
        let run = seed_run(&pool, task, 1).await;
        Self {
            pool,
            task,
            prompt,
            run,
        }
    }

    async fn other_task(&self) -> (TaskId, df_core::PromptId) {
        let row: (Uuid, Uuid) = sqlx::query_as("SELECT project_id, persona_id FROM tasks LIMIT 1")
            .fetch_one(&self.pool)
            .await
            .unwrap();
        create_task_of(
            &self.pool,
            df_core::ProjectId::from_uuid(row.0),
            df_core::PersonaId::from_uuid(row.1),
            "Other",
        )
        .await
    }

    async fn prepared(&self, seq: u64, kind: Kind) -> PreparedEvent {
        let event = Event {
            schema_version: 1,
            kind: Some(kind),
            ..Default::default()
        };
        prepare(&PgBlobStore::new(self.pool.clone()), seq, event)
            .await
            .unwrap()
    }

    /// Appends `events` (runtime) of `run` in one transaction with one writer and finishes it.
    async fn append_runtime(&self, run: RunId, events: &[PreparedEvent]) -> Vec<Option<i64>> {
        let mut tx = self.pool.begin().await.unwrap();
        let lock = lock_run_and_task(&mut tx, run, self.task).await.unwrap();
        let mut writer = TranscriptWriter::new(&lock);
        let mut ids = Vec::new();
        for event in events {
            ids.push(writer.append_runtime(&mut tx, event).await.unwrap());
        }
        writer.finish(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        ids
    }

    async fn undelivered(&self, task: TaskId) -> usize {
        crate::list_undelivered_messages(&self.pool, task)
            .await
            .unwrap()
            .len()
    }
}

async fn create_task_of(
    pool: &PgPool,
    project_id: df_core::ProjectId,
    persona_id: df_core::PersonaId,
    title: &str,
) -> (TaskId, df_core::PromptId) {
    let (task, message) = crate::create_task(
        pool,
        crate::NewTask {
            project_id,
            persona_id,
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: df_core::Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    (task.id, message.id)
}

fn chunk(text: &str) -> Kind {
    Kind::AgentMessageChunk(df_proto::transcript::v1::AgentMessageChunk {
        message_id: "m".into(),
        content: Some(df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent { text: text.into() },
            )),
            hidden: false,
        }),
    })
}

fn user_message(prompt: df_core::PromptId) -> Kind {
    Kind::UserMessage(df_proto::transcript::v1::UserMessage {
        prompt_id: prompt.to_string(),
        author: None,
        content: Vec::new(),
    })
}

#[tokio::test]
async fn runtime_events_are_stored_once_per_run_and_seq_and_ids_increase_across_runs() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(f.run.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let run2 = seed_run(&f.pool, f.task, 2).await;

    let first = f.prepared(1, chunk("a")).await;
    let ids = f.append_runtime(f.run, &[first.clone(), first]).await;
    let later = f
        .append_runtime(run2, &[f.prepared(1, chunk("b")).await])
        .await;

    assert!(ids[0].is_some());
    assert_eq!(ids[1], None);
    assert!(later[0].unwrap() > ids[0].unwrap());
    assert_eq!(count(&f.pool).await, 2);
}

#[tokio::test]
async fn a_stored_user_message_marks_its_queued_prompt_delivered() {
    let f = Fixture::new().await;

    f.append_runtime(f.run, &[f.prepared(1, user_message(f.prompt)).await])
        .await;

    assert_eq!(f.undelivered(f.task).await, 0);
    let delivered_by: Option<Uuid> =
        sqlx::query_scalar("SELECT delivered_run_id FROM task_messages WHERE id = $1")
            .bind(f.prompt.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(delivered_by, Some(f.run.as_uuid()));
}

/// Queues a webhook message for `f.task` and returns its prompt id.
async fn queue_review(f: &Fixture) -> df_core::PromptId {
    let event = df_core::webhooks::WebhookEvent::ReviewSubmitted {
        number: 12,
        head: "factory/task/DF-1".into(),
        actor: "alice".into(),
        verdict: df_core::seams::ReviewVerdict::ChangesRequested,
    };
    let mut tx = f.pool.begin().await.unwrap();
    assert!(
        crate::webhooks::queue_event_tx(&mut tx, f.task, &event)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    crate::list_undelivered_messages(&f.pool, f.task)
        .await
        .unwrap()
        .last()
        .unwrap()
        .id
}

async fn webhook_event_keys(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT source_key FROM transcript_events WHERE source_key LIKE 'webhook:%'")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn delivering_a_webhook_message_appends_one_webhook_event() {
    let f = Fixture::new().await;
    let prompt = queue_review(&f).await;

    f.append_runtime(f.run, &[f.prepared(1, user_message(prompt)).await])
        .await;

    assert_eq!(
        webhook_event_keys(&f.pool).await,
        [format!("webhook:{prompt}")]
    );
    let (payload, run): (Value, Uuid) = sqlx::query_as(
        "SELECT payload, run_id FROM transcript_events WHERE kind = 'forge_activity'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let review = &payload["forgeActivity"]["webhookReceived"]["reviewSubmitted"];
    assert_eq!(review["actor"], "alice", "{payload}");
    assert_eq!(
        review["verdict"],
        "WEBHOOK_REVIEW_VERDICT_CHANGES_REQUESTED"
    );
    // 64-bit numbers are JSON strings in the proto JSON mapping.
    assert_eq!(review["number"], "12", "{payload}");
    assert_eq!(
        run,
        f.run.as_uuid(),
        "the event belongs to the delivering run"
    );
}

#[tokio::test]
async fn a_prompt_delivered_in_an_earlier_run_gets_no_webhook_event_in_a_later_run() {
    let f = Fixture::new().await;
    let prompt = queue_review(&f).await;
    f.append_runtime(f.run, &[f.prepared(1, user_message(prompt)).await])
        .await;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(f.run.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let run2 = seed_run(&f.pool, f.task, 2).await;

    f.append_runtime(run2, &[f.prepared(1, user_message(prompt)).await])
        .await;

    assert_eq!(webhook_event_keys(&f.pool).await.len(), 1);
}

#[tokio::test]
async fn redelivering_a_webhook_message_appends_no_second_event() {
    let f = Fixture::new().await;
    let prompt = queue_review(&f).await;

    f.append_runtime(f.run, &[f.prepared(1, user_message(prompt)).await])
        .await;
    f.append_runtime(f.run, &[f.prepared(2, user_message(prompt)).await])
        .await;

    assert_eq!(webhook_event_keys(&f.pool).await.len(), 1);
}

#[tokio::test]
async fn delivering_other_messages_appends_no_webhook_event() {
    let f = Fixture::new().await;

    // The assignment (`task.assigned`) carries no stored event.
    f.append_runtime(f.run, &[f.prepared(1, user_message(f.prompt)).await])
        .await;

    assert!(webhook_event_keys(&f.pool).await.is_empty());
}

#[tokio::test]
async fn a_runtime_user_message_cannot_forge_a_webhook_event() {
    let f = Fixture::new().await;
    let plain = crate::queue_message(
        &f.pool,
        f.task,
        df_core::Principal::human(df_core::UserId::new(), "Ada"),
        Vec::new(),
    )
    .await
    .unwrap();
    let forged = Kind::UserMessage(df_proto::transcript::v1::UserMessage {
        prompt_id: plain.id.to_string(),
        author: Some(df_proto::transcript::v1::Principal {
            kind: Some(df_proto::transcript::v1::principal::Kind::System(
                df_proto::transcript::v1::SystemPrincipal {
                    reason: "webhook".into(),
                },
            )),
        }),
        content: Vec::new(),
    });

    f.append_runtime(f.run, &[f.prepared(1, forged).await])
        .await;

    assert!(webhook_event_keys(&f.pool).await.is_empty());
}

#[tokio::test]
async fn a_duplicate_seq_carrying_another_prompt_marks_nothing() {
    let f = Fixture::new().await;
    // A second prompt of the same task.
    let second = crate::queue_message(
        &f.pool,
        f.task,
        df_core::Principal::human(df_core::UserId::new(), "Ada"),
        Vec::new(),
    )
    .await
    .unwrap();

    f.append_runtime(f.run, &[f.prepared(1, user_message(f.prompt)).await])
        .await;
    let ids = f
        .append_runtime(f.run, &[f.prepared(1, user_message(second.id)).await])
        .await;

    assert_eq!(ids, [None]);
    let undelivered = crate::list_undelivered_messages(&f.pool, f.task)
        .await
        .unwrap();
    assert_eq!(
        undelivered.iter().map(|m| m.id).collect::<Vec<_>>(),
        [second.id]
    );
}

#[tokio::test]
async fn a_user_message_cannot_deliver_the_prompt_of_another_task() {
    let f = Fixture::new().await;
    let (other, foreign) = f.other_task().await;

    // The runtime of `f.task` names a prompt of `other`.
    f.append_runtime(f.run, &[f.prepared(1, user_message(foreign)).await])
        .await;

    assert_eq!(f.undelivered(other).await, 1);
}

#[tokio::test]
async fn runtime_events_refuse_forge_and_factory_activity() {
    let f = Fixture::new().await;
    let forged = f
        .prepared(1, Kind::ForgeActivity(ForgeActivity::default()))
        .await;

    let mut tx = f.pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, f.run, f.task).await.unwrap();
    let refused = TranscriptWriter::new(&lock)
        .append_runtime(&mut tx, &forged)
        .await;

    assert!(matches!(refused, Err(DbError::InvalidData(_))));
}

#[tokio::test]
async fn control_events_are_keyed_by_source_key_not_by_seq() {
    let f = Fixture::new().await;
    let mut tx = f.pool.begin().await.unwrap();
    let lock = lock_run_and_task(&mut tx, f.run, f.task).await.unwrap();
    let mut writer = TranscriptWriter::new(&lock);
    // The same text under the key of another event is a new event.
    let a = writer
        .append_control(&mut tx, "k1", &pushed())
        .await
        .unwrap();
    let b = writer
        .append_control(&mut tx, "k2", &pushed())
        .await
        .unwrap();
    let dup = writer
        .append_control(&mut tx, "k1", &pushed())
        .await
        .unwrap();
    writer.finish(&mut tx).await.unwrap();
    tx.commit().await.unwrap();

    assert!(a.is_some() && b.is_some());
    assert_eq!(dup, None);
}

#[tokio::test]
async fn finish_publishes_one_notification_with_the_newest_id_and_none_without_a_stored_event() {
    let f = Fixture::new().await;
    let mut listener = sqlx::postgres::PgListener::connect_with(&f.pool)
        .await
        .unwrap();
    listener
        .listen(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .await
        .unwrap();

    let events = [
        f.prepared(1, chunk("a")).await,
        f.prepared(2, chunk("b")).await,
    ];
    let ids = f.append_runtime(f.run, &events).await;
    // Nothing new: both are duplicates.
    f.append_runtime(f.run, &events).await;
    sqlx::query("SELECT pg_notify($1, 'marker')")
        .bind(crate::notify::CHANNEL_TRANSCRIPT_EVENT)
        .execute(&f.pool)
        .await
        .unwrap();

    let first = listener.recv().await.unwrap();
    let payload: Value = serde_json::from_str(first.payload()).unwrap();
    assert_eq!(payload["id"], ids[1].unwrap());
    assert_eq!(listener.recv().await.unwrap().payload(), "marker");
}

#[tokio::test]
async fn the_turn_state_follows_the_last_turn_event() {
    let f = Fixture::new().await;
    assert!(!turn_open(&f.pool, f.run).await.unwrap());
    assert_eq!(last_turn_end(&f.pool, f.run).await.unwrap(), None);

    f.append_runtime(
        f.run,
        &[f.prepared(
            1,
            Kind::TurnStarted(df_proto::transcript::v1::TurnStarted::default()),
        )
        .await],
    )
    .await;
    assert!(turn_open(&f.pool, f.run).await.unwrap());
    assert_eq!(last_turn_end(&f.pool, f.run).await.unwrap(), None);

    f.append_runtime(
        f.run,
        &[f.prepared(
            2,
            Kind::TurnEnded(df_proto::transcript::v1::TurnEnded::default()),
        )
        .await],
    )
    .await;
    assert!(!turn_open(&f.pool, f.run).await.unwrap());
    assert!(last_turn_end(&f.pool, f.run).await.unwrap().is_some());
    assert_eq!(
        last_turn_event(&f.pool, f.run).await.unwrap().unwrap().0,
        KIND_TURN_ENDED
    );
}
