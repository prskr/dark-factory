//! Pull request lifecycle against the real GitHub. Ignored by default; run with
//! `cargo test -p df-plugins --test github_live -- --ignored` and these variables:
//!
//! - `DF_TEST_GITHUB_REPO_URL` (required): `https://github.com/<owner>/<repo>.git` of a scratch repository
//! - `DF_TEST_GITHUB_TOKEN` (required): token with Pull requests, Issues and Contents read/write and
//!   Metadata read on that repository; never printed
//! - `DF_TEST_GITHUB_HEAD` (required): an existing branch with commits ahead of the base that has no
//!   open pull request into it
//! - `DF_TEST_GITHUB_BASE`: target branch (default `main`)
//! - `DF_TEST_GITHUB_ISSUE` (required): number of an open issue in that repository to comment on
//! - `DF_TEST_GITHUB_DRAFT_MODE`: plugin setting `draft-mode`, `native` (default), `title-prefix` or `auto`
//!
//! The assertions are forge-neutral and cannot tell a native draft from a title prefix. Check the
//! form on the pull request timeline, e.g. `gh api repos/<owner>/<repo>/issues/<number>/timeline
//! --jq '.[].event'`: `title-prefix` shows `renamed` events for the `WIP:` prefix, `native` and
//! `auto` (where GitHub accepts native drafts) a `ready_for_review` event.
//!
//! The pull request stays open afterwards (the plugin cannot close it); close it before the next run.

mod common;

use common::{var, var_or};
use df_core::{ForgeClient, NewPullRequest, ProjectId, PullRequestUpdate, RepoRef};

const TITLE: &str = "Dark Factory plugin test";
const RETITLED: &str = "Dark Factory plugin test (retitled)";

#[tokio::test]
#[ignore = "needs github.com, see the module docs"]
async fn draft_is_created_found_retitled_readied_and_commented() -> anyhow::Result<()> {
    let draft_mode = var_or("DF_TEST_GITHUB_DRAFT_MODE", "native");
    let (plugin, config) = common::github_client(
        &var("DF_TEST_GITHUB_TOKEN")?,
        "github.com",
        "https://api.github.com",
        &[("draft-mode", draft_mode.as_str())],
    )?;
    let client =
        common::named_https_router("github", plugin, config, &var("DF_TEST_GITHUB_REPO_URL")?)?;
    let project = ProjectId::new();
    let head = var("DF_TEST_GITHUB_HEAD")?;
    let base = var_or("DF_TEST_GITHUB_BASE", "main");
    let issue: u64 = var("DF_TEST_GITHUB_ISSUE")?.parse()?;

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
                title: TITLE.into(),
                body: "Created by the df-plugins live GitHub test.".into(),
                draft: true,
            },
        )
        .await?;
    assert!(created.draft);
    assert_eq!(created.title, TITLE);

    let found = client
        .find_open_pull_request(&project, &head, &base)
        .await?
        .expect("the pull request is found");
    assert_eq!(found.id.number, created.id.number);
    assert!(found.draft);

    let retitled = client
        .update_pull_request(
            &project,
            created.id.number,
            PullRequestUpdate {
                title: Some(RETITLED.into()),
                ..Default::default()
            },
        )
        .await?;
    assert!(retitled.draft);
    assert_eq!(retitled.title, RETITLED);

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
    assert_eq!(ready.title, RETITLED);

    client
        .add_issue_comment(&project, created.id.number, "Comment on the pull request")
        .await?;
    client
        .add_issue_comment(&project, issue, "Comment on the issue")
        .await?;
    Ok(())
}
