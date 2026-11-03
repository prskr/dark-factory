//! Host calls of the review loop (`pull-requests.list-feedback` and `reply`) against the test-probe
//! plugin, which answers from a mock server and reports `429` as `rate-limited`.

mod common;

use std::time::Duration;

use df_core::{
    Author, AuthorRole, Comment, Feedback, ForgeClient, PrComment, ProjectId, Review,
    ReviewComment, ReviewThread, ReviewVerdict,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;

/// A mock forge that answers `path` with `429` once, then with `200`.
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

fn probe_client(server: &MockServer) -> anyhow::Result<common::Client> {
    common::client(
        &common::probe(),
        &format!("{}/o/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )
}

fn author(login: &str, role: AuthorRole) -> Author {
    Author {
        login: login.into(),
        role,
    }
}

#[tokio::test]
async fn list_feedback_round_trips_and_retries_a_rate_limit() -> anyhow::Result<()> {
    let server = rate_limited_once("/pulls/7/feedback").await;
    let client = probe_client(&server)?;

    let feedback = client
        .list_pull_request_feedback(&ProjectId::new(), 7, Some("2026-11-01T00:00:00Z"))
        .await?;

    let at = "2026-11-02T10:00:00Z".to_string();
    let expected = Feedback {
        comments: vec![PrComment {
            id: "c7".into(),
            author: author("alice", AuthorRole::Member),
            body: "since 2026-11-01T00:00:00Z".into(),
            url: "https://forge.test/o/r/pulls/7#c7".into(),
            created_at: at.clone(),
            updated_at: at.clone(),
        }],
        reviews: vec![Review {
            id: "r1".into(),
            author: author("mallory", AuthorRole::Outside),
            verdict: ReviewVerdict::ChangesRequested,
            body: "please change".into(),
            url: "https://forge.test/o/r/pulls/7#r1".into(),
            submitted_at: at.clone(),
        }],
        threads: vec![ReviewThread {
            id: "t1".into(),
            path: "src/lib.rs".into(),
            line: Some(12),
            resolved: true,
            comments: vec![ReviewComment {
                id: "rc1".into(),
                author: author("factory-bot", AuthorRole::Bot),
                body: "done".into(),
                url: "https://forge.test/o/r/pulls/7#rc1".into(),
                created_at: at.clone(),
                updated_at: at,
            }],
        }],
        truncated: true,
    };
    assert_eq!(feedback, expected);
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 2, "one rate-limited call and one retry");
    Ok(())
}

#[tokio::test]
async fn reply_round_trips_and_retries_a_rate_limit() -> anyhow::Result<()> {
    let server = rate_limited_once("/pulls/7/reply").await;
    let client = probe_client(&server)?;

    let posted = client
        .reply_to_review_thread(&ProjectId::new(), 7, "t1", "fixed in abc123")
        .await?;

    assert_eq!(
        posted,
        Comment {
            id: "7:t1".into(),
            url: "fixed in abc123".into(),
        }
    );
    let requests = server.received_requests().await.unwrap_or_default();
    assert_eq!(requests.len(), 2, "one rate-limited call and one retry");
    Ok(())
}
