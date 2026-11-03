//! The iteration 1 flow through the web UI (`just e2e-local`): assignment, pull request, tools,
//! follow-up, completion and the rate limit; and demo steps 2 to 5 of iteration 2 (push checks,
//! network access request, stop and cancel). Needs the environment of `E2eEnv`, so the tests
//! are ignored by default.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use df_db::push_checks::PushCheck;
use df_e2e::browser::Session;
use df_e2e::forgejo::Forgejo;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use serde_json::Value;
use sqlx::PgPool;

/// Wait limit for the approval to be stored.
const RETRY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the retry may take after the approval, until the UI shows it.
const RETRY_BOUND: Duration = Duration::from_secs(10);
/// What the first turn of the `network-access` script says before it fetches.
const NEED: &str = "I need the package index from pypi.org.";
/// What the fake agent says once the fetch of the first turn got through.
const FETCHED: &str = "Fetched https://pypi.org/";
/// What the second turn of the `network-access` script (the decision message) says.
const NOTED: &str = "Noted the decision.";

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
    let number = pulls[0]["number"].as_u64().context("number")?;
    // The review thread `read_pull_request_feedback` of run 2 returns first.
    let review = world
        .forgejo
        .submit_line_comment(number, "greeting.txt", 1, "Please add a test.")
        .await?;

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
    // `comment_on_pull_request` of run 2 replied in the thread it read.
    let thread = world.forgejo.review_comments(number, review).await?;
    assert!(
        thread
            .iter()
            .any(|c| c["body"] == "Added greeting_test.txt." && c["path"] == "greeting.txt"),
        "{thread:?}"
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

/// A rate-limited task resumes at the retry time reported by the run (60 s in the script) and the
/// next run starts without a message.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn rate_limited_task_resumes_at_the_retry_time_without_a_message() -> Result<()> {
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
    world
        .session
        .expect_region_text("#task-details", "Resumes at", RUN_TIMEOUT)
        .await?;

    // No message: the scheduler starts run 2 once the retry time has passed.
    world
        .session
        .expect_text("Continuing after the rate limit.", RUN_TIMEOUT)
        .await?;
    world.wait_for_status(&key, "waiting").await?;
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 2, "{runs:?}");
    Ok(())
}

/// Demo step 4 of iteration 2: the proxy denies `pypi.org`, the control plane raises a
/// `network-access` request from the gateway's report, the board member approves it for the
/// task and the agent's retry in the same turn gets through.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn denied_host_is_requested_and_the_approval_lets_the_retry_through() -> Result<()> {
    let (host, port) = (data::NETWORK_HOST, data::NETWORK_PORT);
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::NETWORK_PROJECT_KEY,
            "Fetch the package index",
            None,
        )
        .await?;
    world.session.expect_text(NEED, RUN_TIMEOUT).await?;

    let request = eventually("a network-access request", RUN_TIMEOUT, || async {
        db::network_request(&world.pool, &key, host, port).await
    })
    .await?;
    assert_eq!(request.state, "pending", "{request:?}");
    assert!(request.raised_by_control_plane, "{request:?}");
    assert!(!request.blocking, "{request:?}");
    assert!(
        request.reason.contains(&format!("{host}:{port}")),
        "{request:?}"
    );
    assert_eq!(
        db::http_host_scopes(&world.pool, &key, host, port).await?,
        Vec::<String>::new()
    );
    // The fetch of run 1 is still retrying. The run row stays `preparing` during the first turn,
    // so the task status tells that it runs.
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].outcome, None, "{runs:?}");
    assert_eq!(db::task_status(&world.pool, &key).await?, "running");

    world
        .session
        .approve_request(
            &world.env,
            &request.id.to_string(),
            &format!("network access to {host}:{port}"),
            Some("task"),
        )
        .await?;
    let approved_at = Instant::now();

    let decided = eventually("the approval", RETRY_TIMEOUT, || async {
        let request = db::network_request(&world.pool, &key, host, port).await?;
        Ok(request.filter(|r| r.state != "pending"))
    })
    .await?;
    assert_eq!(decided.id, request.id);
    assert_eq!(decided.state, "approved");
    assert_eq!(decided.scope.as_deref(), Some("task"));
    assert_eq!(
        db::http_host_scopes(&world.pool, &key, host, port).await?,
        ["task"]
    );
    world.session.open_task(&world.env, &key).await?;
    // Bound from demo step 4 ("about a second"), measured from the approval, UI polling included.
    // Playwright reads a zero timeout as no limit.
    let left = RETRY_BOUND
        .saturating_sub(approved_at.elapsed())
        .max(Duration::from_millis(1));
    world
        .session
        .expect_region_text("#transcript", FETCHED, left)
        .await?;
    // The decision message is the next prompt of the same session in run 1.
    world
        .session
        .expect_region_text("#transcript", NOTED, RUN_TIMEOUT)
        .await?;

    // Run 1 fetched: a restart (a new run, or a new session after a failed resume) would replay
    // the first turn of the script, so the transcript would say it needs the index twice.
    let transcript = world.session.region_text("#transcript").await?;
    assert_eq!(transcript.matches(NEED).count(), 1, "{transcript}");
    assert_eq!(transcript.matches(FETCHED).count(), 1, "{transcript}");
    assert_eq!(transcript.matches(NOTED).count(), 1, "{transcript}");
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 1, "{runs:?}");
    Ok(())
}

