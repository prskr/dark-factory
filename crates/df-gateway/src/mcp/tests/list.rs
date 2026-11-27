//! The aggregated `tools/list`.

use super::*;
use crate::mcp::testing::{Answer, FakeMcp, Seen, Standard, tool};

fn lists(fake: &FakeMcp) -> usize {
    fake.methods().iter().filter(|m| *m == "tools/list").count()
}

#[tokio::test]
async fn two_servers_are_listed_with_prefixes_and_only_the_five_served_fields() {
    let rich = json!({"name": "get-docs", "title": "Docs", "description": "d", "inputSchema": {"type": "object"},
        "outputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true},
        "_meta": {"x": 1}, "icons": [{"src": "i"}]});
    let context7 = FakeMcp::start(vec![rich]).await;
    let wiki = FakeMcp::start(vec![tool("search")]).await;
    let servers = [
        server("s1", "context7", &context7.url(), 1),
        server("s2", "wiki", &wiki.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1", "s2"]))).await;
    let session = env.session().await;

    let answer = env.rpc(&session, "tools/list", json!({})).await;

    let tools = answer["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["context7__get-docs", "wiki__search"]
    );
    let mut fields: Vec<_> = tools[0].as_object().unwrap().keys().cloned().collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "annotations",
            "description",
            "inputSchema",
            "name",
            "outputSchema",
            "title"
        ]
    );
    assert!(env.records.all().is_empty());
}

