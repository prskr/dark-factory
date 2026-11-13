//! The demo profile of the seed (`seed --demo`): what it writes into a migrated database, that a
//! second run changes nothing and that it leaves the end-to-end fixtures alone.

use anyhow::Result;
use df_db::MIGRATOR;
use df_e2e::seed::{self, demo};

async fn scalar(pool: &df_db::PgPool, sql: &'static str) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).fetch_one(pool).await?)
}

async fn strings(pool: &df_db::PgPool, sql: &'static str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(sql).fetch_all(pool).await?)
}

/// Everything the demo profile writes, as counts.
async fn demo_counts(pool: &df_db::PgPool) -> Result<Vec<i64>> {
    let mut counts = Vec::new();
    for table in [
        "projects",
        "personas",
        "tasks",
        "runs",
        "transcript_events",
        "model_usage",
        "task_artifacts",
        "agent_requests",
        "task_dependencies",
        "audit_log",
    ] {
        let sql = sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}"));
        counts.push(sqlx::query_scalar(sql).fetch_one(pool).await?);
    }
    Ok(counts)
}

#[tokio::test]
async fn demo_profile_writes_projects_personas_and_tasks_in_every_state() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;

    let report = demo::run(&pool).await?;

    assert!(report.hints.is_empty(), "{:?}", report.hints);
    assert_eq!(
        strings(&pool, "SELECT key FROM projects ORDER BY key").await?,
        ["OPS", "PAY", "WEB"]
    );
    assert_eq!(
        strings(&pool, "SELECT name FROM personas ORDER BY name").await?,
        ["Ada", "Grace", "Linus"]
    );
    let states = strings(
        &pool,
        "SELECT status || ' ' || count(*) FROM tasks GROUP BY status ORDER BY status",
    )
    .await?;
    assert_eq!(
        states,
        [
            "awaiting_dependencies 1",
            "blocked 1",
            "cancelled 1",
            "completed 2",
            "failed 1",
            "in_review 3",
            "queued 1",
            "rate_limited 1",
            "waiting 3",
        ]
    );
    // A parent with two subtasks, one of which waits for the other.
    assert_eq!(
        strings(
            &pool,
            "SELECT c.key FROM tasks c JOIN tasks p ON p.id = c.parent_id
             WHERE p.title = 'Checkout redesign' ORDER BY c.key"
        )
        .await?,
        ["WEB-2", "WEB-3"]
    );
    assert_eq!(
        strings(
            &pool,
            "SELECT t.key || ' waits for ' || d.key || ' (' || td.kind || ')'
             FROM task_dependencies td JOIN tasks t ON t.id = td.task_id
             JOIN tasks d ON d.id = td.depends_on_task_id"
        )
        .await?,
        ["WEB-3 waits for WEB-2 (done)"]
    );
    // A pull request opened outside the factory, and a linked issue.
    assert_eq!(
        strings(
            &pool,
            "SELECT key || ' ' || branch || ' #' || pr_number FROM tasks WHERE pr_attached"
        )
        .await?,
        ["PAY-1 renovate/serde-1.x #341"]
    );
    assert_eq!(
        strings(
            &pool,
            "SELECT key FROM tasks WHERE issue_number IS NOT NULL"
        )
        .await?,
        ["WEB-4"]
    );
    // The blocked task asks a question with suggested answers.
    assert_eq!(
        strings(
            &pool,
            "SELECT t.key || ' ' || jsonb_array_length(r.payload -> 'suggested_answers')
             FROM agent_requests r JOIN tasks t ON t.id = r.task_id WHERE r.state = 'pending'"
        )
        .await?,
        ["PAY-2 3"]
    );
    // Every run is finished: nothing for the startup sweep of a control plane to end.
    assert_eq!(
        scalar(
            &pool,
            "SELECT count(*) FROM runs WHERE state NOT IN ('succeeded', 'failed', 'cancelled')"
        )
        .await?,
        0
    );
    assert_eq!(scalar(&pool, "SELECT count(*) FROM runs").await?, 14);
    assert!(scalar(&pool, "SELECT count(*) FROM transcript_events").await? > 50);
    assert!(scalar(&pool, "SELECT count(*) FROM model_usage").await? > 100);
    Ok(())
}

#[tokio::test]
async fn completed_demo_tasks_with_a_pull_request_have_a_merged_one() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;

    demo::run(&pool).await?;

    assert_eq!(
        strings(
            &pool,
            "SELECT key FROM tasks WHERE status = 'completed' AND pr_number IS NOT NULL
               AND pr_merged_at IS NOT NULL AND pr_merged_at <= first_done_at ORDER BY key"
        )
        .await?,
        ["OPS-2", "WEB-4"]
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT count(*) FROM tasks WHERE pr_merged_at IS NOT NULL AND status <> 'completed'"
        )
        .await?,
        0,
        "only completed tasks have a merge time"
    );
    Ok(())
}

