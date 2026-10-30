//! The Forgejo component, run by the host against a mock Forgejo (specs `forgejo-integration` and
//! the credential/host rules of `plugin-host`).

mod common;

use std::time::Duration;

use df_core::{
    ForgeClient, IntegrationError, NewPullRequest, ProjectId, PullRequestState, PullRequestUpdate,
};
use df_plugins::PluginEngine;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;
const PULLS: &str = "/api/v1/repos/o/r/pulls";

fn client(server: &MockServer) -> anyhow::Result<common::Client> {
    common::client(
        &common::forgejo(),
        &format!("{}/o/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )
}

fn pr(title: &str) -> serde_json::Value {
    json!({
        "number": 7, "html_url": "http://forge/o/r/pulls/7", "title": title, "body": "body",
        "state": "open", "merged": false,
        "head": {"ref": "factory/task/DF-1", "sha": "abc123"}, "base": {"ref": "main", "sha": "def"}
    })
}

fn new_pr(draft: bool) -> NewPullRequest {
    NewPullRequest {
        repo: df_core::RepoRef {
            owner: String::new(),
            name: String::new(),
        },
        head: "factory/task/DF-1".into(),
        base: "main".into(),
        title: "DF-1: Fix login".into(),
        body: "body".into(),
        draft,
    }
}

#[tokio::test]
async fn draft_pull_request_carries_the_marker_and_the_host_injected_token() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PULLS))
        .and(header(
            "authorization",
            format!("token {}", common::TOKEN).as_str(),
        ))
        .and(body_json(json!({
            "title": "WIP: DF-1: Fix login", "body": "body",
            "head": "factory/task/DF-1", "base": "main"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(pr("WIP: DF-1: Fix login")))
        .expect(1)
        .mount(&server)
        .await;

    let created = client(&server)?
        .create_pull_request(&ProjectId::new(), new_pr(true))
        .await?;
    assert_eq!(created.title, "DF-1: Fix login");
    assert!(created.draft);
    assert_eq!(created.id.number, 7);
    assert_eq!(created.head_sha, "abc123");
    assert_eq!(created.state, PullRequestState::Open);
    Ok(())
}

#[tokio::test]
async fn duplicate_pull_request_is_a_conflict() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PULLS))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"message": "exists"})))
        .mount(&server)
        .await;
    let err = client(&server)?
        .create_pull_request(&ProjectId::new(), new_pr(false))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Conflict(_)), "{err}");
    Ok(())
}

#[tokio::test]
async fn find_open_recovers_a_pull_request() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PULLS))
        .and(query_param("state", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([pr("DF-1: Fix login")])))
        .mount(&server)
        .await;
    let found = client(&server)?
        .find_open_pull_request(&ProjectId::new(), "factory/task/DF-1", "main")
        .await?
        .expect("open pull request");
    assert_eq!((found.id.number, found.head_sha.as_str()), (7, "abc123"));
    assert_eq!(found.url, "http://forge/o/r/pulls/7");
    Ok(())
}

#[tokio::test]
async fn ready_for_review_removes_the_marker() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{PULLS}/7")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr("WIP: DF-1: Fix login")))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{PULLS}/7")))
        .and(body_json(json!({"title": "DF-1: Fix login"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr("DF-1: Fix login")))
        .expect(1)
        .mount(&server)
        .await;
    let updated = client(&server)?
        .update_pull_request(
            &ProjectId::new(),
            7,
            PullRequestUpdate {
                draft: Some(false),
                ..Default::default()
            },
        )
        .await?;
    assert!(!updated.draft);
    Ok(())
}

#[tokio::test]
async fn comment_on_a_pull_request_number() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/repos/o/r/issues/7/comments"))
        .and(body_json(json!({"body": "hello"})))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"id": 55, "html_url": "http://c/55"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let comment = client(&server)?
        .add_issue_comment(&ProjectId::new(), 7, "hello")
        .await?;
    assert_eq!(
        (comment.id.as_str(), comment.url.as_str()),
        ("55", "http://c/55")
    );
    Ok(())
}

#[tokio::test]
async fn issue_is_read() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/issues/3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "html_url": "http://i/3", "title": "Login broken", "body": "Details",
            "state": "open", "labels": [{"name": "bug"}]
        })))
        .mount(&server)
        .await;
    let issue = client(&server)?.get_issue(&ProjectId::new(), 3).await?;
    assert_eq!(
        (issue.title.as_str(), issue.labels.as_slice()),
        ("Login broken", ["bug".to_string()].as_slice())
    );
    Ok(())
}

#[tokio::test]
async fn bad_token_is_unauthorized() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"message": "bad token"})))
        .mount(&server)
        .await;
    let err = client(&server)?
        .get_issue(&ProjectId::new(), 3)
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Unauthorized(_)), "{err}");
    Ok(())
}

#[tokio::test]
async fn rate_limited_comment_is_retried_after_the_hint() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"id": 1, "html_url": "http://c/1"})),
        )
        .mount(&server)
        .await;
    let comment = client(&server)?
        .add_issue_comment(&ProjectId::new(), 7, "hi")
        .await?;
    assert_eq!(comment.id, "1");
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        2
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn plain_http_is_denied_without_opening_a_connection() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let client = common::https_only_router(
        common::plugin_client(
            &PluginEngine::new()?,
            &common::forgejo(),
            common::TOKEN,
            Duration::from_secs(30),
            64 * MIB,
        )?,
        &format!("{}/o/r.git", server.uri()),
    )?;
    let err = client.get_issue(&ProjectId::new(), 3).await.unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Config(m) if m.contains("is not HTTPS")),
        "{err}"
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    Ok(())
}
