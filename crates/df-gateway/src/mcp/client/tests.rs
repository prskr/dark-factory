use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use df_proto::gateway::v1 as pb;
use serde_json::{Value, json};

use super::*;
use crate::mcp::testing::{Answer, FakeMcp, Seen, Standard, StaticResolver, tool};
use crate::resolve::BlockedRanges;

fn client_with(resolver: HashMap<String, Vec<IpAddr>>, blocked: BlockedRanges) -> Client {
    Client::new(
        UpstreamConnector::new().unwrap().allowing_plain_http(true),
        Arc::new(StaticResolver(resolver)),
        blocked,
    )
}

/// A client that may reach the fake upstream on the loopback address.
fn client() -> Client {
    client_with(HashMap::new(), BlockedRanges::none())
}

fn key(run: &str) -> SessionKey {
    SessionKey {
        run_id: run.into(),
        server_id: "s1".into(),
    }
}

fn scheme_bearer() -> pb::mcp_server::CredentialScheme {
    pb::mcp_server::CredentialScheme::Bearer(pb::McpBearerScheme {})
}

fn target_at(url: &str, scheme: pb::mcp_server::CredentialScheme, secret: &str) -> Target {
    let server = pb::McpServer {
        id: "s1".into(),
        name: "wiki".into(),
        url: url.into(),
        credential_ref: "c".into(),
        credential_scheme: Some(scheme),
        private_network: true,
        config_version: 1,
    };
    let credential = pb::ProviderCredential {
        value: secret.into(),
    };
    Target::from_proto(&server, Some(&credential), true).unwrap()
}

fn target(fake: &FakeMcp) -> Target {
    target_at(&fake.url(), scheme_bearer(), "tok-123")
}

fn text_of(reply: &Reply) -> String {
    match reply {
        Reply::Result(result) => result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        Reply::Error(error) => error["message"].as_str().unwrap_or_default().to_owned(),
    }
}

#[tokio::test]
async fn handshake_runs_in_order_before_the_first_request_and_again_after_a_new_session() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client();
    let target = target(&fake);

    let list = client.list_tools(&key("r1"), &target).await.unwrap();

    assert_eq!(list.tools.len(), 1);
    assert_eq!(
        fake.methods(),
        ["initialize", "notifications/initialized", "tools/list"]
    );
    let initialize = fake.requests()[0].json();
    assert_eq!(initialize["params"]["capabilities"], json!({}));
    assert_eq!(initialize["params"]["protocolVersion"], PROTOCOL_VERSION);

    // Another run has its own session and its own handshake.
    client.list_tools(&key("r2"), &target).await.unwrap();
    assert_eq!(
        fake.methods(),
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "initialize",
            "notifications/initialized",
            "tools/list"
        ]
    );
}

#[tokio::test]
async fn session_id_and_protocol_version_are_sent_after_initialize() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client();
    let target = target(&fake);
    client.list_tools(&key("r1"), &target).await.unwrap();
    client
        .call_tool(
            &key("r1"),
            &target,
            "search",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    let requests = fake.requests();
    assert_eq!(requests[0].header("mcp-session-id"), None);
    for later in &requests[1..] {
        assert_eq!(later.header("mcp-session-id"), Some("sess-1"));
        assert_eq!(later.header("mcp-protocol-version"), Some("2025-06-18"));
    }
    assert_eq!(
        fake.methods().iter().filter(|m| *m == "initialize").count(),
        1
    );
}

#[tokio::test]
async fn tool_list_follows_pages() {
    let tools: Vec<Value> = (0..5).map(|n| tool(&format!("t{n}"))).collect();
    let fake = FakeMcp::start_with(Arc::new(Standard::new(tools).pages_of(2)).handler()).await;

    let list = client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap();

    assert_eq!(list.tools.len(), 5);
    assert_eq!(
        fake.methods().iter().filter(|m| *m == "tools/list").count(),
        3
    );
}

