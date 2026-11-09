//! Interrupting a turn (`interrupt-turn`, `just e2e-local`): a message queued during a long turn
//! and the Interrupt button end that turn; the transcript says who interrupted, and the next turn
//! of the same run starts with the interrupt line and the message. Needs the environment of
//! `E2eEnv`, so the test is ignored by default.

use anyhow::Result;
use df_e2e::browser::Session;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};

/// The display name of the e2e login.
const BOARD_MEMBER: &str = "E2E Admin";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn interrupting_a_long_turn_steers_the_agent_within_the_same_run() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;

    let key = session
        .create_task(&env, data::INTERRUPT_PROJECT_KEY, "Long task", None)
        .await?;
    // The first turn stays open until it is interrupted.
    session
        .expect_text(data::INTERRUPT_STARTED, RUN_TIMEOUT)
        .await?;

    session.send_message("Use the other API").await?;
    session.interrupt_turn().await?;

    session
        .expect_region_text(
            "#transcript",
            &format!("Interrupted by {BOARD_MEMBER}"),
            RUN_TIMEOUT,
        )
        .await?;
    // The next turn got the interrupt line and the queued message.
    session
        .expect_text(data::INTERRUPT_NOTED, RUN_TIMEOUT)
        .await?;
    session
        .expect_text(data::INTERRUPT_SWITCHED, RUN_TIMEOUT)
        .await?;

    eventually("the task to wait", RUN_TIMEOUT, || async {
        Ok((db::task_status(&pool, &key).await? == "waiting").then_some(()))
    })
    .await?;
    let runs = db::runs(&pool, &key).await?;
    assert_eq!(runs.len(), 1, "the interrupt kept the run: {runs:?}");
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'run.interrupt'")
            .fetch_one(&pool)
            .await?;
    assert!(audited >= 1, "the interrupt is audited");
    Ok(())
}
