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
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model_provider_id) VALUES ($1, 'Linus', '', 'claude-code', (SELECT id FROM model_providers LIMIT 1))",
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
        findings: Vec::new(),
        approval_requested: false,
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

/// The `forge_activity` payloads of the transcript, oldest first.
async fn forge_events(pool: &PgPool) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT payload FROM transcript_events WHERE kind = 'forge_activity' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn accepted_task_branch_push_writes_one_branch_pushed_event_with_its_ref() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();

    record_push(&pool, &record(run, push, PushVerdict::Forwarding))
        .await
        .unwrap();
    assert!(
        forge_events(&pool).await.is_empty(),
        "forwarding writes nothing"
    );
    record_push(&pool, &record(run, push, PushVerdict::Accepted))
        .await
        .unwrap();
    record_push(&pool, &record(run, push, PushVerdict::Accepted))
        .await
        .unwrap();

    let events = forge_events(&pool).await;
    assert_eq!(
        events.len(),
        1,
        "the same final verdict twice writes one event"
    );
    let pushed = &events[0]["forgeActivity"]["branchPushed"];
    assert_eq!(pushed["ref"], "refs/heads/factory/task/DF-1");
    assert_eq!(pushed["oldSha"], "0".repeat(40).as_str());
    assert_eq!(pushed["newSha"], "a".repeat(40).as_str());
    assert_eq!(pushed["verdict"], "PUSH_VERDICT_ACCEPTED");
    let key: String = sqlx::query_scalar("SELECT source_key FROM transcript_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(key, format!("push:{push}"));
}

#[tokio::test]
async fn rejected_task_branch_push_carries_the_reason_of_the_ref() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let mut rejected = record(run, new_id(), PushVerdict::Rejected);
    rejected.refs[0].reason = "df:non-fast-forward".into();

    record_push(&pool, &rejected).await.unwrap();

    let events = forge_events(&pool).await;
    let pushed = &events[0]["forgeActivity"]["branchPushed"];
    assert_eq!(pushed["verdict"], "PUSH_VERDICT_REJECTED");
    assert_eq!(pushed["reason"], "df:non-fast-forward");
}

#[tokio::test]
async fn the_event_carries_the_verdict_of_the_task_branch_not_of_the_push() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let mut mixed = record(run, new_id(), PushVerdict::UpstreamFailed);
    mixed.refs[0].verdict = PushVerdict::Accepted;
    mixed.refs.push(RefUpdate {
        name: "refs/heads/factory/wip/DF-1".into(),
        verdict: PushVerdict::UpstreamFailed,
        reason: "df:forge-refused".into(),
        ..mixed.refs[0].clone()
    });

    record_push(&pool, &mixed).await.unwrap();

    let events = forge_events(&pool).await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["forgeActivity"]["branchPushed"]["verdict"],
        "PUSH_VERDICT_ACCEPTED"
    );
}

#[tokio::test]
async fn push_that_only_updates_the_wip_branch_writes_no_event() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let mut wip = record(run, new_id(), PushVerdict::Accepted);
    wip.refs[0].name = "refs/heads/factory/wip/DF-1".into();

    record_push(&pool, &wip).await.unwrap();

    assert!(forge_events(&pool).await.is_empty());
}

#[tokio::test]
async fn an_upstream_failed_task_branch_push_writes_an_event_with_that_verdict() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let mut failed = record(run, new_id(), PushVerdict::UpstreamFailed);
    failed.refs[0].reason = "df: forge unreachable".into();

    record_push(&pool, &failed).await.unwrap();

    let events = forge_events(&pool).await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["forgeActivity"]["branchPushed"]["verdict"],
        "PUSH_VERDICT_UPSTREAM_FAILED"
    );
}

#[tokio::test]
async fn a_final_verdict_waits_for_the_run_lock_before_it_touches_the_task_row() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let run = seed_run(&pool).await;
    let push = new_id();
    // The forwarding record exists, so the final one inserts nothing that references the run.
    record_push(&pool, &record(run, push, PushVerdict::Forwarding))
        .await
        .unwrap();
    let task_id: Uuid = sqlx::query_scalar("SELECT task_id FROM runs WHERE id = $1")
        .bind(run.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let writer = {
        let pool = pool.clone();
        tokio::spawn(async move {
            record_push(&pool, &record(run, push, PushVerdict::Accepted))
                .await
                .unwrap()
        })
    };
    // Wait on the condition: the writer waits for a lock.
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
            "the writer did not wait for the run lock"
        );
        tokio::task::yield_now().await;
    }
    // The task row is still free: the writer has not touched it while it waits for the run.
    let mut probe = pool.begin().await.unwrap();
    let free = sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(task_id)
        .execute(&mut *probe)
        .await;
    probe.rollback().await.unwrap();
    blocker.commit().await.unwrap();

    assert!(free.is_ok(), "the task row was locked before the run row");
    assert_eq!(writer.await.unwrap(), RecordOutcome::Advanced);
}

fn new_id() -> Uuid {
    RunId::new().as_uuid()
}
