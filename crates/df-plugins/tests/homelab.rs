//! Pull request lifecycle against a real Forgejo. Ignored by default; run with
//! `cargo test -p df-plugins --test homelab -- --ignored` and these variables:
//!
//! - `DF_TEST_FORGEJO_REPO_URL`: clone URL of a scratch repository, e.g. `https://git.example.org/bot/scratch.git`
//! - `DF_TEST_FORGEJO_TOKEN`: token of the bot account (scopes: see `crates/df-plugins/README.md`)
//! - `DF_TEST_FORGEJO_HEAD`: an existing branch with commits ahead of the base that has no open pull request
//! - `DF_TEST_FORGEJO_BASE`: target branch (default `main`)
//! - `DF_TEST_FORGEJO_ISSUE`: number of an existing issue to comment on
//!
//! The pull request stays open afterwards (the plugin cannot close it); close it before the next run.
//!
//! `review_reply_joins_the_thread` (`-- --ignored review_reply`) needs the repository and token
//! variables and these:
//!
//! - `DF_TEST_FORGEJO_REVIEW_PR`: a pull request where another user submitted a review with a line
//!   comment
//! - `DF_TEST_FORGEJO_OTHER_PR` (optional): another pull request of the repository; replying there
//!   with a thread of the first one must fail
//!
//! Each run adds one reply.

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{var, var_or};
use df_core::{
    AuthorRole, ForgeClient, IntegrationError, NewPullRequest, ProjectId, PullRequestUpdate,
    RepoRef,
};

#[tokio::test]
#[ignore = "needs a Forgejo instance, see the module docs"]
async fn draft_pull_request_lifecycle() -> anyhow::Result<()> {
    let client = common::client_with_token(
        &common::forgejo(),
        &var("DF_TEST_FORGEJO_REPO_URL")?,
        &var("DF_TEST_FORGEJO_TOKEN")?,
        Duration::from_secs(30),
        64 * 1024 * 1024,
    )?;
    let project = ProjectId::new();
    let head = var("DF_TEST_FORGEJO_HEAD")?;
    let base = var_or("DF_TEST_FORGEJO_BASE", "main");
    let issue: u64 = var("DF_TEST_FORGEJO_ISSUE")?.parse()?;

    let created = client
        .create_pull_request(
            &project,
            NewPullRequest {
                // The repository comes from the project.
                repo: RepoRef {
                    owner: String::new(),
                    name: String::new(),
                },
                head: head.clone(),
                base: base.clone(),
                title: "Dark Factory plugin test".into(),
                body: "Created by the df-plugins integration test.".into(),
                draft: true,
            },
        )
        .await?;
    assert!(created.draft);
    assert_eq!(created.title, "Dark Factory plugin test");

    let found = client
        .find_open_pull_request(&project, &head, &base)
        .await?
        .expect("the pull request is found");
    assert_eq!(found.id.number, created.id.number);

    let retitled = client
        .update_pull_request(
            &project,
            created.id.number,
            PullRequestUpdate {
                title: Some("Dark Factory plugin test (retitled)".into()),
                ..Default::default()
            },
        )
        .await?;
    assert!(retitled.draft);

    let ready = client
        .update_pull_request(
            &project,
            created.id.number,
            PullRequestUpdate {
                draft: Some(false),
                ..Default::default()
            },
        )
        .await?;
    assert!(!ready.draft);
    assert_eq!(ready.title, "Dark Factory plugin test (retitled)");

    client
        .add_issue_comment(&project, created.id.number, "Comment on the pull request")
        .await?;
    client
        .add_issue_comment(&project, issue, "Comment on the issue")
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs a Forgejo instance, see the module docs"]
async fn review_reply_joins_the_thread() -> anyhow::Result<()> {
    let client = common::client_with_token(
        &common::forgejo(),
        &var("DF_TEST_FORGEJO_REPO_URL")?,
        &var("DF_TEST_FORGEJO_TOKEN")?,
        Duration::from_secs(30),
        64 * 1024 * 1024,
    )?;
    let project = ProjectId::new();
    let pr: u64 = var("DF_TEST_FORGEJO_REVIEW_PR")?.parse()?;

    let before = client
        .list_pull_request_feedback(&project, pr, None)
        .await?;
    let thread = before
        .threads
        .iter()
        .find(|t| {
            t.comments
                .first()
                .is_some_and(|c| c.author.role != AuthorRole::Bot)
        })
        .expect("a review thread started by another user");
    let body = format!(
        "Reply from the df-plugins live test ({}s)",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
    );
    let posted = client
        .reply_to_review_thread(&project, pr, &thread.id, &body)
        .await?;

    let after = client
        .list_pull_request_feedback(&project, pr, None)
        .await?;
    let same = after
        .threads
        .iter()
        .find(|t| t.id == thread.id)
        .expect("the thread keeps its id");
    assert_eq!(same.comments.len(), thread.comments.len() + 1);
    let last = same.comments.last().expect("the reply");
    assert_eq!(
        (last.id.as_str(), last.body.as_str()),
        (posted.id.as_str(), body.as_str())
    );
    assert_eq!(last.author.role, AuthorRole::Bot);
    assert_eq!(after.threads.len(), before.threads.len());

    if let Ok(other) = var("DF_TEST_FORGEJO_OTHER_PR") {
        let err = client
            .reply_to_review_thread(&project, other.parse()?, &thread.id, "misplaced")
            .await
            .expect_err("a thread of another pull request");
        assert!(matches!(err, IntegrationError::InvalidInput(_)), "{err:?}");
    }
    Ok(())
}
