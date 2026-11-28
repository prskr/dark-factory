//! Identification, sessions, protocol and the methods of the endpoint.

use super::*;
use crate::mcp::testing::{FakeMcp, tool};

#[tokio::test]
async fn unknown_pod_gets_403_and_no_snapshot_503() {
    let none = Env::start(None).await;
    let (status, _, _) = none.send(Method::POST, &[], "{}").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let env = Env::start(Some(policy(&[], &[]))).await;
    // Another source address than the snapshot's sandbox.
    let other = Policy::default();
    env.set_policy(other);
    let (status, _, _) = env.send(Method::POST, &[], "{}").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn initialize_answers_tools_capability_only_and_a_session_bound_to_the_run() {
    let env = Env::start(Some(policy(&[], &[]))).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {"sampling": {}, "roots": {}}}});

    let (status, headers, text) = env.send(Method::POST, &[], &body.to_string()).await;

    assert_eq!(status, StatusCode::OK);
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        answer["result"]["capabilities"],
        json!({"tools": {"listChanged": false}})
    );
    assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
    let session = headers["mcp-session-id"].to_str().unwrap();
    assert!(session.starts_with(&format!("df-{RUN}-")), "{session}");
}

#[tokio::test]
async fn an_unknown_protocol_version_is_answered_with_the_latest() {
    let env = Env::start(Some(policy(&[], &[]))).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2030-01-01", "capabilities": {}}});

    let (_, _, text) = env.send(Method::POST, &[], &body.to_string()).await;

    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["result"]["protocolVersion"], "2025-11-25");
}

#[tokio::test]
async fn get_is_405_and_delete_is_204_without_contacting_an_upstream() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;

    let (status, headers, _) = env.send(Method::GET, &[], "").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(headers["allow"], "POST, DELETE");
    let session = env.session().await;
    let (status, _, _) = env
        .send(Method::DELETE, &[("mcp-session-id", &session)], "")
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn sessions_are_checked_without_state_on_the_pod() {
    let env = Env::start(Some(policy(&[], &[]))).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();

    let (status, _, _) = env.send(Method::POST, &[], &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing session id");
    let other_run = format!("df-0197a000-0000-7000-8000-0000000000ff-{}", "a".repeat(32));
    let (status, _, _) = env
        .send(Method::POST, &[("mcp-session-id", &other_run)], &body)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another run's session");
    let (status, _, _) = env
        .send(Method::POST, &[("mcp-session-id", "garbage")], &body)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "malformed session");

    // A session issued by another gateway pod for this run (same format, other random part).
    let other_pod = format!("df-{RUN}-{}", "b".repeat(32));
    let (status, _, _) = env
        .send(Method::POST, &[("mcp-session-id", &other_pod)], &body)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn unsupported_protocol_header_and_batches_are_400() {
    let env = Env::start(Some(policy(&[], &[]))).await;
    let session = env.session().await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();

    let (status, _, _) = env
        .send(
            Method::POST,
            &[
                ("mcp-session-id", &session),
                ("mcp-protocol-version", "1999-01-01"),
            ],
            &body,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let batch = json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}, {"jsonrpc": "2.0", "id": 2, "method": "ping"}]);
    let (status, _, _) = env
        .send(
            Method::POST,
            &[("mcp-session-id", &session)],
            &batch.to_string(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = env
        .send(Method::POST, &[("mcp-session-id", &session)], "not json")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn only_tools_methods_are_served_and_notifications_accepted() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;

    let answer = env.rpc(&session, "resources/list", json!({})).await;
    assert_eq!(answer["error"]["code"], -32601);
    let ping = env.rpc(&session, "ping", json!({})).await;
    assert_eq!(ping["result"], json!({}));
    for notification in ["notifications/initialized", "notifications/cancelled"] {
        let body = json!({"jsonrpc": "2.0", "method": notification, "params": {"requestId": 99}});
        let (status, _, _) = env
            .send(
                Method::POST,
                &[("mcp-session-id", &session)],
                &body.to_string(),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{notification}");
    }
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn a_request_body_over_1_mib_is_413_and_contacts_no_upstream() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;
    let big = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "wiki__a", "arguments": {"x": "y".repeat(2 * 1024 * 1024)}}});

    let (status, _, _) = env
        .send(
            Method::POST,
            &[("mcp-session-id", &session)],
            &big.to_string(),
        )
        .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn a_known_sandbox_without_servers_gets_an_empty_list() {
    let env = Env::start(Some(policy(&[], &[]))).await;
    let session = env.session().await;

    assert!(env.tool_names(&session).await.is_empty());
}