#[tokio::test]
async fn a_list_over_1000_tools_is_refused() {
    let tools: Vec<Value> = (0..1_001).map(|n| tool(&format!("t{n}"))).collect();
    let fake = FakeMcp::start_with(Arc::new(Standard::new(tools).pages_of(400)).handler()).await;

    let error = client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::TooLarge);
}

#[tokio::test]
async fn a_list_over_4_mib_is_refused() {
    let big = json!({"name": "big", "description": "x".repeat(3 * 1024 * 1024),
        "inputSchema": {"type": "object"}});
    let fake =
        FakeMcp::start_with(Arc::new(Standard::new(vec![big.clone(), big]).pages_of(1)).handler())
            .await;

    let error = client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::TooLarge);
}

#[tokio::test(start_paused = true)]
async fn a_list_that_takes_longer_than_15_seconds_times_out() {
    let standard = Arc::new(Standard::new(vec![tool("a")]));
    let inner = standard.clone().handler();
    let fake = FakeMcp::start_with(Arc::new(move |seen: &Seen| {
        if seen.rpc_method().as_deref() == Some("tools/list") {
            Answer::Stall
        } else {
            inner(seen)
        }
    }))
    .await;

    let error = client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::TimedOut);
}

#[tokio::test]
async fn call_returns_the_json_answer_with_the_arguments_unchanged() {
    let fake = FakeMcp::start(vec![tool("search")]).await;

    let reply = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "search",
            json!({"q": "tokio", "n": 2}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(text_of(&reply), r#"called search {"n":2,"q":"tokio"}"#);
    let call = fake.requests().pop().unwrap().json();
    assert_eq!(
        call["params"],
        json!({"name": "search", "arguments": {"q": "tokio", "n": 2}})
    );
}

#[tokio::test]
async fn sse_answers_work_for_the_handshake_and_the_call() {
    let fake =
        FakeMcp::start_with(Arc::new(Standard::new(vec![tool("search")]).sse()).handler()).await;

    let reply = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "search",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(text_of(&reply), "called search {}");
}

#[tokio::test]
async fn a_stateless_upstream_gets_no_session_id() {
    let fake =
        FakeMcp::start_with(Arc::new(Standard::new(vec![tool("search")]).stateless()).handler())
            .await;

    client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap();

    assert!(
        fake.requests()
            .iter()
            .all(|seen| seen.header("mcp-session-id").is_none())
    );
}

#[tokio::test]
async fn an_upstream_error_comes_back_as_an_error_reply() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| match seen.rpc_method().as_deref() {
        Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": seen.json()["id"],
            "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
        Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": seen.json()["id"],
            "error": {"code": -32602, "message": "unknown tool"}})),
        _ => Answer::Accepted,
    }))
    .await;

    let reply = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "nope",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        reply,
        Reply::Error(json!({"code": -32602, "message": "unknown tool"}))
    );
}

#[tokio::test]
async fn upstream_requests_are_refused_and_notifications_dropped() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let message = seen.json();
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") => Answer::Sse(vec![
                json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1}}).to_string(),
                json!({"jsonrpc": "2.0", "id": "srv-1", "method": "sampling/createMessage", "params": {}}).to_string(),
                json!({"jsonrpc": "2.0", "id": message["id"], "result": {"content": [{"type": "text", "text": "done"}]}}).to_string(),
            ]),
            _ => Answer::Accepted,
        }
    }))
    .await;

    let reply = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "t",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(text_of(&reply), "done");
    let refusal = fake
        .requests()
        .into_iter()
        .map(|seen| seen.json())
        .find(|message| message["id"] == "srv-1")
        .expect("the sampling request was answered");
    assert_eq!(refusal["error"]["code"], -32601);
}

