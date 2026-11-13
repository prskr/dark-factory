//! Tasks on existing branches (`iter3-branch-workflows`, `just e2e-local`): a task on a bot
//! branch with an open pull request is attached and the factory leaves the pull request's draft
//! state alone, a merge makes the task Done, and a rebase is pushed with lease. Needs the
//! environment of `E2eEnv`, so the tests are ignored by default.

use std::time::Duration;

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::forgejo::Forgejo;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use serde_json::Value;
use sqlx::PgPool;

/// From the merge to the task being Done: the webhook delivery and the status change.
const MERGE_TIMEOUT: Duration = Duration::from_secs(30);

const BOT_BRANCH: &str = "renovate/e2e-bump";
const REBASE_BRANCH: &str = "renovate/e2e-rebase";
const PR_TITLE: &str = "Bump the dependency";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_task_on_a_bot_branch_is_attached_keeps_the_draft_state_and_is_done_on_merge()
-> Result<()> {
    let env = E2eEnv::from_env()?;
    let bot = Forgejo::new(&env.forgejo_url, &env.forgejo_token)?;
    bot.create_branch(BOT_BRANCH, data::BASE_BRANCH).await?;
    bot.create_file(BOT_BRANCH, "bot.txt", "bot change\n", "Bot change")
        .await?;
    let number = bot
        .open_pull(BOT_BRANCH, data::BASE_BRANCH, PR_TITLE, true)
        .await?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;

    let key = session
        .create_task_on_branch(
            &env,
            data::EXISTING_BRANCH_PROJECT_KEY,
            &format!(
                "{} Bump the dependency.",
                data::EXISTING_BRANCH_ATTACHED_MARKER
            ),
            BOT_BRANCH,
        )
        .await?;

    // Attached at creation, before any run.
    let attached = db::task_pull_request(&pool, &key)
        .await?
        .context("the task is attached to the pull request")?;
    assert_eq!(attached.0, i64::try_from(number)?);
    session.open_task(&env, &key).await?;
    session
        .expect_text("attached", RUN_TIMEOUT)
        .await
        .context("the details mark the pull request as attached")?;

    // The run completes the task: the summary is a comment, the pull request stays as it was.
    wait_for_status(&pool, &key, "in_review").await?;
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
    let pull = pull_of(&bot, BOT_BRANCH)
        .await?
        .context("the pull request of the bot branch")?;
    assert_eq!(pull["draft"], true, "{:?}", pull["title"]);
    assert_eq!(pull["title"], format!("WIP: {PR_TITLE}"));
    assert_eq!(pull["state"], "open");

    // The merge makes the task Done.
    // Forgejo refuses to merge a draft, so the maintainer marks it ready first.
    bot.retitle_pull(number, PR_TITLE).await?;
    bot.merge_pull(number).await?;
    eventually("the task to be done", MERGE_TIMEOUT, || async {
        Ok((db::task_status(&pool, &key).await? == "completed").then_some(()))
    })
    .await
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_rebase_of_an_existing_branch_is_pushed_with_lease() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let bot = Forgejo::new(&env.forgejo_url, &env.forgejo_token)?;
    bot.create_branch(REBASE_BRANCH, data::BASE_BRANCH).await?;
    bot.create_file(REBASE_BRANCH, "rebase.txt", "bot change\n", "Bot change")
        .await?;
    // Main moves on, so the branch can only follow it by rewriting its commit.
    let moved = "Move main on";
    bot.create_file(data::BASE_BRANCH, "moved-on.txt", "main moved\n", moved)
        .await?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;

    let key = session
        .create_task_on_branch(
            &env,
            data::EXISTING_BRANCH_PROJECT_KEY,
            &format!("{} Rebase the branch.", data::EXISTING_BRANCH_REBASE_MARKER),
            REBASE_BRANCH,
        )
        .await?;

    // The runtime pushes the rewritten branch after the turn: the branch now contains the commit
    // main moved on with, and the push was not rejected.
    eventually("the rebased branch", RUN_TIMEOUT, || async {
        let commits = bot.commits(REBASE_BRANCH).await?;
        Ok(commits
            .iter()
            .any(|commit| {
                commit
                    .pointer("/commit/message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.starts_with(moved))
            })
            .then_some(()))
    })
    .await?;
    let rejected = db::push_rejected_messages(&pool, &key).await?;
    assert!(rejected.is_empty(), "{rejected:?}");
    Ok(())
}

async fn wait_for_status(pool: &PgPool, key: &str, status: &str) -> Result<()> {
    eventually(
        &format!("task {key} to be {status}"),
        RUN_TIMEOUT,
        || async { Ok((db::task_status(pool, key).await? == status).then_some(())) },
    )
    .await
}

/// The pull request whose head is `branch`.
async fn pull_of(forgejo: &Forgejo, branch: &str) -> Result<Option<Value>> {
    Ok(forgejo
        .pulls()
        .await?
        .into_iter()
        .find(|pr| pr.pointer("/head/ref").and_then(Value::as_str) == Some(branch)))
}
