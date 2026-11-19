//! CI reads (`iter3-ci-tools`, `just e2e-local`): a task lists the open pull requests of its
//! project, reads the failing check of its own branch and asks for a job log. Works in the
//! repository `factory/existing-branch` of project `EB`; the branch carries the unique suffix of
//! the run, the check is posted on its head commit and the task finds it by that commit, so
//! concurrent runs don't see each other's. The test touches only its own branch and pull request.
//! Needs the environment of `E2eEnv`, so the test is ignored by default.
//!
//! The e2e Forgejo is v15 and has no Actions runner: a `ci-failed` event can't wake the task and
//! no job log can be read. The failing check is a commit status the test posts, which has no job,
//! and the log call is an error result that says why (spec `ci-reads`, "Old Forgejo").

use anyhow::{Context, Result};
use df_e2e::fixture;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{data, db};

/// The context of the status the test posts: the check the agent must find.
const CHECK: &str = "e2e / ci-reads (push)";

/// Checks of spec `ci-reads` as far as one pull request and a Forgejo v15 allow: `read_ci_status`
/// of the branch's head commit (a ref, not a pull request number) returns the failing check,
/// `list_pull_requests` by head prefix lists the pull request (not a survey of several), and
/// `read_ci_log` fails saying logs need Forgejo v16 ("Old Forgejo").
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_task_reads_the_failing_check_of_its_own_branch_and_is_told_logs_need_forgejo_16()
-> Result<()> {
    fixture::run(
        "a_task_reads_the_failing_check_of_its_own_branch_and_is_told_logs_need_forgejo_16",
        |world| async move {
            let bot = world.forgejo_for(data::FORGE_EXISTING_BRANCH_REPO)?;
            let branch = format!("{}{}", data::CI_READS_BRANCH_PREFIX, world.suffix);
            bot.create_branch(&branch, data::BASE_BRANCH).await?;
            bot.create_file(
                &branch,
                &format!("{}.txt", world.unique("ci")),
                "bot change\n",
                "Bot change",
            )
            .await?;
            let number = bot
                .open_pull(&branch, data::BASE_BRANCH, "Bump for CI", false)
                .await?;
            let pull = bot
                .pull_of(data::BASE_BRANCH, &branch)
                .await?
                .context("the pull request of the bot branch")?;
            let sha = pull
                .pointer("/head/sha")
                .and_then(|sha| sha.as_str())
                .context("the head commit of the pull request")?;
            bot.create_commit_status(sha, "failure", CHECK, "http://forge.test/ci/1")
                .await?;

            let key = world
                .session
                .create_task_on_branch(
                    &world.env,
                    data::EXISTING_BRANCH_PROJECT_KEY,
                    &format!("{} Look at the CI.", data::EXISTING_BRANCH_CI_MARKER),
                    &branch,
                )
                .await?;
            // The script ends with this line, after its calls.
            let reached = eventually("the agent to read the CI state", RUN_TIMEOUT, || async {
                let said =
                    db::transcript_events_containing(&world.pool, &key, "Read the CI state.")
                        .await?;
                Ok((said > 0).then_some(()))
            })
            .await;
            // Leave no open pull request behind.
            bot.close_pull(number).await?;
            reached?;

            let (pool, key) = (&world.pool, &key);
            // The task works on this branch, so other events could name it: require it in the
            // event of the `list_pull_requests` result, which carries the key `pull_requests`.
            assert!(
                db::transcript_events_containing_both(pool, key, &branch, "pull_requests").await?
                    > 0,
                "the list_pull_requests result names the branch {branch}"
            );
            assert!(
                db::transcript_events_containing_both(pool, key, CHECK, "failure").await? > 0,
                "the check {CHECK} of pull request #{number} is reported as failure"
            );
            assert!(
                db::transcript_events_containing(pool, key, "job logs need Forgejo v16 or later")
                    .await?
                    > 0,
                "the log call says why it has no log"
            );
            Ok(())
        },
    )
    .await
}