#[tokio::test]
async fn an_expired_upstream_session_is_replaced_once() {
    let standard = Arc::new(Standard::new(vec![tool("search")]));
    let fake = FakeMcp::start_with(standard.clone().handler()).await;
    let client = client();
    let target = target(&fake);
    client.list_tools(&key("r1"), &target).await.unwrap();
    standard.expire_sessions();

    let reply = client
        .call_tool(
            &key("r1"),
            &target,
            "search",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(text_of(&reply), "called search {}");
    assert_eq!(
        fake.methods()[3..],
        [
            "tools/call",
            "initialize",
            "notifications/initialized",
            "tools/call"
        ]
    );
}

#[tokio::test]
async fn a_second_404_fails_the_call() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| match seen.rpc_method().as_deref() {
        Some("initialize") => Answer::JsonWith(
            vec![("mcp-session-id", "s".into())],
            json!({"jsonrpc": "2.0", "id": seen.json()["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}}),
        ),
        Some("tools/call") => Answer::Status(404),
        _ => Answer::Accepted,
    }))
    .await;

    let error = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "t",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::Status(404));
    assert_eq!(
        fake.methods().iter().filter(|m| *m == "initialize").count(),
        2
    );
}

#[tokio::test]
async fn sweeping_ends_sessions_the_snapshot_dropped_with_delete() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client();
    let target = target(&fake);
    client.list_tools(&key("r1"), &target).await.unwrap();
    client.list_tools(&key("r2"), &target).await.unwrap();

    client
        .sweep(&|key, _target| key.run_id == "r2", Duration::from_secs(600))
        .await;

    let deletes: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|seen| seen.method == Method::DELETE)
        .collect();
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].header("mcp-session-id"), Some("sess-1"));
}

#[tokio::test]
async fn sweeping_ends_sessions_idle_for_longer_than_the_limit() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client();
    let target = target(&fake);
    client.list_tools(&key("r1"), &target).await.unwrap();
    client.sweep(&|_, _| true, Duration::from_secs(600)).await;
    assert!(!fake.methods().contains(&"DELETE".to_owned()));

    // Every session is idle for at least zero seconds.
    client.sweep(&|_, _| true, Duration::ZERO).await;

    assert!(fake.methods().contains(&"DELETE".to_owned()));
    // The next call starts a new session.
    client.list_tools(&key("r1"), &target).await.unwrap();
    assert_eq!(
        fake.methods().iter().filter(|m| *m == "initialize").count(),
        2
    );
}

#[tokio::test]
async fn a_new_config_version_ends_the_old_session_before_the_next_request() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client();
    let first = target(&fake);
    client.list_tools(&key("r1"), &first).await.unwrap();
    let mut server = pb::McpServer {
        id: "s1".into(),
        name: "wiki".into(),
        url: fake.url(),
        credential_ref: "c".into(),
        credential_scheme: Some(scheme_bearer()),
        private_network: true,
        config_version: 2,
    };
    server.config_version = 2;
    let second = Target::from_proto(
        &server,
        Some(&pb::ProviderCredential {
            value: "new".into(),
        }),
        true,
    )
    .unwrap();

    client.list_tools(&key("r1"), &second).await.unwrap();

    let methods = fake.methods();
    assert_eq!(&methods[3..5], ["DELETE", "initialize"]);
    let last = fake.requests().pop().unwrap();
    assert_eq!(last.header("authorization"), Some("Bearer new"));
}

fn sent_headers(seen: &Seen) -> Vec<String> {
    let mut names: Vec<String> = seen
        .headers
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn bearer_credential_is_the_only_credential_and_no_other_headers_are_sent() {
    let fake = FakeMcp::start(vec![tool("search")]).await;

    client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap();

    for seen in fake.requests() {
        assert_eq!(seen.header("authorization"), Some("Bearer tok-123"));
        let names = sent_headers(&seen);
        for name in &names {
            assert!(
                [
                    "accept",
                    "accept-encoding",
                    "authorization",
                    "content-length",
                    "content-type",
                    "host",
                    "mcp-protocol-version",
                    "mcp-session-id"
                ]
                .contains(&name.as_str()),
                "unexpected header {name}"
            );
        }
    }
}

#[tokio::test]
async fn header_scheme_sends_the_prefix_and_the_value_in_the_named_header() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let scheme = pb::mcp_server::CredentialScheme::Header(pb::McpHeaderScheme {
        name: "CONTEXT7_API_KEY".into(),
        prefix: "Key ".into(),
    });
    let target = target_at(&fake.url(), scheme, "abc");

    client().list_tools(&key("r1"), &target).await.unwrap();

    for seen in fake.requests() {
        assert_eq!(seen.header("context7_api_key"), Some("Key abc"));
        assert_eq!(seen.header("authorization"), None);
    }
}

