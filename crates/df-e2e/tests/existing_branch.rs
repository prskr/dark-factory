//! Tasks on existing branches (`iter3-branch-workflows`, `just e2e-local`): a task on a bot
//! branch with an open pull request is attached and the factory leaves the pull request's draft
//! state alone, a merge makes the task Done, and a rebase is pushed with lease. The tests work in
//! the repository `factory/existing-branch` of project `EB`, because they move and merge into its
//! `main`; their branches and files carry the unique suffix of the run. Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use std::time::Duration;

use anyhow::{Context, Result};
use df_core::TaskStatus;
use df_e2e::fixture;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually, status_reached};
use df_e2e::{data, db};
use serde_json::Value;

/// From the merge to the task being Done: the webhook delivery and the status change.
const MERGE_TIMEOUT: Duration = Duration::from_secs(30);

const PR_TITLE: &str = "Bump the dependency";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_task_on_a_bot_branch_is_attached_keeps_the_draft_state_and_is_done_on_merge()
-> Result<()> {
    fixture::run(
        "a_task_on_a_bot_branch_is_attached_keeps_the_draft_state_and_is_done_on_merge",
        |world| async move {
            let bot = world.forgejo_for(data::FORGE_EXISTING_BRANCH_REPO)?;
            let branch = world.unique("renovate/e2e-bump");
            bot.create_branch(&branch, data::BASE_BRANCH).await?;
            bot.create_file(
                &branch,
                &format!("{}.txt", world.unique("bot")),
                "bot change\n",
                "Bot change",
            )
            .await?;
            let number = bot
                .open_pull(&branch, data::BASE_BRANCH, PR_TITLE, true)
                .await?;

            let key = world
                .session
                .create_task_on_branch(
                    &world.env,
                    data::EXISTING_BRANCH_PROJECT_KEY,
                    &format!(
                        "{} Bump the dependency.",
                        data::EXISTING_BRANCH_ATTACHED_MARKER
                    ),
                    &branch,
                )
                .await?;

            // Attached at creation, before any run.
            let attached = db::task_pull_request(&world.pool, &key)
                .await?
                .context("the task is attached to the pull request")?;
            assert_eq!(attached.0, i64::try_from(number)?);
            world.session.open_task(&world.env, &key).await?;
            world
                .session
                .expect_text("attached", UI_TIMEOUT)
                .await
                .context("the details mark the pull request as attached")?;

            // The run completes the task: the summary is a comment, the pull request stays as it
            // was.
            world.wait_for_status(&key, TaskStatus::InReview).await?;
            let comment = eventually(
                "the summary comment on the pull request",
                RUN_TIMEOUT,
                || async {
                    Ok(bot.comments(number).await?.into_iter().find(|comment| {
                        comment["body"]
                            .as_str()
                            .is_some_and(|body| body.contains(data::EXISTING_BRANCH_SUMMARY))
                    }))
                },
            )
            .await?;
            assert!(comment["body"].is_string(), "{comment}");
            let pull = bot
                .pull_of(data::BASE_BRANCH, &branch)
                .await?
                .context("the pull request of the bot branch")?;
            assert_eq!(pull["draft"], true, "{:?}", pull["title"]);
            assert_eq!(pull["title"], format!("WIP: {PR_TITLE}"));
            assert_eq!(pull["state"], "open");

            // The merge makes the task Done. Forgejo refuses to merge a draft, so the maintainer
            // marks it ready first.
            bot.retitle_pull(number, PR_TITLE).await?;
            bot.merge_pull(number).await?;
            eventually("the task to be done", MERGE_TIMEOUT, || async {
                // Fails at once, naming the status, when the task ended otherwise.
                status_reached(
                    db::task_status(&world.pool, &key).await?,
                    TaskStatus::Completed,
                )
            })
            .await
        },
    )
    .await
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_rebase_of_an_existing_branch_is_pushed_with_lease() -> Result<()> {
    fixture::run(
        "a_rebase_of_an_existing_branch_is_pushed_with_lease",
        |world| async move {
            let bot = world.forgejo_for(data::FORGE_EXISTING_BRANCH_REPO)?;
            let branch = world.unique("renovate/e2e-rebase");
            bot.create_branch(&branch, data::BASE_BRANCH).await?;
            bot.create_file(
                &branch,
                &format!("{}.txt", world.unique("rebase")),
                "bot change\n",
                "Bot change",
            )
            .await?;
            // Main moves on, so the branch can only follow it by rewriting its commit. The commit
            // is this test's own (unique message): the parallel test merges into main too, and its
            // commits must not satisfy this check.
            let moved = world.unique("Move main on");
            bot.create_file(
                data::BASE_BRANCH,
                &format!("{}.txt", world.unique("moved-on")),
                "main moved\n",
                &moved,
            )
            .await?;

            let key = world
                .session
                .create_task_on_branch(
                    &world.env,
                    data::EXISTING_BRANCH_PROJECT_KEY,
                    &format!("{} Rebase the branch.", data::EXISTING_BRANCH_REBASE_MARKER),
                    &branch,
                )
                .await?;

            // The runtime pushes the rewritten branch after the turn: the branch now contains the
            // commit main moved on with, and the push was not rejected.
            eventually("the rebased branch", RUN_TIMEOUT, || async {
                let commits = bot.commits(&branch).await?;
                Ok(commits
                    .iter()
                    .any(|commit| {
                        commit
                            .pointer("/commit/message")
                            .and_then(Value::as_str)
                            .is_some_and(|message| message.starts_with(&moved))
                    })
                    .then_some(()))
            })
            .await?;
            let rejected = db::push_rejected_messages(&world.pool, &key).await?;
            assert!(rejected.is_empty(), "{rejected:?}");
            Ok(())
        },
    )
    .await
}
