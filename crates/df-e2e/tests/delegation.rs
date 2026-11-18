//! Delegation without a forge webhook (`iter3-task-dependencies`, `just e2e-local`): a coordinator
//! creates an implementation and a review that depends on its completion; the review starts when
//! the implementation completes, the coordinator is woken by each child and relays the review to
//! the implementation for a second round. The children are found through the coordinator the test
//! created, by the marker of the prompt they were created with, never by a predicted key. Needs
//! the environment of `E2eEnv`, so the tests are ignored by default.

use anyhow::Result;
use df_core::TaskStatus;
use df_e2e::fixture;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db};

/// The prompt markers of `crates/fake-acp-agent/scripts/delegation.yaml` and `research.yaml`.
const IMPLEMENTATION: &str = "[impl]";
const REVIEW: &str = "[review]";
const RESEARCH: &str = "[research]";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn coordinator_delegates_implementation_and_review_and_relays_the_next_round() -> Result<()> {
    fixture::run(
        "coordinator_delegates_implementation_and_review_and_relays_the_next_round",
        |world| async move {
            let coordinator = world
                .session
                .create_task(
                    &world.env,
                    data::DELEGATION_PROJECT_KEY,
                    "[coordinator] Get the greeting written and reviewed.",
                    None,
                )
                .await?;

            // The implementation completes, which releases the review; both wake the coordinator,
            // whose relay of the review reopens the implementation for a second completion.
            let implementation = world.child_of(&coordinator, IMPLEMENTATION).await?;
            let review = world.child_of(&coordinator, REVIEW).await?;
            world.wait_for_runs(&implementation, 2).await?;
            world.wait_for_runs(&review, 1).await?;
            for key in [&implementation, &review] {
                let status = db::task_status(&world.pool, key).await?;
                assert_eq!(status, TaskStatus::InReview, "{key}");
            }
            let wakeups = db::system_messages(
                &world.pool,
                &coordinator,
                df_core::system_reason::CHILD_ENDED,
            )
            .await?;
            assert_eq!(wakeups.len(), 3, "{wakeups:?}");
            assert!(
                wakeups
                    .iter()
                    .any(|m| m.contains("Review done, two findings."))
            );
            assert_eq!(db::agent_messages(&world.pool, &implementation).await?, 1);
            assert_eq!(
                db::children(&world.pool, &coordinator).await?,
                [implementation.clone(), review.clone()],
                "the coordinator created exactly these two, in this order"
            );

            world.session.open_task(&world.env, &review).await?;
            world.session.open_dependencies().await?;
            world.session.expect_text("Waits for", UI_TIMEOUT).await?;
            world.session.expect_text("satisfied", UI_TIMEOUT).await
        },
    )
    .await
}

/// Hand-over with an artifact (`iter3-artifacts`, demo step 5): the researcher writes
/// `findings.md`, its completion wakes the coordinator with the artifact in the message, the
/// coordinator reads it, and the artifact shows in the UI.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_researcher_hands_its_findings_to_the_coordinator_as_an_artifact() -> Result<()> {
    fixture::run(
        "a_researcher_hands_its_findings_to_the_coordinator_as_an_artifact",
        |world| async move {
            let coordinator = world
                .session
                .create_task(
                    &world.env,
                    data::RESEARCH_PROJECT_KEY,
                    "[coordinator] Find out why the login loops.",
                    None,
                )
                .await?;

            let researcher = world.child_of(&coordinator, RESEARCH).await?;
            world.wait_for_runs(&researcher, 1).await?;
            let artifact = format!("{researcher}/{}", data::RESEARCH_ARTIFACT);
            eventually(
                "the coordinator to read the findings",
                RUN_TIMEOUT,
                || async {
                    let reads =
                        db::artifact_reads(&world.pool, &researcher, data::RESEARCH_ARTIFACT)
                            .await?;
                    Ok((reads > 0).then_some(()))
                },
            )
            .await?;
            let wakeups = db::system_messages(
                &world.pool,
                &coordinator,
                df_core::system_reason::CHILD_ENDED,
            )
            .await?;
            assert!(wakeups.iter().any(|m| m.contains(&artifact)), "{wakeups:?}");

            world.session.open_task(&world.env, &researcher).await?;
            world.session.expect_text("Artifacts", UI_TIMEOUT).await?;
            world
                .session
                .page
                .goto(
                    &format!(
                        "{}/tasks/{researcher}/artifacts/{}",
                        world.env.base_url,
                        data::RESEARCH_ARTIFACT
                    ),
                    None,
                )
                .await?;
            world
                .session
                .expect_text(data::RESEARCH_FINDING, UI_TIMEOUT)
                .await
        },
    )
    .await
}