#[tokio::test]
async fn query_scheme_appends_the_url_encoded_value_after_the_urls_own_parameters() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let scheme = pb::mcp_server::CredentialScheme::Query(pb::McpQueryScheme {
        parameter: "token".into(),
    });
    let target = target_at(&format!("{}?file=abc", fake.url()), scheme, "a b/c");

    client().list_tools(&key("r1"), &target).await.unwrap();

    for seen in fake.requests() {
        assert_eq!(seen.path_and_query, "/mcp?file=abc&token=a+b%2Fc");
        assert_eq!(seen.header("authorization"), None);
    }
}

fn echo_handler(echo: &'static str) -> Handler {
    Arc::new(move |seen: &Seen| {
        let message = seen.json();
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/list") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"tools": [{"name": "t", "description": format!("use {echo}"),
                    "inputSchema": {"type": "object"}}]}})),
            Some("tools/call") if message["params"]["name"] == "fail" => Answer::Json(json!({
                "jsonrpc": "2.0", "id": message["id"],
                "error": {"code": -32000, "message": format!("bad request https://h/mcp?token={echo}")}})),
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"content": [{"type": "text", "text": format!("you sent {echo}")}]}})),
            _ => Answer::Accepted,
        }
    })
}

use crate::mcp::testing::Handler;

#[tokio::test]
async fn the_credential_is_redacted_in_lists_results_and_errors_raw_and_encoded() {
    let fake = FakeMcp::start_with(echo_handler("a+b%2Fc and a b/c")).await;
    let scheme = pb::mcp_server::CredentialScheme::Query(pb::McpQueryScheme {
        parameter: "token".into(),
    });
    let target = target_at(&fake.url(), scheme, "a b/c");
    let client = client();

    let list = client.list_tools(&key("r1"), &target).await.unwrap();
    let ok = client
        .call_tool(
            &key("r1"),
            &target,
            "t",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let error = client
        .call_tool(
            &key("r1"),
            &target,
            "fail",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        list.tools[0]["description"],
        "use [redacted] and [redacted]"
    );
    assert_eq!(text_of(&ok), "you sent [redacted] and [redacted]");
    assert_eq!(
        text_of(&error),
        "bad request https://h/mcp?token=[redacted] and [redacted]"
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let fake = FakeMcp::start_with(Arc::new(|_: &Seen| Answer::Redirect("http://10.0.0.5/"))).await;

    let error = client()
        .list_tools(&key("r1"), &target(&fake))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::Redirect);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn http_errors_and_non_mcp_bodies_are_errors_without_the_credential() {
    for (answer, expected, message) in [
        (
            Answer::Status(401),
            ClientError::Status(401),
            "upstream refused the credential (401)",
        ),
        (
            Answer::Status(500),
            ClientError::Status(500),
            "upstream error (500)",
        ),
        (
            Answer::Raw("text/html", "<html>tok-123</html>".into()),
            ClientError::NotMcp,
            "upstream answered with something that is not MCP",
        ),
        (
            Answer::Raw("application/json", "not json tok-123".into()),
            ClientError::NotMcp,
            "upstream answered with something that is not MCP",
        ),
    ] {
        let fake = FakeMcp::start_with(Arc::new(move |_: &Seen| answer.clone())).await;

        let error = client()
            .list_tools(&key("r1"), &target(&fake))
            .await
            .unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(error.message(), message);
        assert!(!error.message().contains("tok-123"));
    }
}

#[tokio::test]
async fn a_result_over_4_mib_is_cut_and_reported_as_too_large() {
    let fake = FakeMcp::start_with(Arc::new(|seen: &Seen| {
        let message = seen.json();
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}})),
            Some("tools/call") => Answer::Json(json!({"jsonrpc": "2.0", "id": message["id"],
                "result": {"content": [{"type": "text", "text": "x".repeat(5 * 1024 * 1024)}]}})),
            _ => Answer::Accepted,
        }
    }))
    .await;

    let error = client()
        .call_tool(
            &key("r1"),
            &target(&fake),
            "t",
            json!({}),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::TooLarge);
}

