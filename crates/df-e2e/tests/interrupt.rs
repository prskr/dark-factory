//! Interrupting a turn (`interrupt-turn`, `just e2e-local`): a message queued during a long turn
//! and the Interrupt button end that turn; the transcript says who interrupted, and the next turn
//! of the same run starts with the interrupt line and the message. Needs the environment of
//! `E2eEnv`, so the test is ignored by default.

use anyhow::Result;
use df_core::TaskStatus;
use df_e2e::fixture;
use df_e2e::wait::RUN_TIMEOUT;
use df_e2e::{data, db};

/// The display name of the e2e login.
const BOARD_MEMBER: &str = "E2E Admin";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn interrupting_a_long_turn_steers_the_agent_within_the_same_run() -> Result<()> {
    fixture::run(
        "interrupting_a_long_turn_steers_the_agent_within_the_same_run",
        |world| async move {
            let key = world
                .session
                .create_task(&world.env, data::INTERRUPT_PROJECT_KEY, "Long task", None)
                .await?;
            // The first turn stays open until it is interrupted.
            world
                .session
                .expect_text(data::INTERRUPT_STARTED, RUN_TIMEOUT)
                .await?;

            world.session.send_message("Use the other API").await?;
            world.session.interrupt_turn().await?;

            world
                .session
                .expect_region_text(
                    "#transcript",
                    &format!("Interrupted by {BOARD_MEMBER}"),
                    RUN_TIMEOUT,
                )
                .await?;
            // The next turn got the interrupt line and the queued message.
            world
                .session
                .expect_text(data::INTERRUPT_NOTED, RUN_TIMEOUT)
                .await?;
            world
                .session
                .expect_text(data::INTERRUPT_SWITCHED, RUN_TIMEOUT)
                .await?;

            world.wait_for_status(&key, TaskStatus::Waiting).await?;
            let runs = db::runs(&world.pool, &key).await?;
            assert_eq!(runs.len(), 1, "the interrupt kept the run: {runs:?}");
            assert_eq!(
                db::run_audit_entries(&world.pool, &key, "run.interrupt").await?,
                1,
                "the interrupt of the run is audited"
            );
            Ok(())
        },
    )
    .await
}