/// Demo step 5 of iteration 2, first half: the board member stops the run during its first turn,
/// the task waits and its pending request stays; cancelling the task then closes the request.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn stopping_a_run_makes_the_task_wait_and_cancelling_it_closes_its_requests() -> Result<()> {
    let (host, port) = (data::NETWORK_HOST, data::NETWORK_PORT);
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::NETWORK_PROJECT_KEY,
            "Fetch the package index",
            None,
        )
        .await?;
    world.session.expect_text(NEED, RUN_TIMEOUT).await?;
    let request = eventually("a network-access request", RUN_TIMEOUT, || async {
        db::network_request(&world.pool, &key, host, port).await
    })
    .await?;
    assert_eq!(request.state, "pending", "{request:?}");

    world.session.stop_run().await?;

    // The runtime finalizes and reports `cancelled`: the task waits, the request is untouched.
    world.wait_for_status(&key, "waiting").await?;
    let runs = db::runs(&world.pool, &key).await?;
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].state, "cancelled", "{runs:?}");
    assert_eq!(runs[0].outcome.as_deref(), Some("cancelled"), "{runs:?}");
    world
        .session
        .expect_region_text("#task-details", "waiting", RUN_TIMEOUT)
        .await?;
    let after_stop = db::network_request(&world.pool, &key, host, port)
        .await?
        .context("the request still exists")?;
    assert_eq!(after_stop.state, "pending", "{after_stop:?}");

    world.session.cancel_task().await?;

    world.wait_for_status(&key, "cancelled").await?;
    world
        .session
        .expect_region_text("#task-details", "cancelled", RUN_TIMEOUT)
        .await?;
    let closed = db::network_request(&world.pool, &key, host, port)
        .await?
        .context("the request still exists")?;
    assert_eq!(closed.state, "closed", "{closed:?}");
    assert_eq!(closed.close_reason.as_deref(), Some("task-cancelled"));
    Ok(())
}

/// Demo step 5 of iteration 2, second half: cancelling a task with a draft PR leaves the PR
/// as it is.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn cancelling_a_task_leaves_its_pull_request_untouched() -> Result<()> {
    let world = World::start().await?;
    let key = world
        .session
        .create_task(&world.env, data::PROJECT_KEY, "Add a greeting file", None)
        .await?;
    world.wait_for_status(&key, "waiting").await?;
    let before = world.pulls_of(&branch_of(&key)).await?;
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!(before[0]["draft"], true, "{before:?}");

    world.session.cancel_task().await?;

    world.wait_for_status(&key, "cancelled").await?;
    let after = world.pulls_of(&branch_of(&key)).await?;
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0]["draft"], true, "{after:?}");
    assert_eq!(after[0]["state"], "open", "{after:?}");
    Ok(())
}