fn public_target(host: &str, port: u16) -> Target {
    let server = pb::McpServer {
        id: "s1".into(),
        name: "docs".into(),
        url: format!("http://{host}:{port}/mcp"),
        credential_ref: "c".into(),
        credential_scheme: Some(scheme_bearer()),
        private_network: false,
        config_version: 1,
    };
    Target::from_proto(
        &server,
        Some(&pb::ProviderCredential {
            value: "tok".into(),
        }),
        true,
    )
    .unwrap()
}

#[tokio::test]
async fn a_public_server_is_refused_if_any_resolved_address_is_blocked() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let resolver = HashMap::from([(
        "docs.test".to_owned(),
        vec!["203.0.113.9".parse().unwrap(), "10.0.0.5".parse().unwrap()],
    )]);
    let client = client_with(resolver, BlockedRanges::new([]).unwrap());

    let error = client
        .list_tools(&key("r1"), &public_target("docs.test", fake.addr.port()))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::Blocked);
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn a_public_server_is_reached_at_its_checked_address_with_its_own_host_header() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let resolver = HashMap::from([("docs.test".to_owned(), vec!["127.0.0.1".parse().unwrap()])]);
    let client = client_with(resolver, BlockedRanges::none());

    client
        .list_tools(&key("r1"), &public_target("docs.test", fake.addr.port()))
        .await
        .unwrap();

    assert_eq!(
        fake.requests()[0].header("host"),
        Some(format!("docs.test:{}", fake.addr.port()).as_str())
    );
}

#[tokio::test]
async fn an_ip_literal_in_a_blocked_range_is_refused_unless_the_server_is_private() {
    let fake = FakeMcp::start(vec![tool("search")]).await;
    let client = client_with(HashMap::new(), BlockedRanges::new([]).unwrap());
    let public = public_target("127.0.0.1", fake.addr.port());
    assert_eq!(
        client.list_tools(&key("r1"), &public).await.unwrap_err(),
        ClientError::Blocked
    );

    // "Private network": no check, the name resolves by itself.
    let private = target_at(
        &format!("http://localhost:{}/mcp", fake.addr.port()),
        scheme_bearer(),
        "tok",
    );
    client.list_tools(&key("r1"), &private).await.unwrap();
}

#[tokio::test]
async fn a_host_that_does_not_resolve_is_an_error() {
    let client = client_with(HashMap::new(), BlockedRanges::none());

    let error = client
        .list_tools(&key("r1"), &public_target("nowhere.test", 1))
        .await
        .unwrap_err();

    assert_eq!(error, ClientError::Resolve);
}

#[tokio::test]
async fn cancelling_a_call_drops_the_request_and_tells_the_upstream() {
    let standard = Arc::new(Standard::new(vec![tool("slow")]));
    let inner = standard.clone().handler();
    let fake = FakeMcp::start_with(Arc::new(move |seen: &Seen| {
        if seen.rpc_method().as_deref() == Some("tools/call") {
            Answer::Stall
        } else {
            inner(seen)
        }
    }))
    .await;
    let client = Arc::new(client());
    let target = target(&fake);
    client.list_tools(&key("r1"), &target).await.unwrap();
    let cancel = CancellationToken::new();
    let call = tokio::spawn({
        let (client, target, cancel) = (client.clone(), target.clone(), cancel.clone());
        async move {
            client
                .call_tool(&key("r1"), &target, "slow", json!({}), &cancel)
                .await
        }
    });
    while !fake.methods().contains(&"tools/call".to_owned()) {
        tokio::task::yield_now().await;
    }

    cancel.cancel();

    assert_eq!(call.await.unwrap().unwrap_err(), ClientError::Cancelled);
    while !fake
        .methods()
        .contains(&"notifications/cancelled".to_owned())
    {
        tokio::task::yield_now().await;
    }
}