#[tokio::test]
async fn invalid_names_too_long_names_and_duplicates_are_left_out() {
    let tools = vec![
        tool("search"),
        tool("search"),
        tool("has space"),
        tool(&"x".repeat(46)),
        tool("ok"),
    ];
    let fake = FakeMcp::start(tools).await;
    let env = Env::start(Some(policy(
        &[server("s1", "forgejo", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;

    assert_eq!(
        env.tool_names(&session).await,
        ["forgejo__search", "forgejo__ok"]
    );
}

#[tokio::test]
async fn a_transient_failure_is_retried_once() {
    let standard = Arc::new(Standard::new(vec![tool("a")]));
    let inner = standard.handler();
    let failed = Arc::new(Mutex::new(false));
    let fake = FakeMcp::start_with(Arc::new(move |seen: &Seen| {
        let first = seen.rpc_method().as_deref() == Some("tools/list")
            && !std::mem::replace(&mut *failed.lock().unwrap(), true);
        if first {
            Answer::Status(503)
        } else {
            inner(seen)
        }
    }))
    .await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;

    assert_eq!(env.tool_names(&session).await, ["wiki__a"]);
    assert!(env.records.all().is_empty());
}

#[tokio::test]
async fn a_server_that_fails_twice_is_left_out_with_a_list_record_and_the_others_stay() {
    let good = FakeMcp::start(vec![tool("a")]).await;
    let bad = FakeMcp::start_with(Arc::new(|_: &Seen| Answer::Status(500))).await;
    let servers = [
        server("s1", "context7", &good.url(), 1),
        server("s2", "wiki", &bad.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1", "s2"]))).await;
    let session = env.session().await;

    assert_eq!(env.tool_names(&session).await, ["context7__a"]);

    env.records.wait_for(1).await;
    assert_eq!(
        env.records.summary(),
        [(
            "wiki".to_owned(),
            "tools/list".to_owned(),
            df_core::mcp::McpCallOutcome::UpstreamError
        )]
    );
}

#[tokio::test]
async fn a_slow_server_is_left_out_with_a_timeout_record_and_the_others_answer() {
    let good = FakeMcp::start(vec![tool("a")]).await;
    let slow = FakeMcp::start_with(Arc::new(|_: &Seen| Answer::Stall)).await;
    let servers = [
        server("s1", "context7", &good.url(), 1),
        server("s2", "wiki", &slow.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1", "s2"]))).await;
    let session = env.session().await;

    assert_eq!(env.tool_names(&session).await, ["context7__a"]);

    env.records.wait_for(1).await;
    assert_eq!(
        env.records.summary(),
        [(
            "wiki".to_owned(),
            "tools/list".to_owned(),
            df_core::mcp::McpCallOutcome::Timeout
        )]
    );
}

#[tokio::test]
async fn a_server_with_too_many_tools_is_left_out_as_too_large() {
    let tools: Vec<Value> = (0..1_500).map(|n| tool(&format!("t{n}"))).collect();
    let fake = FakeMcp::start_with(Arc::new(Standard::new(tools).pages_of(500)).handler()).await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;

    assert!(env.tool_names(&session).await.is_empty());

    env.records.wait_for(1).await;
    assert_eq!(
        env.records.summary()[0].2,
        df_core::mcp::McpCallOutcome::TooLarge
    );
}

#[tokio::test]
async fn a_public_server_resolving_to_a_private_address_is_left_out_without_a_connection() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let mut docs = server(
        "s1",
        "docs",
        &format!("http://docs.test:{}/mcp", fake.addr.port()),
        1,
    );
    docs.private_network = false;
    let hosts = HashMap::from([("docs.test".to_owned(), vec!["10.0.0.5".parse().unwrap()])]);
    let env = Env::start_with(
        Some(policy(&[docs], &["s1"])),
        hosts,
        BlockedRanges::new([]).unwrap(),
    )
    .await;
    let session = env.session().await;

    assert!(env.tool_names(&session).await.is_empty());

    env.records.wait_for(1).await;
    assert_eq!(
        env.records.summary()[0].2,
        df_core::mcp::McpCallOutcome::UpstreamError
    );
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn repeated_listing_within_the_cache_window_reaches_the_upstream_once() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let env = Env::start(Some(policy(
        &[server("s1", "wiki", &fake.url(), 1)],
        &["s1"],
    )))
    .await;
    let session = env.session().await;

    env.tool_names(&session).await;
    env.tool_names(&session).await;

    assert_eq!(lists(&fake), 1);
}

#[tokio::test]
async fn the_cache_is_dropped_when_a_server_is_disabled_or_edited() {
    let context7 = FakeMcp::start(vec![tool("a")]).await;
    let wiki = FakeMcp::start(vec![tool("b")]).await;
    let servers = [
        server("s1", "context7", &context7.url(), 1),
        server("s2", "wiki", &wiki.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1", "s2"]))).await;
    let session = env.session().await;
    assert_eq!(env.tool_names(&session).await, ["context7__a", "wiki__b"]);

    env.set_policy(policy(&servers, &["s2"]));
    assert_eq!(env.tool_names(&session).await, ["wiki__b"]);

    let edited = [
        server("s1", "context7", &context7.url(), 1),
        server("s2", "wiki", &wiki.url(), 2),
    ];
    env.set_policy(policy(&edited, &["s2"]));
    env.tool_names(&session).await;
    assert_eq!(lists(&wiki), 3, "listed again after each change");
}

#[tokio::test]
async fn a_server_enabled_during_the_run_appears_in_the_next_list() {
    let fake = FakeMcp::start(vec![tool("a")]).await;
    let servers = [server("s1", "wiki", &fake.url(), 1)];
    let env = Env::start(Some(policy(&servers, &[]))).await;
    let session = env.session().await;
    assert!(env.tool_names(&session).await.is_empty());

    env.set_policy(policy(&servers, &["s1"]));

    assert_eq!(env.tool_names(&session).await, ["wiki__a"]);
}

#[tokio::test]
async fn servers_are_asked_in_parallel() {
    let slow = |tools| async move {
        let standard = Arc::new(Standard::new(tools));
        let inner = standard.handler();
        FakeMcp::start_with(Arc::new(move |seen: &Seen| {
            if seen.rpc_method().as_deref() == Some("tools/list") {
                Answer::After(Duration::from_millis(400), Box::new(inner(seen)))
            } else {
                inner(seen)
            }
        }))
        .await
    };
    let (a, b) = (slow(vec![tool("a")]).await, slow(vec![tool("b")]).await);
    let servers = [
        server("s1", "one", &a.url(), 1),
        server("s2", "two", &b.url(), 1),
    ];
    let env = Env::start(Some(policy(&servers, &["s1", "s2"]))).await;
    let session = env.session().await;

    let started = std::time::Instant::now();
    assert_eq!(env.tool_names(&session).await, ["one__a", "two__b"]);

    assert!(
        started.elapsed() < Duration::from_millis(750),
        "{:?}",
        started.elapsed()
    );
}
