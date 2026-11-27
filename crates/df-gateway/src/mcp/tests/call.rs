//! `tools/call`.

use df_core::mcp::McpCallOutcome;

use super::*;
use crate::mcp::testing::{Answer, FakeMcp, Seen, Standard, tool};

fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn env_with(fake: &FakeMcp) -> (Env, String) {
    let env = Env::start(Some(policy(
        &[server("s1", "context7", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;
    (env, session)
}

fn call(name: &str, arguments: Value) -> Value {
    json!({"name": name, "arguments": arguments})
}

#[tokio::test]
async fn a_call_is_forwarded_with_the_name_after_the_prefix_and_the_arguments_unchanged() {
    let fake = FakeMcp::start(vec![tool("get-docs")]).await;
    let (env, session) = env_with(&fake).await;

    let answer = env
        .rpc(
            &session,
            "tools/call",
            call(
                "context7__get-docs",
                json!({"libraryId": "/tokio-rs/tokio"}),
            ),
        )
        .await;

    assert_eq!(
        text(&answer),
        r#"called get-docs {"libraryId":"/tokio-rs/tokio"}"#
    );
    assert_eq!(answer["result"].get("isError"), None);
    env.records.wait_for(1).await;
    let record = &env.records.all()[0];
    assert_eq!(
        (record.server_name.as_str(), record.tool.as_str()),
        ("context7", "get-docs")
    );
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::Ok);
    assert!(record.request_bytes > 0 && record.response_bytes > 0);
}

#[tokio::test]
async fn the_upstream_gets_only_the_servers_credential_never_the_sandboxs() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let (env, session) = env_with(&fake).await;

    env.rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    for seen in fake.requests() {
        assert_eq!(seen.header("authorization"), Some("Bearer tok-s1"));
        assert_eq!(seen.header("context7_api_key"), None);
        assert!(
            seen.header("mcp-session-id")
                .is_none_or(|id| !id.starts_with("df-"))
        );
    }
}

#[tokio::test]
async fn a_call_to_a_server_not_enabled_for_the_run_is_rejected_without_reaching_it() {
    let billing = FakeMcp::start(vec![tool("export")]).await;
    let wiki = FakeMcp::start(vec![tool("a")]).await;
    let servers = [
        server("s1", "wiki", &wiki.url(), 1),
        server("s2", "billing", &billing.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1"]))).await;
    let session = env.session().await;

    let answer = env
        .rpc(&session, "tools/call", call("billing__export", json!({})))
        .await;

    assert_eq!(answer["result"]["isError"], true);
    assert_eq!(
        text(&answer),
        "tool billing__export is not available in this run"
    );
    assert!(billing.requests().is_empty());
    env.records.wait_for(1).await;
    assert_eq!(
        env.records.summary(),
        [(
            "billing".to_owned(),
            "export".to_owned(),
            McpCallOutcome::Rejected
        )]
    );
}

#[tokio::test]
async fn a_server_disabled_after_listing_is_rejected_at_once() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let servers = [server("s1", "context7", &fake.url(), 1)];
    let env = Env::start(Some(policy(&servers, &["s1"]))).await;
    let session = env.session().await;
    env.tool_names(&session).await;
    env.set_policy(policy(&servers, &[]));

    let answer = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    assert_eq!(answer["result"]["isError"], true);
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::Rejected);
}

#[tokio::test]
async fn a_name_without_a_server_prefix_is_rejected() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let (env, session) = env_with(&fake).await;

    let answer = env
        .rpc(&session, "tools/call", call("plain", json!({})))
        .await;

    assert_eq!(text(&answer), "tool plain is not available in this run");
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn an_upstream_json_rpc_error_becomes_a_tool_error() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let m = seen.json();
        match m.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "error": {"code": -32602, "message": "unknown tool: no-such-tool"}})),
            _ => Answer::Accepted,
        }
    }))
    .await;
    let (env, session) = env_with(&fake).await;

    let answer = env
        .rpc(
            &session,
            "tools/call",
            call("context7__no-such-tool", json!({})),
        )
        .await;

    assert_eq!(answer["result"]["isError"], true);
    assert_eq!(text(&answer), "unknown tool: no-such-tool");
    env.records.wait_for(1).await;
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::ToolError);
}

#[tokio::test]
async fn a_result_with_is_error_is_forwarded_and_recorded_as_tool_error() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let m = seen.json();
        match m.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"content": [{"type": "text", "text": "nope"}], "isError": true, "structuredContent": {"k": 1}}})),
            _ => Answer::Accepted,
        }
    }))
    .await;
    let (env, session) = env_with(&fake).await;

    let answer = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    assert_eq!(answer["result"]["structuredContent"], json!({"k": 1}));
    env.records.wait_for(1).await;
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::ToolError);
}

