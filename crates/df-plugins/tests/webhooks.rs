//! Webhook handling in the host: signature check, event acceptance, no network while parsing.

mod common;

use std::time::Duration;

use df_core::webhooks::{ParsedEvent, WebhookEvent};
use df_core::{AuthorRole, ForgeClient, IntegrationError, ProjectId};
use df_plugins::{Capabilities, PluginEngine, PluginForgeClient, PluginHost, WebhookError};
use secrecy::SecretString;

const SECRET: &str = "test-secret";
const BODY: &[u8] = br#"{"action":"probe"}"#;

fn probe_client() -> anyhow::Result<PluginForgeClient> {
    common::plugin_client(
        &PluginEngine::new()?,
        &common::probe(),
        common::TOKEN,
        Duration::from_secs(30),
        64 << 20,
    )
}

fn webhook_client() -> anyhow::Result<PluginForgeClient> {
    Ok(probe_client()?.with_webhook_secret(SecretString::from(SECRET)))
}

fn signature(secret: &str, body: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    hex::encode(ring::hmac::sign(&key, body))
}

fn headers(mode: &str, signature: &str) -> Vec<(String, String)> {
    vec![
        ("X-Probe-Mode".to_string(), mode.to_string()),
        ("X-Probe-Signature".to_string(), signature.to_string()),
    ]
}

#[tokio::test]
async fn correctly_signed_delivery_yields_the_parsed_events() -> anyhow::Result<()> {
    let events = webhook_client()?
        .handle_webhook(&headers("verify", &signature(SECRET, BODY)), BODY)
        .await?;

    assert_eq!(
        events,
        vec![ParsedEvent {
            repo_url: "https://forge.test/o/r".to_string(),
            event: WebhookEvent::CiFailed {
                head: "main".to_string(),
                check: "ci".to_string(),
                url: String::new(),
            },
        }]
    );
    Ok(())
}

