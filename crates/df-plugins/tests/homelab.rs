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

mod common;

use std::time::Duration;

use df_core::{ForgeClient, NewPullRequest, ProjectId, PullRequestUpdate, RepoRef};

fn var(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_not_set| anyhow::anyhow!("{name} is not set"))
}

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
    let base = std::env::var("DF_TEST_FORGEJO_BASE").unwrap_or_else(|_| "main".into());
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
