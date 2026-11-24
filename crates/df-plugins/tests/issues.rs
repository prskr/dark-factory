//! Host calls `issues.list`, `issues.create` and the `pull-request` flag of `issues.get` against
//! the test-probe plugin, which echoes its arguments and reports `429` as `rate-limited`.

mod common;

use std::time::Duration;

use df_core::{ForgeClient, IssueFilter, IssueStateFilter, NewIssue, ProjectId};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;

fn probe_client(server: &MockServer) -> anyhow::Result<common::Client> {
    common::client(
        &common::probe(),
        &format!("{}/o/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )
}

/// A mock forge that answers `at` with `429` once, then with `200`.
async fn rate_limited_once(at: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn list_issues_round_trips_filter_cursor_and_page_and_retries_a_rate_limit()
-> anyhow::Result<()> {
    let server = rate_limited_once("/issues/list").await;
    let client = probe_client(&server)?;
    let filter = IssueFilter {
        state: IssueStateFilter::Closed,
        labels: vec!["bug".into(), "ui".into()],
        query: Some("timeout".into()),
    };

    let page = client
        .list_issues(&ProjectId::new(), &filter, 7, Some("c1"))
        .await?;

    assert_eq!(page.issues.len(), 1);
    assert_eq!(page.issues[0].title, "closed|bug,ui|timeout|7|c1");
    assert!(!page.issues[0].pull_request);
    assert_eq!(page.next_cursor.as_deref(), Some("c2"));
    assert!(page.truncated);
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 2, "one rate-limited call and one retry");
    Ok(())
}

#[tokio::test]
async fn create_issue_round_trips_and_retries_a_rate_limit() -> anyhow::Result<()> {
    let server = rate_limited_once("/issues/create").await;
    let client = probe_client(&server)?;

    let issue = client
        .create_issue(
            &ProjectId::new(),
            &NewIssue {
                title: "Flaky".into(),
                body: "Details".into(),
                labels: vec!["bug".into()],
            },
        )
        .await?;

    assert_eq!(issue.id.number, 99);
    assert_eq!(issue.title, "Flaky");
    assert_eq!(issue.body, "Details");
    assert_eq!(issue.labels, vec!["bug".to_string()]);
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 2, "one rate-limited call and one retry");
    Ok(())
}

#[tokio::test]
async fn get_issue_maps_the_pull_request_flag() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let client = probe_client(&server)?;

    let pull_request = client.get_issue(&ProjectId::new(), 40).await?;
    let issue = client.get_issue(&ProjectId::new(), 41).await?;

    assert!(pull_request.pull_request);
    assert!(!issue.pull_request);
    Ok(())
}