#[tokio::test]
async fn upstream_failures_become_fixed_messages_with_outcome_upstream_error() {
    for (answer, message) in [
        (Answer::Status(401), "upstream refused the credential (401)"),
        (Answer::Status(500), "upstream error (500)"),
        (Answer::Redirect("http://10.0.0.5/"), "upstream redirected"),
    ] {
        let fake = FakeMcp::start_with(Arc::new(move |_: &Seen| answer.clone())).await;
        let (env, session) = env_with(&fake).await;

        let result = env
            .rpc(&session, "tools/call", call("context7__a", json!({})))
            .await;

        assert_eq!(result["result"]["isError"], true);
        assert_eq!(text(&result), message);
        env.records.wait_for(1).await;
        assert_eq!(env.records.summary()[0].2, McpCallOutcome::UpstreamError);
        assert_eq!(fake.requests().len(), 1, "a redirect is not followed");
    }
}

#[tokio::test]
async fn an_upstream_that_refuses_the_connection_is_an_upstream_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    drop(listener);
    let env = Env::start(Some(policy(&[server("s1", "context7", &url, 1)], &["s1"]))).await;
    let session = env.session().await;

    let answer = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    assert_eq!(text(&answer), "upstream unreachable");
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::UpstreamError);
}

#[tokio::test]
async fn a_server_without_credential_or_scheme_is_not_contacted() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let mut no_scheme = server("s2", "wiki", &fake.url(), 1);
    no_scheme.credential_scheme = None;
    let servers = [server("nocred", "context7", &fake.url(), 1), no_scheme];
    let env = Env::start(Some(policy(&servers, &["nocred", "s2"]))).await;
    let session = env.session().await;

    for name in ["context7__a", "wiki__a"] {
        let answer = env.rpc(&session, "tools/call", call(name, json!({}))).await;
        assert_eq!(answer["result"]["isError"], true, "{name}");
        assert_eq!(text(&answer), "upstream credential unavailable", "{name}");
    }
    assert!(env.tool_names(&session).await.is_empty());
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn the_credential_is_redacted_in_results_and_errors() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let m = seen.json();
        match m.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") if m["params"]["name"] == "fail" => {
                Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "error": {"code": -1, "message": "bad https://x/mcp?token=tok-s1"}}))
            }
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"content": [{"type": "text", "text": "your token is tok-s1"}]}})),
            _ => Answer::Accepted,
        }
    }))
    .await;
    let (env, session) = env_with(&fake).await;

    let ok = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;
    let failed = env
        .rpc(&session, "tools/call", call("context7__fail", json!({})))
        .await;

    assert_eq!(text(&ok), "your token is [redacted]");
    assert_eq!(text(&failed), "bad https://x/mcp?token=[redacted]");
}

#[tokio::test(start_paused = true)]
async fn a_call_without_an_answer_times_out_after_120_seconds() {
    let standard = Arc::new(Standard::new(vec![tool("a")]));
    let inner = standard.handler();
    let fake = FakeMcp::start_with(Arc::new(move |seen: &Seen| {
        if seen.rpc_method().as_deref() == Some("tools/call") {
            Answer::Stall
        } else {
            inner(seen)
        }
    }))
    .await;
    let (env, session) = env_with(&fake).await;

    let answer = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    assert_eq!(text(&answer), "upstream timeout");
    env.records.wait_for(1).await;
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::Timeout);
}

/// A server that never answers a call, for tests of calls in progress.
async fn stalling() -> FakeMcp {
    let standard = Arc::new(Standard::new(vec![tool("a")]));
    let inner = standard.handler();
    FakeMcp::start_with(Arc::new(move |seen: &Seen| {
        if seen.rpc_method().as_deref() == Some("tools/call") {
            Answer::Stall
        } else {
            inner(seen)
        }
    }))
    .await
}

