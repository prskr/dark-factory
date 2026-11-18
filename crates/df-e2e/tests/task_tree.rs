//! The task tree (`iter3-task-tree`, demo steps 1 and 4, `just e2e-local`): a reviewer that builds on
//! the implementation's branch, and a feature agent that merges the pull requests of its iterations
//! into its own branch while the next iteration starts on the merge. The children are found through
//! the coordinator the test created, by the marker of the prompt they were created with, never by a
//! predicted key. Needs the environment of `E2eEnv`, so the tests are ignored by default.

use anyhow::Result;
use df_core::TaskStatus;
use df_e2e::fixture;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db};

/// The prompt markers of the coordinators in `crates/fake-acp-agent/scripts/task-tree.yaml`.
const REVIEW_COORDINATOR: &str = "[tt-coord-review]";
const FEATURE_COORDINATOR: &str = "[tt-coord-feature]";

/// Demo step 1 (`task-tree` "Effective base branch"): the reviewer is created with the
/// implementation as `base_task`, so its workspace contains the implementation's commit.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_reviewer_based_on_the_implementation_finds_its_commit_in_its_workspace() -> Result<()> {
    fixture::run(
        "a_reviewer_based_on_the_implementation_finds_its_commit_in_its_workspace",
        |world| async move {
            let coordinator = world
                .session
                .create_task(
                    &world.env,
                    data::TASK_TREE_PROJECT_KEY,
                    &format!("{REVIEW_COORDINATOR} Get the implementation reviewed."),
                    None,
                )
                .await?;

            let implementation = world
                .child_of(&coordinator, data::TASK_TREE_IMPLEMENTATION)
                .await?;
            let review = world.child_of(&coordinator, data::TASK_TREE_REVIEW).await?;
            world.wait_for_runs(&review, 1).await?;

            assert_eq!(
                db::base_task_of(&world.pool, &review).await?.as_deref(),
                Some(implementation.as_str())
            );
            // The reviewer's script lists its `git log`: the implementation's commit is in it.
            assert!(
                db::transcript_events_containing(&world.pool, &review, data::TASK_TREE_COMMIT)
                    .await?
                    > 0,
                "the reviewer's workspace has the commit of {implementation}"
            );
            world.session.open_task(&world.env, &review).await?;
            world.session.expect_text("Base task", UI_TIMEOUT).await?;
            world.session.expect_text(&implementation, UI_TIMEOUT).await
        },
    )
    .await
}

/// Demo step 4 (`task-tree` "Merging subtask PRs", "Merge outcome", "Open subtasks"): the feature
/// agent creates two iterations on its own branch, the second after the first is done. It merges
/// each pull request itself, so the second iteration starts on the merge, and its own draft pull
/// request into the default branch appears.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_feature_agent_merges_its_iterations_and_the_next_one_starts_on_the_merge() -> Result<()>
{
    fixture::run(
        "a_feature_agent_merges_its_iterations_and_the_next_one_starts_on_the_merge",
        |world| async move {
            let feature = world
                .session
                .create_task(
                    &world.env,
                    data::TASK_TREE_PROJECT_KEY,
                    &format!("{FEATURE_COORDINATOR} Build the feature in two iterations."),
                    None,
                )
                .await?;
            let one = world
                .child_of(&feature, data::TASK_TREE_ITERATION_ONE)
                .await?;
            let two = world
                .child_of(&feature, data::TASK_TREE_ITERATION_TWO)
                .await?;

            // Both iterations end done through the feature agent's merges, then the feature agent
            // completes: it could not while an iteration's pull request was open.
            for key in [&one, &two] {
                world.wait_for_status(key, TaskStatus::Completed).await?;
                assert!(
                    db::pull_request_merged(&world.pool, key).await?,
                    "the merge of {key} is recorded"
                );
                assert_eq!(
                    db::base_task_of(&world.pool, key).await?.as_deref(),
                    Some(feature.as_str())
                );
            }
            // The feature agent's run ended `completed`. Its status is not checked: the merge of the
            // second iteration wakes it once more (the iteration is done), and that turn ends
            // without completing.
            eventually(
                &format!("the feature agent {feature} to complete a run"),
                RUN_TIMEOUT,
                || async {
                    Ok((db::completed_runs(&world.pool, &feature).await? >= 1).then_some(()))
                },
            )
            .await?;

            // The second iteration's workspace contained the first one's commit.
            assert!(
                db::transcript_events_containing(&world.pool, &two, "Add iteration one file")
                    .await?
                    > 0
            );
            // The feature branch has both iterations, and its pull request targets the default branch.
            let branch = format!("factory/task/{feature}");
            // A `pr.sync` job opens it, asynchronously.
            let pull = eventually(
                &format!("the pull request of {branch} into {}", data::BASE_BRANCH),
                RUN_TIMEOUT,
                || async { world.forgejo.pull_of(data::BASE_BRANCH, &branch).await },
            )
            .await?;
            assert_eq!(pull["base"]["ref"], data::BASE_BRANCH);

            world.session.open_task(&world.env, &feature).await?;
            world.session.open_subtasks_tab().await?;
            world
                .session
                .expect_region_text("#panel-subtasks", "2 of 2 done", UI_TIMEOUT)
                .await
        },
    )
    .await
}
