//! The iteration 1 flow through the web UI (`just e2e-local`): assignment, pull request, tools,
//! follow-up, completion and the rate limit. Needs the environment of `E2eEnv`, so the tests
//! are ignored by default.

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::forgejo::Forgejo;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use serde_json::Value;
use sqlx::PgPool;

struct World {
    env: E2eEnv,
    session: Session,
    forgejo: Forgejo,
    pool: PgPool,
}

impl World {
    async fn start() -> Result<Self> {
        let env = E2eEnv::from_env()?;
        let session = Session::start(&env).await?;
        session.login(&env).await?;
        Ok(Self {
            forgejo: Forgejo::new(&env.forgejo_url, &env.forgejo_token)?,
            pool: db::connect(&env.database_url).await?,
            env,
            session,
        })
    }

    /// Waits until task `key` has `status`.
    async fn wait_for_status(&self, key: &str, status: &str) -> Result<()> {
        eventually(
            &format!("task {key} to be {status}"),
            RUN_TIMEOUT,
            || async { Ok((db::task_status(&self.pool, key).await? == status).then_some(())) },
        )
        .await
    }

    /// The pull requests of the repository whose head is `branch`.
    async fn pulls_of(&self, branch: &str) -> Result<Vec<Value>> {
        Ok(self
            .forgejo
            .pulls()
            .await?
            .into_iter()
            .filter(|pr| pr.pointer("/head/ref").and_then(Value::as_str) == Some(branch))
            .collect())
    }
}

fn branch_of(key: &str) -> String {
    format!("factory/task/{key}")
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn assigned_task_gets_a_draft_pr_with_the_shaped_title_and_an_issue_comment() -> Result<()> {
    let world = World::start().await?;

    let key = world
        .session
        .create_task(
            &world.env,
            data::PROJECT_KEY,
            "Add a greeting file",
            Some(data::FORGE_ISSUE_NUMBER),
        )
        .await?;

    world
        .session
        .expect_text("Pushed the greeting. Tell me what to change.", RUN_TIMEOUT)
        .await?;
    world.wait_for_status(&key, "waiting").await?;

    let branch = branch_of(&key);
    assert!(world.forgejo.branch_exists(&branch).await?, "{branch}");
    let pulls = world.pulls_of(&branch).await?;
    assert_eq!(pulls.len(), 1, "{pulls:?}");
    assert_eq!(pulls[0]["base"]["ref"], data::BASE_BRANCH);
    assert_eq!(pulls[0]["draft"], true, "{:?}", pulls[0]["title"]);
    // `update_pull_request` of the script; a draft carries the `WIP: ` prefix of the plugin.
    assert_eq!(pulls[0]["title"], "WIP: Add a greeting");
    let (number, _url) = db::task_pull_request(&world.pool, &key)
        .await?
        .context("the task links its pull request")?;
    assert_eq!(number, pulls[0]["number"].as_i64().context("number")?);
    // The task page links the pull request.
    world
        .session
        .expect_region_text("#task-details", &format!("PR #{number}"), RUN_TIMEOUT)
        .await?;

    // `comment_on_issue` of the script.
    let comments = world.forgejo.comments(data::FORGE_ISSUE_NUMBER).await?;
    assert!(
        comments
            .iter()
            .any(|c| c["body"] == "First pass pushed; waiting for feedback."),
        "{comments:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn follow_up_resumes_the_session_and_completing_marks_the_pr_ready() -> Result<()> {
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::PROJECT_KEY,
            "Add a greeting file",
            Some(data::FORGE_ISSUE_NUMBER),
        )
        .await?;
    world.wait_for_status(&key, "waiting").await?;
    let first_run = db::runs(&world.pool, &key).await?;
    assert_eq!(first_run.len(), 1, "{first_run:?}");
    let pulls = world.pulls_of(&branch_of(&key)).await?;
    assert_eq!(pulls.len(), 1, "{pulls:?}");

    world
        .session
        .send_message("also add a test for the greeting")
        .await?;

    // `#transcript` holds delivered messages only; the queued row is in `#pending`.
    world
        .session
        .expect_region_text(
            "#transcript",
            "also add a test for the greeting",
            RUN_TIMEOUT,
        )
        .await?;

    // `complete_task` ends run 2 as `completed` and puts the task in review.
    world.wait_for_status(&key, "in_review").await?;
    world
        .session
        .expect_text("Adding the requested test.", RUN_TIMEOUT)
        .await?;
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[1].outcome.as_deref(), Some("completed"));
    assert_eq!(
        runs[1].harness_session_id, first_run[0].harness_session_id,
        "run 2 resumes the session of run 1"
    );
    // The same pull request, now ready, with the commit of run 2.
    let pulls = world.pulls_of(&branch_of(&key)).await?;
    assert_eq!(pulls.len(), 1, "no second PR: {pulls:?}");
    assert_eq!(pulls[0]["draft"], false);
    assert_eq!(pulls[0]["title"], "Add a greeting");
    let commits = world.forgejo.commits(&branch_of(&key)).await?;
    assert!(
        commits.iter().any(|c| c["commit"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("Add greeting test"))),
        "{commits:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn rate_limited_run_shows_the_limit_and_the_next_message_continues() -> Result<()> {
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::RATE_LIMIT_PROJECT_KEY,
            "Do some work",
            None,
        )
        .await?;

    world.wait_for_status(&key, "rate_limited").await?;
    // The status of the task.
    world
        .session
        .expect_region_text("#task-details", "rate limited", RUN_TIMEOUT)
        .await?;
    // The run card shows the end reason and the time to retry at.
    world.session.open_runs().await?;
    world
        .session
        .expect_region_text("#runs", "rate limited", RUN_TIMEOUT)
        .await?;
    world
        .session
        .expect_region_text("#runs", "retry at", RUN_TIMEOUT)
        .await?;
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs[0].outcome.as_deref(), Some("rate_limited"), "{runs:?}");
    assert!(runs[0].has_retry_after, "{runs:?}");

    world.session.send_message("please continue").await?;

    world
        .session
        .expect_text("Continuing after the rate limit.", RUN_TIMEOUT)
        .await?;
    world.wait_for_status(&key, "waiting").await?;
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 2, "{runs:?}");
    Ok(())
}