async fn calls_started(fake: &FakeMcp, count: usize) {
    while fake.methods().iter().filter(|m| *m == "tools/call").count() < count {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn the_ninth_concurrent_call_of_a_run_fails_at_once() {
    let fake = stalling().await;
    let (env, session) = env_with(&fake).await;
    let env = Arc::new(env);
    let mut running = Vec::new();
    for n in 0..8 {
        let (env, session) = (env.clone(), session.clone());
        running.push(tokio::spawn(async move {
            let body = json!({"jsonrpc": "2.0", "id": 100 + n, "method": "tools/call", "params": call("context7__a", json!({}))});
            env.send(Method::POST, &[("mcp-session-id", &session)], &body.to_string()).await
        }));
    }
    calls_started(&fake, 8).await;

    let answer = env
        .rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    assert_eq!(text(&answer), "too many concurrent calls");
    assert_eq!(
        fake.methods().iter().filter(|m| *m == "tools/call").count(),
        8
    );
    assert_eq!(
        env.records.summary(),
        [(
            "context7".to_owned(),
            "a".to_owned(),
            McpCallOutcome::Rejected
        )]
    );
    for task in running {
        task.abort();
    }
}

#[tokio::test]
async fn a_client_that_disconnects_cancels_the_call_and_frees_its_slot() {
    let fake = stalling().await;
    let (env, session) = env_with(&fake).await;
    let env = Arc::new(env);
    let call_task = tokio::spawn({
        let (env, session) = (env.clone(), session.clone());
        async move {
            env.rpc_raw(&session, "tools/call", call("context7__a", json!({})))
                .await
        }
    });
    calls_started(&fake, 1).await;

    call_task.abort();

    env.records.wait_for(1).await;
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::Cancelled);
    let slot = env.mcp.slot(RUN);
    assert_eq!(
        slot.available_permits(),
        crate::mcp::call::MAX_CONCURRENT_CALLS
    );
}

#[tokio::test]
async fn a_cancel_notification_ends_the_call_on_the_holding_pod() {
    let fake = stalling().await;
    let (env, session) = env_with(&fake).await;
    let env = Arc::new(env);
    let call_task = tokio::spawn({
        let (env, session) = (env.clone(), session.clone());
        async move {
            let body = json!({"jsonrpc": "2.0", "id": 42, "method": "tools/call", "params": call("context7__a", json!({}))});
            env.send(
                Method::POST,
                &[("mcp-session-id", &session)],
                &body.to_string(),
            )
            .await
        }
    });
    calls_started(&fake, 1).await;

    let cancel =
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 42}});
    let (status, _, _) = env
        .send(
            Method::POST,
            &[("mcp-session-id", &session)],
            &cancel.to_string(),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _, body) = call_task.await.unwrap();
    assert_eq!((status, body.as_str()), (StatusCode::ACCEPTED, ""));
    env.records.wait_for(1).await;
    assert_eq!(env.records.summary()[0].2, McpCallOutcome::Cancelled);
    while !fake
        .methods()
        .contains(&"notifications/cancelled".to_owned())
    {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_cancel_for_a_call_on_another_pod_is_ignored() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let (env, session) = env_with(&fake).await;

    let cancel =
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 5}});
    let (status, _, _) = env
        .send(
            Method::POST,
            &[("mcp-session-id", &session)],
            &cancel.to_string(),
        )
        .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(env.records.all().is_empty());
}

#[tokio::test]
async fn a_server_that_left_the_snapshot_has_its_upstream_session_ended_by_the_sweep() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let servers = [server("s1", "context7", &fake.url(), 1)];
    let env = Env::start(Some(policy(&servers, &["s1"]))).await;
    let session = env.session().await;
    env.rpc(&session, "tools/call", call("context7__a", json!({})))
        .await;

    let gone = policy(&servers, &[]);
    env.mcp.sweep(Some(&gone)).await;

    assert!(fake.methods().contains(&"DELETE".to_owned()));
}

#[tokio::test]
async fn nothing_but_names_outcomes_and_ids_reaches_the_logs() {
    let (captured, _guard) = df_telemetry::testing::capture("debug");
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let m = seen.json();
        match m.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"content": [{"type": "text", "text": "RESULT-SECRET-TEXT"}]}})),
            Some("tools/list") => Answer::Json(json!({"jsonrpc": "2.0", "id": m["id"],
                "result": {"tools": [{"name": "a", "description": "DESCRIPTION-SECRET", "inputSchema": {"type": "object"}}]}})),
            _ => Answer::Accepted,
        }
    }))
    .await;
    let (env, session) = env_with(&fake).await;

    env.tool_names(&session).await;
    env.rpc(
        &session,
        "tools/call",
        call("context7__a", json!({"apiKey": "ARGUMENT-SECRET"})),
    )
    .await;
    env.records.wait_for(1).await;

    let logs = captured.text();
    for secret in [
        "ARGUMENT-SECRET",
        "RESULT-SECRET-TEXT",
        "DESCRIPTION-SECRET",
        "tok-s1",
    ] {
        assert!(!logs.contains(secret), "{secret} in {logs}");
    }
    assert!(logs.contains("context7"), "{logs}");
}
