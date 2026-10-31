//! Tests of the push records (container-backed).

use super::*;

/// Seeds a project, persona, task and run; returns the run.
async fn seed_run(pool: &PgPool) -> RunId {
    let (project, persona, task, run) = (new_id(), new_id(), new_id(), new_id());
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, 'DF', 'DF', 'https://forge.example.test/o/r.git', 'img', 'main')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness) VALUES ($1, 'Linus', '', 'claude-code')",
    )
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id)
         VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3)",
    )
    .bind(task)
    .bind(project)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number) VALUES ($1, $2, 1)")
        .bind(run)
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
    RunId::from_uuid(run)
}

fn record(run_id: RunId, push_id: Uuid, verdict: PushVerdict) -> PushRecord {
    PushRecord {
        push_id,
        run_id,
        gateway_pod: "gateway-0".into(),
        refs: vec![RefUpdate {
            name: "refs/heads/factory/task/DF-1".into(),
            old_sha: "0".repeat(40),
            new_sha: "a".repeat(40),
            verdict,
            reason: String::new(),
        }],
        verdict,
        bytes: 1024,
        duration: Duration::from_millis(250),
    }
}

async fn stored(pool: &PgPool, push_id: Uuid) -> (i64, String) {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT verdict FROM git_pushes WHERE push_id = $1")
        .bind(push_id)
        .fetch_all(pool)
        .await
        .unwrap();
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'git.push' AND details->>'push_id' = $1",
    )
    .bind(push_id.to_string())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "one record per push");
    (audits, rows[0].0.clone())
}

#[tokio::test]
async fn git_push_retried_record_is_stored_and_audited_once() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();

    let first = record_push(&pool, &record(run, push, PushVerdict::Rejected))
        .await
        .unwrap();
    let retry = record_push(&pool, &record(run, push, PushVerdict::Rejected))
        .await
        .unwrap();

    assert_eq!(first, RecordOutcome::Inserted);
    assert_eq!(retry, RecordOutcome::Unchanged);
    assert_eq!(stored(&pool, push).await, (1, "rejected".into()));
}

#[tokio::test]
async fn git_push_forwarding_then_accepted_writes_one_audit_entry_with_the_agent_actor() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();

    record_push(&pool, &record(run, push, PushVerdict::Forwarding))
        .await
        .unwrap();
    assert_eq!(stored(&pool, push).await, (0, "forwarding".into()));
    let outcome = record_push(&pool, &record(run, push, PushVerdict::Accepted))
        .await
        .unwrap();

    assert_eq!(outcome, RecordOutcome::Advanced);
    assert_eq!(stored(&pool, push).await, (1, "accepted".into()));
    let actor: serde_json::Value =
        sqlx::query_scalar("SELECT actor FROM audit_log WHERE action = 'git.push'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(actor["display_name"], "Linus");
    assert_eq!(actor["run_id"], run.to_string());
}

#[tokio::test]
async fn git_push_backwards_verdict_is_ignored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();

    record_push(&pool, &record(run, push, PushVerdict::Accepted))
        .await
        .unwrap();
    let late = record_push(&pool, &record(run, push, PushVerdict::Forwarding))
        .await
        .unwrap();

    assert_eq!(late, RecordOutcome::Unchanged);
    assert_eq!(stored(&pool, push).await, (1, "accepted".into()));
}

#[tokio::test]
async fn git_push_with_a_known_push_id_from_another_run_is_rejected() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let other = new_id();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number, state)
         SELECT $1, task_id, 2, 'succeeded' FROM runs WHERE id = $2",
    )
    .bind(other)
    .bind(run.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let push = new_id();
    record_push(&pool, &record(run, push, PushVerdict::Forwarding))
        .await
        .unwrap();

    let err = record_push(
        &pool,
        &record(RunId::from_uuid(other), push, PushVerdict::Accepted),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DbError::InvalidData(_)), "{err:?}");
    assert_eq!(stored(&pool, push).await, (0, "forwarding".into()));
    let owner: Uuid = sqlx::query_scalar("SELECT run_id FROM git_pushes WHERE push_id = $1")
        .bind(push)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(owner, run.as_uuid());
}

#[tokio::test]
async fn git_push_repeated_accepted_after_forwarding_is_audited_once() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();

    let outcomes = [
        PushVerdict::Forwarding,
        PushVerdict::Accepted,
        PushVerdict::Accepted,
    ];
    let mut results = Vec::new();
    for verdict in outcomes {
        results.push(
            record_push(&pool, &record(run, push, verdict))
                .await
                .unwrap(),
        );
    }

    assert_eq!(
        results,
        [
            RecordOutcome::Inserted,
            RecordOutcome::Advanced,
            RecordOutcome::Unchanged
        ]
    );
    assert_eq!(stored(&pool, push).await, (1, "accepted".into()));
}

#[tokio::test]
async fn git_push_final_verdict_is_not_replaced_by_another_final_verdict() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let cases = [
        (
            PushVerdict::Accepted,
            PushVerdict::UpstreamFailed,
            "accepted",
        ),
        (PushVerdict::Rejected, PushVerdict::Accepted, "rejected"),
    ];

    for (first, second, kept) in cases {
        let push = new_id();
        record_push(&pool, &record(run, push, first)).await.unwrap();
        let outcome = record_push(&pool, &record(run, push, second))
            .await
            .unwrap();

        assert_eq!(
            outcome,
            RecordOutcome::Unchanged,
            "{first:?} then {second:?}"
        );
        assert_eq!(stored(&pool, push).await, (1, kept.into()));
    }
}

#[tokio::test]
async fn git_push_unknown_run_is_not_found() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    let err = record_push(
        &pool,
        &record(RunId::new(), new_id(), PushVerdict::Rejected),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DbError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn git_push_stale_forwarding_is_marked_unknown_and_a_late_verdict_still_lands() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let (old, young) = (new_id(), new_id());
    record_push(&pool, &record(run, old, PushVerdict::Forwarding))
        .await
        .unwrap();
    record_push(&pool, &record(run, young, PushVerdict::Forwarding))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE git_pushes SET created_at = now() - interval '11 minutes' WHERE push_id = $1",
    )
    .bind(old)
    .execute(&pool)
    .await
    .unwrap();

    let marked = mark_stale_forwarding(&pool, Duration::from_secs(600))
        .await
        .unwrap();

    assert_eq!(marked, 1);
    assert_eq!(stored(&pool, old).await, (0, "unknown".into()));
    assert_eq!(stored(&pool, young).await, (0, "forwarding".into()));
    record_push(&pool, &record(run, old, PushVerdict::Accepted))
        .await
        .unwrap();
    assert_eq!(stored(&pool, old).await, (1, "accepted".into()));
}

fn new_id() -> Uuid {
    RunId::new().as_uuid()
}