#[tokio::test]
async fn wrong_or_empty_signature_is_unverified() -> anyhow::Result<()> {
    let client = webhook_client()?;
    for signature in [signature("other-secret", BODY), String::new()] {
        let result = client
            .handle_webhook(&headers("verify", &signature), BODY)
            .await;
        assert!(
            matches!(result, Err(WebhookError::Unverified)),
            "{result:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn signature_over_another_body_is_unverified() -> anyhow::Result<()> {
    let result = webhook_client()?
        .handle_webhook(&headers("verify", &signature(SECRET, b"other")), BODY)
        .await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn events_returned_without_verification_are_rejected() -> anyhow::Result<()> {
    let result = webhook_client()?
        .handle_webhook(&headers("skip", ""), BODY)
        .await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn http_request_while_parsing_fails_with_a_transport_error() -> anyhow::Result<()> {
    let result = webhook_client()?
        .handle_webhook(&headers("http", &signature(SECRET, BODY)), BODY)
        .await;
    assert!(
        matches!(
            result,
            Err(WebhookError::Failed(IntegrationError::Transport(_)))
        ),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn rejected_payload_is_invalid_input() -> anyhow::Result<()> {
    let result = webhook_client()?
        .handle_webhook(&headers("invalid", ""), BODY)
        .await;
    assert!(
        matches!(result, Err(WebhookError::InvalidInput(_))),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn plugin_without_webhook_secret_has_no_endpoint() -> anyhow::Result<()> {
    let result = probe_client()?
        .handle_webhook(&headers("verify", &signature(SECRET, BODY)), BODY)
        .await;
    assert!(matches!(result, Err(WebhookError::NotFound)), "{result:?}");
    Ok(())
}

#[tokio::test]
async fn router_exposes_a_webhook_secret_only_for_a_loaded_plugin_that_has_one()
-> anyhow::Result<()> {
    let with_secret = common::router(webhook_client()?, "https://forge.test/o/r")?;
    assert!(
        with_secret
            .client("forgejo")
            .is_some_and(|client| client.webhook_secret().is_some())
    );
    assert!(with_secret.client("github").is_none());
    let without_secret = common::router(probe_client()?, "https://forge.test/o/r")?;
    assert!(
        without_secret
            .client("forgejo")
            .is_some_and(|client| client.webhook_secret().is_none())
    );
    Ok(())
}

#[tokio::test]
async fn router_has_no_endpoint_for_an_unknown_plugin() -> anyhow::Result<()> {
    let router = common::router(webhook_client()?, "https://forge.test/o/r")?;
    let result = router
        .handle_webhook("github", &headers("verify", &signature(SECRET, BODY)), BODY)
        .await;
    assert!(matches!(result, Err(WebhookError::NotFound)), "{result:?}");
    let events = router
        .handle_webhook(
            "forgejo",
            &headers("verify", &signature(SECRET, BODY)),
            BODY,
        )
        .await?;
    assert_eq!(events.len(), 1);
    Ok(())
}

#[tokio::test]
async fn verify_is_false_outside_a_webhook_call() -> anyhow::Result<()> {
    let router = common::router(webhook_client()?, "https://forge.test/o/r")?;
    let issue = router.get_issue(&ProjectId::new(), 8).await?;
    assert_eq!(issue.body, "false");
    Ok(())
}

#[tokio::test]
async fn whoami_and_author_role_run_on_the_project_instance() -> anyhow::Result<()> {
    let router = common::router(probe_client()?, "https://forge.test/o/r")?;
    let project = ProjectId::new();
    assert_eq!(router.whoami(&project).await?, "probe-bot");
    assert_eq!(
        router.author_role(&project, "alice").await?,
        AuthorRole::Member
    );
    assert_eq!(
        router.author_role(&project, "mallory").await?,
        AuthorRole::Outside
    );
    Ok(())
}

/// A merged pull request recorded from Forgejo 15, signed with [`SECRET`].
const FORGEJO_HEADERS: &str =
    include_str!("../../../plugins/forgejo/tests/fixtures/webhooks/pull_request_merged.headers");
const FORGEJO_BODY: &[u8] =
    include_bytes!("../../../plugins/forgejo/tests/fixtures/webhooks/pull_request_merged.json");

fn forgejo_client() -> anyhow::Result<PluginForgeClient> {
    Ok(common::plugin_client(
        &PluginEngine::new()?,
        &common::forgejo(),
        common::TOKEN,
        Duration::from_secs(30),
        64 << 20,
    )?
    .with_webhook_secret(SecretString::from(SECRET)))
}

fn recorded_headers() -> Vec<(String, String)> {
    FORGEJO_HEADERS
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.trim().to_string()))
        .collect()
}

#[tokio::test]
async fn forgejo_component_verifies_and_parses_a_recorded_delivery() -> anyhow::Result<()> {
    let events = forgejo_client()?
        .handle_webhook(&recorded_headers(), FORGEJO_BODY)
        .await?;
    assert_eq!(
        events,
        vec![ParsedEvent {
            repo_url: "http://localhost:13000/peter/demo".to_string(),
            event: WebhookEvent::PullRequestClosed {
                number: 1,
                head: "factory/task/DF-1".to_string(),
                actor: "peter".to_string(),
                merged: true,
            },
        }]
    );
    Ok(())
}

#[tokio::test]
async fn forgejo_component_rejects_a_missing_or_altered_signature() -> anyhow::Result<()> {
    let client = forgejo_client()?;
    let without: Vec<_> = recorded_headers()
        .into_iter()
        .filter(|(key, _)| key != "X-Forgejo-Signature")
        .collect();
    let result = client.handle_webhook(&without, FORGEJO_BODY).await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    let mut altered = FORGEJO_BODY.to_vec();
    altered.push(b' ');
    let result = client.handle_webhook(&recorded_headers(), &altered).await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    Ok(())
}

/// GitHub's example payload of a closed pull request (`octokit/webhooks`).
const GITHUB_CLOSED: &[u8] =
    include_bytes!("../../../plugins/github/tests/fixtures/webhooks/pull_request_closed.json");

fn github_client(secret: &str) -> anyhow::Result<PluginForgeClient> {
    Ok(common::plugin_client(
        &PluginEngine::new()?,
        &common::github(),
        common::TOKEN,
        Duration::from_secs(30),
        64 << 20,
    )?
    .with_webhook_secret(SecretString::from(secret)))
}

fn github_headers(event: &str, signature: &str) -> Vec<(String, String)> {
    vec![
        ("X-GitHub-Event".to_string(), event.to_string()),
        (
            "X-Hub-Signature-256".to_string(),
            format!("sha256={signature}"),
        ),
    ]
}

#[tokio::test]
async fn github_component_accepts_the_documented_test_signature() -> anyhow::Result<()> {
    // Test values from GitHub's "Validating webhook deliveries".
    let client = github_client("It's a Secret to Everybody")?;
    let headers = github_headers(
        "ping",
        "757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17",
    );
    let events = client.handle_webhook(&headers, b"Hello, World!").await?;
    assert!(events.is_empty(), "{events:?}");
    let result = client.handle_webhook(&headers, b"Hello, World?").await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn github_component_verifies_and_parses_a_signed_example_delivery() -> anyhow::Result<()> {
    let client = github_client(SECRET)?;
    let events = client
        .handle_webhook(
            &github_headers("pull_request", &signature(SECRET, GITHUB_CLOSED)),
            GITHUB_CLOSED,
        )
        .await?;
    assert_eq!(
        events,
        vec![ParsedEvent {
            repo_url: "https://github.com/Codertocat/Hello-World".to_string(),
            event: WebhookEvent::PullRequestClosed {
                number: 2,
                head: "changes".to_string(),
                actor: "Codertocat".to_string(),
                merged: false,
            },
        }]
    );
    let legacy_only = vec![
        ("X-GitHub-Event".to_string(), "pull_request".to_string()),
        ("X-Hub-Signature".to_string(), "sha1=00".to_string()),
    ];
    let result = client.handle_webhook(&legacy_only, GITHUB_CLOSED).await;
    assert!(
        matches!(result, Err(WebhookError::Unverified)),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn component_importing_the_signature_check_loads_with_webhooks() -> anyhow::Result<()> {
    let host = PluginHost::from_bytes(
        &PluginEngine::new()?,
        "probe",
        &common::probe(),
        Duration::from_secs(30),
        64 << 20,
    )?;
    assert_eq!(
        host.capabilities(),
        Capabilities {
            pull_requests: true,
            issues: true,
            webhooks: true,
        }
    );
    Ok(())
}