#[tokio::test]
async fn demo_profile_keeps_the_resume_times_in_the_future() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    demo::run(&pool).await?;
    let waiting = "SELECT key FROM tasks WHERE resume_at > now() ORDER BY key";
    assert_eq!(strings(&pool, waiting).await?, ["PAY-4", "WEB-6"]);

    // The resume time of the weekly limit passed since the seeding.
    sqlx::query("UPDATE tasks SET resume_at = now() - interval '1 hour' WHERE key = 'PAY-4'")
        .execute(&pool)
        .await?;
    let report = demo::run(&pool).await?;

    assert!(report.hints.is_empty(), "{:?}", report.hints);
    assert_eq!(strings(&pool, waiting).await?, ["PAY-4", "WEB-6"]);
    let retry_after: bool = sqlx::query_scalar(
        "SELECT r.retry_after > now() FROM runs r JOIN tasks t ON t.id = r.task_id WHERE t.key = 'PAY-4'",
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        retry_after,
        "the limit of the run moved with the resume time"
    );
    Ok(())
}

#[tokio::test]
async fn demo_profile_hints_at_a_task_the_scheduler_started() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    demo::run(&pool).await?;
    sqlx::query(
        "UPDATE tasks SET resume_at = now() - interval '1 hour', status = 'failed' WHERE key = 'PAY-4'",
    )
    .execute(&pool)
    .await?;

    let report = demo::run(&pool).await?;

    assert_eq!(report.hints.len(), 1, "{:?}", report.hints);
    assert!(report.hints[0].starts_with("PAY-4 "), "{:?}", report.hints);
    Ok(())
}

#[tokio::test]
async fn demo_profile_writes_the_artifacts_of_the_report_task() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;

    demo::run(&pool).await?;

    assert_eq!(
        strings(
            &pool,
            "SELECT a.name || ' ' || a.kind || ' ' || a.mime_type || ' ' || a.size
             FROM task_artifacts a JOIN tasks t ON t.id = a.task_id
             WHERE t.key = 'WEB-5' ORDER BY a.name"
        )
        .await?
        .iter()
        .map(|line| line
            .rsplit_once(' ')
            .map_or(line.as_str(), |(head, _size)| head))
        .collect::<Vec<_>>(),
        [
            "profiler-output.log text text/plain",
            "search-latency-report.md text text/markdown",
            "search-latency.png image image/png",
        ]
    );
    assert_eq!(
        scalar(&pool, "SELECT count(DISTINCT task_id) FROM task_artifacts").await?,
        2,
        "the report task and the task with the test log"
    );
    let dimensions: (Option<i32>, Option<i32>) = sqlx::query_as(
        "SELECT width, height FROM task_artifacts WHERE name = 'search-latency.png'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(dimensions, (Some(640), Some(360)));
    // The Artifacts tab and the transcript show where they came from.
    assert_eq!(
        scalar(
            &pool,
            "SELECT count(*) FROM transcript_events WHERE source_key LIKE 'artifact:%'"
        )
        .await?,
        4
    );
    Ok(())
}

#[tokio::test]
async fn second_demo_run_changes_nothing() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    demo::run(&pool).await?;
    let before = demo_counts(&pool).await?;

    demo::run(&pool).await?;

    assert_eq!(demo_counts(&pool).await?, before);
    Ok(())
}

#[tokio::test]
async fn demo_profile_leaves_the_e2e_fixtures_unchanged() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    seed::run(&pool).await?;
    let fixtures = |pool: df_db::PgPool| async move {
        let mut rows = strings(
            &pool,
            "SELECT to_jsonb(p)::text FROM projects p WHERE key NOT IN ('WEB', 'PAY', 'OPS') ORDER BY key",
        )
        .await?;
        rows.extend(
            strings(
                &pool,
                "SELECT to_jsonb(p)::text FROM personas p WHERE name = 'e2e'",
            )
            .await?,
        );
        rows.extend(
            strings(
                &pool,
                "SELECT to_jsonb(c)::text FROM credential_refs c WHERE name IN ('forgejo-bot', 'fake-llm') ORDER BY name",
            )
            .await?,
        );
        anyhow::Ok(rows)
    };
    let before = fixtures(pool.clone()).await?;
    assert_eq!(before.len(), 13 + 1 + 2);

    demo::run(&pool).await?;

    assert_eq!(fixtures(pool.clone()).await?, before);
    assert_eq!(
        scalar(&pool, "SELECT count(*) FROM projects").await?,
        13 + 3
    );
    Ok(())
}
