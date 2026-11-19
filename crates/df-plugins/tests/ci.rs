//! CI reads and the `list-open` filters through the host: the test-probe plugin for the contract
//! and the retry rules, the Forgejo component for the `Range` tail of job logs.

mod common;

use std::time::Duration;

use df_core::{CiState, ForgeClient, IntegrationError, ProjectId, PullRequestFilter};
use df_plugins::PluginEngine;
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;

fn client(component: &[u8], server: &MockServer) -> anyhow::Result<common::Client> {
    common::client(
        component,
        &format!("{}/o/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )
}

#[tokio::test]
async fn the_probe_reports_ci_status_with_job_ids() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ci/feature"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let status = client(&common::probe(), &server)?
        .read_ci_status(&ProjectId::new(), "feature")
        .await?;

    assert_eq!(status.state, CiState::Failure);
    let checks: Vec<_> = status
        .checks
        .iter()
        .map(|c| (c.name.as_str(), c.state, c.job_id))
        .collect();
    assert_eq!(
        checks,
        [
            ("ci / test", CiState::Failure, Some(812)),
            ("ci / lint", CiState::Success, None)
        ]
    );
    Ok(())
}

/// Scenario "Plugin without CI" is covered by `Capabilities::of_exports` and the `unsupported`
/// answer of the plugin; this checks that the plugin's own `unsupported` reaches the caller.
#[tokio::test]
async fn unsupported_and_not_found_job_logs_reach_the_caller() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = client(&common::probe(), &server)?;
    let project = ProjectId::new();

    let unsupported = client.read_ci_job_log(&project, 15).await.unwrap_err();
    let missing = client.read_ci_job_log(&project, 404).await.unwrap_err();
    let log = client.read_ci_job_log(&project, 2).await?;

    assert!(
        matches!(&unsupported, IntegrationError::Unsupported(m) if m.contains("v16")),
        "{unsupported}"
    );
    assert!(
        matches!(missing, IntegrationError::NotFound(_)),
        "{missing}"
    );
    assert_eq!(
        (log.text.as_str(), log.head_dropped),
        ("log of job 2", true)
    );
    Ok(())
}

// plugin-host, scenario "Plugin without CI": the component loads without `ci`, and CI calls for
// its projects fail as not supported without reaching the forge.
#[tokio::test]
async fn a_plugin_without_ci_loads_and_its_ci_calls_are_unsupported() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let host = df_plugins::PluginHost::from_bytes(
        &PluginEngine::new()?,
        "probe",
        &common::probe_without_ci(),
        Duration::from_secs(30),
        64 * MIB,
    )?;
    assert!(!host.capabilities().ci);
    assert!(host.capabilities().pull_requests);
    assert!(host.capabilities().issues);
    let client = client(&common::probe_without_ci(), &server)?;
    let project = ProjectId::new();

    let status = client.read_ci_status(&project, "main").await.unwrap_err();
    let log = client.read_ci_job_log(&project, 1).await.unwrap_err();

    assert!(
        matches!(status, IntegrationError::Unsupported(_)),
        "{status}"
    );
    assert!(matches!(log, IntegrationError::Unsupported(_)), "{log}");
    assert!(
        client
            .list_open_pull_requests(&project, "fix/login")
            .await?
            .is_empty(),
        "the other capabilities work"
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

#[tokio::test]
async fn a_rate_limited_ci_call_is_retried() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ci/jobs/7"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ci/jobs/7"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let log = client(&common::probe(), &server)?
        .read_ci_job_log(&ProjectId::new(), 7)
        .await?;

    assert_eq!(log.text, "log of job 7");
    assert_eq!(
        server.received_requests().await.unwrap_or_default().len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn list_pull_requests_filters_cross_the_boundary_and_return_author_and_labels()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    let client = client(&common::probe(), &server)?;
    let project = ProjectId::new();
    let renovate = PullRequestFilter {
        head_prefix: Some("renovate/".into()),
        label: Some("major".into()),
        ..PullRequestFilter::default()
    };

    let listed = client.list_pull_requests(&project, &renovate).await?;
    let other = client
        .list_pull_requests(
            &project,
            &PullRequestFilter {
                author: Some("alice".into()),
                ..PullRequestFilter::default()
            },
        )
        .await?;
    let unsupported = client
        .list_pull_requests(
            &project,
            &PullRequestFilter {
                author: Some("unsupported".into()),
                ..PullRequestFilter::default()
            },
        )
        .await
        .unwrap_err();

    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].pr.id.number,
            listed[0].author.as_str(),
            listed[0].labels.as_slice()
        ),
        (40, "renovate-bot", ["major".to_string()].as_slice())
    );
    assert!(other.is_empty());
    assert!(
        matches!(unsupported, IntegrationError::Unsupported(_)),
        "{unsupported}"
    );
    assert_eq!(client.get_pull_request(&project, 40).await?.head_sha, "abc");
    Ok(())
}

#[tokio::test]
async fn forgejo_v16_log_is_read_with_a_range_and_a_cut_start_is_reported() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version": "16.0.0"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/actions/jobs/812/logs"))
        .and(header("range", "bytes=-4194304"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 100-119/120")
                .set_body_string("end of the log\n"),
        )
        .mount(&server)
        .await;

    let log = client(&common::forgejo(), &server)?
        .read_ci_job_log(&ProjectId::new(), 812)
        .await?;

    assert_eq!(log.text, "end of the log\n");
    assert!(log.head_dropped, "the range starts at byte 100");
    Ok(())
}

#[tokio::test]
async fn forgejo_status_and_pull_request_listing_round_trip() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/commits/main/status"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc",
            "statuses": [{"context": "ci / test (push)", "status": "failure", "target_url": "https://git/x"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/o/r/commits/main/status"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"statuses": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version": "15.0.0"})))
        .mount(&server)
        .await;
    let client = client(&common::forgejo(), &server)?;

    let status = client.read_ci_status(&ProjectId::new(), "main").await?;

    assert_eq!(status.state, CiState::Failure);
    assert_eq!(status.checks[0].url.as_deref(), Some("https://git/x"));
    assert_eq!(status.checks[0].job_id, None, "v15 has no job ids");
    Ok(())
}