/// Demo step 2 of iteration 2: the Git proxy rejects the agent's commit with a token, raises a
/// `push-check-exception` request and tells the agent (`push.rejected`). The agent amends its
/// unpushed commit, the next push passes and closes the request as obsolete.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn committed_secret_is_rejected_and_the_amended_commit_closes_the_request() -> Result<()> {
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::SECRET_PROJECT_KEY,
            "Add the deploy configuration",
            None,
        )
        .await?;
    let token = data::test_token();

    let request = eventually("a push-check-exception request", RUN_TIMEOUT, || async {
        Ok(db::exception_requests(&world.pool, &key)
            .await?
            .into_iter()
            .next())
    })
    .await?;
    let [finding] = request.findings.as_slice() else {
        panic!("one finding: {request:?}");
    };
    assert_eq!(finding.check, PushCheck::SecretScan, "{finding:?}");
    assert_eq!(finding.path, data::SECRET_PATH);
    assert_eq!(finding.line, Some(data::SECRET_LINE));
    assert_eq!(finding.rule.as_deref(), Some("betterleaks.github-pat"));
    assert_eq!(finding.preview, "ghp_********");

    let closed = eventually("the request to close", RUN_TIMEOUT, || async {
        let requests = db::exception_requests(&world.pool, &key).await?;
        Ok(requests
            .into_iter()
            .find(|r| r.id == request.id && r.state != "pending"))
    })
    .await?;
    assert_eq!(closed.state, "closed", "{closed:?}");
    assert_eq!(closed.close_reason.as_deref(), Some("push-accepted"));
    let requests = db::exception_requests(&world.pool, &key).await?;
    assert!(
        requests.iter().all(|r| r.state != "pending"),
        "{requests:?}"
    );

    // The rejection reached the push record and the agent, masked.
    let pushes = db::pushes(&world.pool, &key).await?;
    let rejected = pushes
        .iter()
        .flat_map(|p| &p.refs)
        .find(|r| r.reason.starts_with("df:secret-scan"))
        .with_context(|| format!("a df:secret-scan rejection: {pushes:?}"))?;
    let location = format!("{}:{}", data::SECRET_PATH, data::SECRET_LINE);
    assert!(rejected.reason.contains(&location), "{rejected:?}");
    assert!(pushes.iter().any(|p| p.verdict == "accepted"), "{pushes:?}");
    let messages = db::push_rejected_messages(&world.pool, &key).await?;
    assert!(!messages.is_empty());
    assert!(
        messages
            .iter()
            .all(|m| m.contains("df:secret-scan") && m.contains("ghp_********")),
        "{messages:?}"
    );
    assert!(
        pushes.iter().all(|p| !format!("{p:?}").contains(&token)),
        "a push record holds the token"
    );
    assert!(
        messages.iter().all(|m| !m.contains(&token)),
        "a message holds the token"
    );

    // The forge has the amended commit only: no token on the branch.
    let branch = branch_of(&key);
    let config = world.forgejo.file(&branch, data::SECRET_PATH).await?;
    assert!(!config.contains(&token), "{config}");
    assert!(config.contains("DEPLOY_TOKEN"), "{config}");
    let commits = world.forgejo.commits(&branch).await?;
    let added = commits
        .iter()
        .filter(|c| {
            c["commit"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("Add deploy configuration"))
        })
        .count();
    assert_eq!(added, 1, "{commits:?}");
    Ok(())
}

/// Demo step 3 of iteration 2: the agent edits the CI workflow, a protected path in mode
/// `approval`. Every push of the commit is rejected until the board member approves the
/// `push-check-exception` request; after the next turn the same commit passes.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn protected_path_passes_after_the_exception_is_approved() -> Result<()> {
    let world = World::start().await?;
    let key = world
        .session
        .create_task(
            &world.env,
            data::PROTECTED_PROJECT_KEY,
            "Run the tests in CI",
            None,
        )
        .await?;
    let branch = branch_of(&key);

    let request = eventually("a push-check-exception request", RUN_TIMEOUT, || async {
        Ok(db::exception_requests(&world.pool, &key)
            .await?
            .into_iter()
            .next())
    })
    .await?;
    assert_eq!(request.state, "pending", "{request:?}");
    let [finding] = request.findings.as_slice() else {
        panic!("one finding: {request:?}");
    };
    assert_eq!(finding.check, PushCheck::ProtectedPath, "{finding:?}");
    assert_eq!(finding.path, data::PROTECTED_PATH);
    let fingerprint = format!("path:{}", data::PROTECTED_PATH);
    assert_eq!(finding.fingerprint, fingerprint);
    let pushes = db::pushes(&world.pool, &key).await?;
    assert!(pushes.iter().all(|p| p.verdict == "rejected"), "{pushes:?}");
    assert!(
        pushes
            .iter()
            .flat_map(|p| &p.refs)
            .all(|r| r.reason.starts_with("df:protected-path")),
        "{pushes:?}"
    );

    world
        .session
        .approve_request(
            &world.env,
            &request.id.to_string(),
            data::PROTECTED_PATH,
            None,
        )
        .await?;

    let decided = eventually("the approval", RETRY_TIMEOUT, || async {
        let requests = db::exception_requests(&world.pool, &key).await?;
        Ok(requests
            .into_iter()
            .find(|r| r.id == request.id && r.state != "pending"))
    })
    .await?;
    assert_eq!(decided.state, "approved", "{decided:?}");
    assert_eq!(db::push_exceptions(&world.pool, &key).await?, [fingerprint]);

    // The decision message starts the next turn; the runtime pushes the same commit again.
    eventually("an accepted push", RUN_TIMEOUT, || async {
        let pushes = db::pushes(&world.pool, &key).await?;
        Ok(pushes.iter().any(|p| p.verdict == "accepted").then_some(()))
    })
    .await?;
    let commits = world.forgejo.commits(&branch).await?;
    assert!(
        commits.iter().any(|c| c["commit"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("Run the tests in CI"))),
        "{commits:?}"
    );
    let workflow = world.forgejo.file(&branch, data::PROTECTED_PATH).await?;
    assert!(workflow.contains("echo test"), "{workflow}");
    let requests = db::exception_requests(&world.pool, &key).await?;
    assert!(
        requests.iter().all(|r| r.state != "pending"),
        "{requests:?}"
    );
    Ok(())
}
