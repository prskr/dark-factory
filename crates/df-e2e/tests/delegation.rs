//! Delegation without a forge webhook (`iter3-task-dependencies`, `just e2e-local`): a coordinator
//! creates an implementation and a review that depends on its completion; the review starts when
//! the implementation completes, the coordinator is woken by each child and relays the review to
//! the implementation for a second round. Needs the environment of `E2eEnv`, so the test is
//! ignored by default.

use anyhow::Result;
use df_e2e::browser::Session;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use sqlx::PgPool;

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn coordinator_delegates_implementation_and_review_and_relays_the_next_round() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;

    let coordinator = session
        .create_task(
            &env,
            data::DELEGATION_PROJECT_KEY,
            "[coordinator] Get the greeting written and reviewed.",
            None,
        )
        .await?;
    assert_eq!(coordinator, "DG-1");

    // The implementation completes, which releases the review; both wake the coordinator, whose
    // relay of the review reopens the implementation for a second completion.
    wait_for_runs(&pool, "DG-2", 2).await?;
    wait_for_runs(&pool, "DG-3", 1).await?;
    for key in ["DG-2", "DG-3"] {
        let status = db::task_status(&pool, key).await?;
        assert_eq!(status, "in_review", "{key}");
    }
    let wakeups = db::system_messages(
        &pool,
        &coordinator,
        df_core::task_transition::CHILD_ENDED_REASON,
    )
    .await?;
    assert_eq!(wakeups.len(), 3, "{wakeups:?}");
    assert!(
        wakeups
            .iter()
            .any(|m| m.contains("Review done, two findings."))
    );
    let relayed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_messages m JOIN tasks t ON t.id = m.task_id \
         WHERE t.key = 'DG-2' AND m.author ->> 'kind' = 'agent'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(relayed, 1);
    let parents: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM tasks c JOIN tasks p ON p.id = c.parent_id \
         WHERE p.key = 'DG-1' AND c.key IN ('DG-2', 'DG-3')",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(parents, 2);

    session.open_task(&env, "DG-3").await?;
    session.open_dependencies().await?;
    session.expect_text("Waits for", RUN_TIMEOUT).await?;
    session.expect_text("satisfied", RUN_TIMEOUT).await?;
    Ok(())
}

/// Waits until task `key` has ended `runs` runs and is in review.
async fn wait_for_runs(pool: &PgPool, key: &str, runs: i64) -> Result<()> {
    eventually(
        &format!("task {key} to have {runs} finished run(s)"),
        RUN_TIMEOUT,
        || async {
            let ended: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM runs r JOIN tasks t ON t.id = r.task_id \
                 WHERE t.key = $1 AND r.outcome = 'completed'",
            )
            .bind(key)
            .fetch_one(pool)
            .await?;
            Ok((ended >= runs && db::task_status(pool, key).await? == "in_review").then_some(()))
        },
    )
    .await
}
