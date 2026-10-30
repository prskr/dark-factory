//! Host icons.

use std::sync::Arc;

use axum::http::StatusCode;
use df_proto::gateway::v1::FlowEvent;

use super::{FakeFlows, FakeServices, add_host, allowed, denied, live, part};
use crate::egress::log::Filter;
use crate::testkit::{SseReader, TestApp, form, text};

#[test]
fn an_ip_literal_destination_shows_the_server_icon() {
    let body = live(Filter::default(), &[denied("203.0.113.42", "DF", 1)]).unwrap();
    assert!(
        part(&body, "#flows tbody")
            .unwrap()
            .contains("data-host-icon=\"server\"")
    );
    assert!(
        part(&body, "#graph")
            .unwrap()
            .contains("data-host-icon=\"server\"")
    );
}

async fn forgejo_app(events: Vec<FlowEvent>) -> TestApp {
    TestApp::with_egress(
        crate::testkit::default_forge("forgejo"),
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await
}

/// Creates a project through the form; `credential` is a forge credential reference.
async fn add_project(
    app: &TestApp,
    cookie: &str,
    key: &str,
    repo: &str,
    issue: &str,
    credential: &str,
) {
    let body = form(&[
        ("key", key),
        ("name", key),
        ("repo_url", repo),
        ("issue_tracker_url", issue),
        ("image", "ghcr.io/example/sandbox:1"),
        ("base_branch", "main"),
        ("forge_credential_id", credential),
    ]);
    let response = app.post("/projects", &body, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{key}");
}

/// Creates a forge credential reference and returns its id.
async fn add_credential(app: &TestApp, cookie: &str) -> String {
    let body = form(&[
        ("name", "forge"),
        ("secret_name", "forge"),
        ("secret_key", "t"),
    ]);
    let response = app.post("/credentials", &body, cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .to_string()
}

const ICON_KEY: &str = "data-host-icon=\"";

/// The attribute value that starts after the `data-host-icon="` found at `key_at` in `html`.
fn icon_value(html: &str, key_at: usize) -> String {
    let from = key_at + ICON_KEY.len();
    let len = html[from..].find('"').expect("closing quote");
    html[from..from + len].to_string()
}

/// The `data-host-icon` of the last element before `marker` in `html`.
fn icon_before(html: &str, marker: &str) -> String {
    let at = html.find(marker).expect("marker in html");
    icon_value(
        html,
        html[..at].rfind(ICON_KEY).expect("icon before marker"),
    )
}

/// The `data-host-icon` of the first element after `marker` in `html`.
fn icon_after(html: &str, marker: &str) -> String {
    let at = html.find(marker).expect("marker in html");
    icon_value(
        html,
        at + html[at..].find(ICON_KEY).expect("icon after marker"),
    )
}

/// The events table row of `host:443`.
fn row_of<'a>(body: &'a str, host: &str) -> &'a str {
    part(body, "#flows tbody")
        .expect("rows")
        .split("<tr ")
        .find(|row| row.contains(&format!(">{host}:443<")))
        .expect("row of host")
}

/// The icon of the events table row of `host:443`.
fn row_icon(body: &str, host: &str) -> String {
    icon_after(row_of(body, host), "<td")
}

/// The `data-flow` JSON of the row of `host:443`, as the details panel reads it.
fn flow_of(body: &str, host: &str) -> serde_json::Value {
    let row = row_of(body, host);
    let start = row.find("data-flow=\"").expect("data-flow") + "data-flow=\"".len();
    let json = row[start..start + row[start..].find('"').expect("end")]
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    serde_json::from_str(&json).expect("flow json")
}

async fn events_frame(app: &TestApp, cookie: &str) -> String {
    SseReader::new(app.get("/egress/log/events", cookie).await)
        .next()
        .await
        .data
}

#[tokio::test]
async fn project_hosts_show_their_icons_in_rows_graph_and_details() {
    let hosts = [
        "git.example.internal",
        "code.example.org",
        "jira.example.com",
        "api.git.example.internal",
        "ops.example.internal",
    ];
    let events = hosts
        .iter()
        .zip(1..)
        .map(|(host, second)| allowed(host, "DF", second))
        .collect();
    let app = forgejo_app(events).await;
    let cookie = app.login().await;
    let credential = add_credential(&app, &cookie).await;
    add_project(
        &app,
        &cookie,
        "DF",
        "https://git.example.internal/org/df.git",
        "https://jira.example.com/browse/DF",
        &credential,
    )
    .await;
    // No forge credential, tracker on the repository host.
    add_project(
        &app,
        &cookie,
        "WEB",
        "https://code.example.org/web.git",
        "https://code.example.org/web/issues",
        "",
    )
    .await;
    // Served project with the tracker on the repository host: the forge icon, not the issue icon.
    add_project(
        &app,
        &cookie,
        "OPS",
        "https://ops.example.internal/org/ops.git",
        "https://ops.example.internal/org/ops/issues",
        &credential,
    )
    .await;

    let body = events_frame(&app, &cookie).await;
    for (host, icon) in [
        ("git.example.internal", "forge-forgejo"),
        ("code.example.org", "forge-git"),
        ("jira.example.com", "issue"),
        ("api.git.example.internal", "globe"),
        ("ops.example.internal", "forge-forgejo"),
    ] {
        assert_eq!(row_icon(&body, host), icon, "row {host}");
        let graph = part(&body, "#graph").expect("graph");
        assert_eq!(
            icon_before(graph, &format!("title=\"{host}\"")),
            icon,
            "graph {host}"
        );
    }
    // The details panel reads its icon from the row's JSON.
    let flow = flow_of(&body, "git.example.internal");
    let panel = flow["icon"].as_str().expect("icon");
    assert!(
        panel.contains("data-host-icon=\"forge-forgejo\""),
        "{panel}"
    );
}

#[tokio::test]
async fn a_project_host_beats_the_provider_monogram_on_a_github_instance() {
    let events = vec![
        allowed("github.com", "DF", 1),
        allowed("api.github.com", "DF", 2),
    ];
    let app = TestApp::with_egress(
        crate::testkit::default_forge("github"),
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await;
    let cookie = app.login().await;
    let credential = add_credential(&app, &cookie).await;
    add_project(
        &app,
        &cookie,
        "DF",
        "https://github.com/org/df.git",
        "",
        &credential,
    )
    .await;
    let body = events_frame(&app, &cookie).await;
    assert_eq!(row_icon(&body, "github.com"), "forge-github");
    assert_eq!(row_icon(&body, "api.github.com"), "GH");
}

#[tokio::test]
async fn a_host_claimed_by_a_plugin_that_did_not_load_shows_the_git_icon() {
    let events = vec![
        allowed("github.com", "GH", 1),
        allowed("git.example.internal", "DF", 2),
    ];
    let app = TestApp::with_egress(
        crate::testkit::forge_routes(
            &[("github", &["github.com"]), ("forgejo", &[])],
            &["forgejo"],
        ),
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await;
    let cookie = app.login().await;
    let credential = add_credential(&app, &cookie).await;
    add_project(
        &app,
        &cookie,
        "GH",
        "https://github.com/org/gh.git",
        "",
        &credential,
    )
    .await;
    add_project(
        &app,
        &cookie,
        "DF",
        "https://git.example.internal/org/df.git",
        "",
        &credential,
    )
    .await;
    let body = events_frame(&app, &cookie).await;
    assert_eq!(row_icon(&body, "github.com"), "forge-git");
    assert_eq!(row_icon(&body, "git.example.internal"), "forge-forgejo");
}

#[tokio::test]
async fn a_project_created_while_the_page_is_open_shows_after_a_reconnect() {
    let app = forgejo_app(vec![allowed("git.example.internal", "DF", 1)]).await;
    let cookie = app.login().await;
    let first = SseReader::new(app.get("/egress/log/events", &cookie).await)
        .next()
        .await;
    assert_eq!(row_icon(&first.data, "git.example.internal"), "globe");

    let credential = add_credential(&app, &cookie).await;
    add_project(
        &app,
        &cookie,
        "DF",
        "https://git.example.internal/org/df.git",
        "",
        &credential,
    )
    .await;
    let second = SseReader::new(app.get("/egress/log/events", &cookie).await)
        .next()
        .await;
    assert_eq!(
        row_icon(&second.data, "git.example.internal"),
        "forge-forgejo"
    );
}

#[tokio::test]
async fn the_rules_page_shows_project_hosts_before_wildcards_and_brands() {
    let app = forgejo_app(vec![]).await;
    let cookie = app.login().await;
    let credential = add_credential(&app, &cookie).await;
    add_project(
        &app,
        &cookie,
        "DF",
        "https://git.example.internal/org/df.git",
        "",
        &credential,
    )
    .await;
    add_host(&app, &cookie, "", "git.example.internal", "443").await;
    add_host(&app, &cookie, "", "*.git.example.internal", "443").await;
    add_host(&app, &cookie, "", "pypi.org", "443").await;
    let page = text(app.get("/egress/allowlist", &cookie).await).await;
    let icon_of = |host: &str| icon_before(&page, &format!(">{host}<"));
    assert_eq!(icon_of("git.example.internal"), "forge-forgejo");
    assert_eq!(icon_of("*.git.example.internal"), "globe");
    assert_eq!(icon_of("pypi.org"), "Py");
}
