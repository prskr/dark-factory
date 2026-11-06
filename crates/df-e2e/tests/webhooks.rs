//! Forge webhooks (`iter3-webhooks`, `just e2e-local`): the e2e Forgejo delivers the events of
//! `factory/demo` to the control plane, and a review by a member wakes a task in review. Needs the
//! environment of `E2eEnv`, so the test is ignored by default.

use std::time::Duration;

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::forgejo::Forgejo;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use serde_json::Value;
use sqlx::PgPool;

/// From the review to the message on the task page: delivery, the message and the page update.
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn member_review_wakes_the_task_in_review_and_its_pr_is_a_draft_again() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;
    let bot = Forgejo::new(&env.forgejo_url, &env.forgejo_token)?;
    let reviewer = Forgejo::new(&env.forgejo_url, &env.forgejo_reviewer_token)?;
    let key = session
        .create_task(&env, data::WEBHOOK_PROJECT_KEY, "Add a welcome file", None)
        .await?;
    let branch = format!("factory/task/{key}");

    // Run 1 completes the task: in review with a ready pull request.
    wait_for_status(&pool, &key, "in_review").await?;
    let number = eventually("a ready pull request", RUN_TIMEOUT, || async {
        Ok(pull_of(&bot, &branch)
            .await?
            .filter(|pr| pr["draft"] == false)
            .and_then(|pr| pr["number"].as_u64()))
    })
    .await?;

    reviewer
        .submit_review(number, "REQUEST_CHANGES", "Please greet in German as well.")
        .await?;

    let line = format!(
        "{} submitted a review on PR #{number}: changes requested.",
        data::FORGE_REVIEWER
    );
    session.expect_text(&line, WEBHOOK_TIMEOUT).await?;
    let messages = db::system_messages(&pool, &key, df_core::system_reason::WEBHOOK).await?;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains(&line), "{messages:?}");
    // Run 2 of the script reads the feedback and waits; the reopened task's pull request stays a
    // draft.
    eventually(
        "the pull request to be a draft again",
        RUN_TIMEOUT,
        || async {
            Ok(pull_of(&bot, &branch)
                .await?
                .and_then(|pr| (pr["draft"] == true).then_some(())))
        },
    )
    .await?;
    wait_for_status(&pool, &key, "waiting").await?;
    session
        .expect_text("Read the review; waiting for more.", RUN_TIMEOUT)
        .await?;
    let pull = pull_of(&bot, &branch)
        .await?
        .context("the pull request of the task")?;
    assert_eq!(pull["draft"], true, "{:?}", pull["title"]);
    assert_eq!(pull["state"], "open");
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
