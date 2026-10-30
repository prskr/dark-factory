//! The GitHub component, run by the host against a mock GitHub (spec `github-integration`).

mod common;
mod github_openapi;

use std::time::{SystemTime, UNIX_EPOCH};

use df_core::{
    ForgeClient, IntegrationError, IssueState, NewPullRequest, ProjectId, PullRequestUpdate,
    RateLimit,
};
use df_plugins::ForgeRouter;
use serde_json::json;
use wiremock::matchers::{
    body_json, body_partial_json, body_string_contains, header, header_regex, method, path,
    query_param,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PULLS: &str = "/repos/o/r/pulls";
const NODE_ID: &str = "PR_kwDO7";

/// The plugin `github` serving `github.example`, whose API is the mock `server`.
fn client(server: &MockServer, settings: &[(&str, &str)]) -> anyhow::Result<ForgeRouter> {
    let (plugin, config) =
        common::github_client(common::TOKEN, "github.example", &server.uri(), settings)?;
    common::named_router("github", plugin, config, "https://github.example/o/r.git")
}

fn pr(title: &str, draft: bool) -> serde_json::Value {
    json!({
        "number": 7, "node_id": NODE_ID, "html_url": "http://gh/o/r/pull/7", "title": title,
        "body": "body", "state": "open", "draft": draft, "merged_at": null,
        "head": {"ref": "factory/task/DF-1", "sha": "abc123"}, "base": {"ref": "main", "sha": "def"}
    })
}

fn new_draft() -> NewPullRequest {
    NewPullRequest {
        repo: df_core::RepoRef {
            owner: String::new(),
            name: String::new(),
        },
        head: "factory/task/DF-1".into(),
        base: "main".into(),
        title: "DF-1: Fix login".into(),
        body: "body".into(),
        draft: true,
    }
}

fn ready() -> PullRequestUpdate {
    PullRequestUpdate {
        draft: Some(false),
        ..Default::default()
    }
}

/// A mocked response of the REST operation `operation_id`, checked against GitHub's description.
fn github_response(
    operation_id: &str,
    status: u16,
    body: serde_json::Value,
) -> anyhow::Result<ResponseTemplate> {
    github_openapi::validate_response(operation_id, status, &body)?;
    Ok(ResponseTemplate::new(status).set_body_json(body))
}

/// Every REST request the mock received matches an operation of GitHub's description.
async fn assert_requests_match_the_extract(server: &MockServer) {
    let errors: Vec<String> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() != "/graphql")
        .filter_map(|request| github_openapi::validate_received(request).err())
        .map(|err| format!("{err:#}"))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

async fn mount_comment(server: &MockServer) -> anyhow::Result<()> {
    Mock::given(method("POST"))
        .and(path("/repos/o/r/issues/7/comments"))
        .and(body_json(json!({"body": "hello"})))
        .respond_with(github_response(
            "issues/create-comment",
            201,
            json!({"id": 55, "html_url": "http://c/55"}),
        )?)
        .expect(1)
        .mount(server)
        .await;
    Ok(())
}

#[tokio::test]
async fn title_prefix_lifecycle_marks_drafts_in_the_title() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PULLS))
        .and(body_json(json!({
            "title": "WIP: DF-1: Fix login", "body": "body",
            "head": "factory/task/DF-1", "base": "main"
        })))
        .respond_with(github_response(
            "pulls/create",
            201,
            pr("WIP: DF-1: Fix login", false),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(PULLS))
        .and(query_param("state", "open"))
        .and(query_param("head", "o:factory/task/DF-1"))
        .and(query_param("base", "main"))
        .and(query_param("per_page", "100"))
        .respond_with(github_response(
            "pulls/list",
            200,
            json!([pr("WIP: DF-1: Fix login", false)]),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{PULLS}/7")))
        .respond_with(github_response(
            "pulls/get",
            200,
            pr("WIP: DF-1: Fix login", false),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{PULLS}/7")))
        .and(body_json(json!({"title": "DF-1: Fix login"})))
        .respond_with(github_response(
            "pulls/update",
            200,
            pr("DF-1: Fix login", false),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    mount_comment(&server).await?;

    let client = client(&server, &[("draft-mode", "title-prefix")])?;
    let project = ProjectId::new();
    let created = client.create_pull_request(&project, new_draft()).await?;
    assert!(created.draft);
    assert_eq!(created.title, "DF-1: Fix login");
    let found = client
        .find_open_pull_request(&project, "factory/task/DF-1", "main")
        .await?
        .expect("open pull request");
    assert_eq!((found.id.number, found.draft), (7, true));
    let updated = client.update_pull_request(&project, 7, ready()).await?;
    assert!(!updated.draft);
    let comment = client.add_issue_comment(&project, 7, "hello").await?;
    assert_eq!(comment.id, "55");
    assert_requests_match_the_extract(&server).await;
    Ok(())
}

#[tokio::test]
async fn native_lifecycle_uses_the_draft_flag_and_graphql() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PULLS))
        .and(body_json(json!({
            "title": "DF-1: Fix login", "body": "body",
            "head": "factory/task/DF-1", "base": "main", "draft": true
        })))
        .respond_with(github_response(
            "pulls/create",
            201,
            pr("DF-1: Fix login", true),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(PULLS))
        .and(query_param("head", "o:factory/task/DF-1"))
        .respond_with(github_response(
            "pulls/list",
            200,
            json!([pr("DF-1: Fix login", true)]),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{PULLS}/7")))
        .respond_with(github_response(
            "pulls/get",
            200,
            pr("DF-1: Fix login", true),
        )?)
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("markPullRequestReadyForReview"))
        .and(body_partial_json(json!({"variables": {"id": NODE_ID}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{PULLS}/7")))
        .respond_with(github_response(
            "pulls/get",
            200,
            pr("DF-1: Fix login", false),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .respond_with(github_response(
            "pulls/update",
            200,
            pr("DF-1: Fix login", false),
        )?)
        .expect(0)
        .mount(&server)
        .await;
    mount_comment(&server).await?;

    let client = client(&server, &[])?;
    let project = ProjectId::new();
    let created = client.create_pull_request(&project, new_draft()).await?;
    assert!(created.draft);
    let found = client
        .find_open_pull_request(&project, "factory/task/DF-1", "main")
        .await?
        .expect("open pull request");
    assert!(found.draft);
    let updated = client.update_pull_request(&project, 7, ready()).await?;
    assert!(!updated.draft);
    client.add_issue_comment(&project, 7, "hello").await?;
    assert_requests_match_the_extract(&server).await;
    Ok(())
}

#[tokio::test]
async fn every_request_carries_the_github_headers_and_the_host_injected_token() -> anyhow::Result<()>
{
    let server = MockServer::start().await;
    let token = format!("Bearer {}", common::TOKEN);
    Mock::given(method("POST"))
        .and(path(PULLS))
        .and(header_regex("user-agent", "^dark-factory-github-plugin/"))
        .and(header("accept", "application/vnd.github+json"))
        .and(header("x-github-api-version", "2026-03-10"))
        .and(header("authorization", token.as_str()))
        .and(header("content-type", "application/json"))
        .respond_with(github_response(
            "pulls/create",
            201,
            pr("WIP: DF-1: Fix login", false),
        )?)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{PULLS}/7")))
        .respond_with(github_response(
            "pulls/get",
            200,
            pr("WIP: DF-1: Fix login", false),
        )?)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .respond_with(github_response(
            "pulls/update",
            200,
            pr("DF-1: Fix login", false),
        )?)
        .mount(&server)
        .await;
    mount_comment(&server).await?;

    let client = client(&server, &[("draft-mode", "title-prefix")])?;
    let project = ProjectId::new();
    client.create_pull_request(&project, new_draft()).await?;
    client.update_pull_request(&project, 7, ready()).await?;
    client.add_issue_comment(&project, 7, "hello").await?;

    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        let get = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        assert!(
            get("user-agent").starts_with("dark-factory-github-plugin/"),
            "{} {}",
            request.method,
            request.url
        );
        assert_eq!(get("accept"), "application/vnd.github+json");
        assert_eq!(get("x-github-api-version"), "2026-03-10");
        assert_eq!(get("authorization"), token);
    }
    assert_requests_match_the_extract(&server).await;
    Ok(())
}

#[tokio::test]
async fn closed_issue_with_a_null_body_is_read_with_its_labels() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/issues/9"))
        .respond_with(github_response(
            "issues/get",
            200,
            json!({
                "number": 9, "title": "Login fails", "body": null, "state": "closed",
                "html_url": "https://github.example/o/r/issues/9",
                "labels": [{"name": "bug"}, {"name": "factory"}]
            }),
        )?)
        .expect(1)
        .mount(&server)
        .await;

    let issue = client(&server, &[])?
        .get_issue(&ProjectId::new(), 9)
        .await?;

    assert_eq!(issue.id.number, 9);
    assert_eq!(issue.title, "Login fails");
    assert_eq!(issue.body, "");
    assert_eq!(issue.state, IssueState::Closed);
    assert_eq!(issue.labels, ["bug", "factory"]);
    assert_eq!(issue.url, "https://github.example/o/r/issues/9");
    assert_requests_match_the_extract(&server).await;
    Ok(())
}

#[tokio::test]
async fn rate_limit_with_a_distant_reset_is_returned_without_a_retry() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let reset = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 600;
    Mock::given(method("POST"))
        .respond_with(
            github_response(
                "issues/create-comment",
                403,
                json!({"message": "API rate limit exceeded"}),
            )?
            .insert_header("x-ratelimit-remaining", "0")
            .insert_header("x-ratelimit-reset", reset.to_string().as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client(&server, &[])?
        .add_issue_comment(&ProjectId::new(), 7, "hello")
        .await
        .unwrap_err();
    // Above the host's 30 s retry cap; the first call may spend a while compiling the component.
    assert!(
        matches!(
            err,
            IntegrationError::RateLimited(RateLimit { retry_after_seconds: Some(s), .. })
                if (31..=600).contains(&s)
        ),
        "{err:?}"
    );
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        1
    );
    assert_requests_match_the_extract(&server).await;
    Ok(())
}
