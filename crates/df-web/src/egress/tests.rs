use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use df_db::{EntryKind, Notification};
use df_proto::gateway::v1::{FlowEvent, Phase, Verdict};
use df_proto::pbjson_types::Timestamp;
use futures::StreamExt;
use futures::stream::{self, BoxStream};

use super::log::{Filter, VerdictFilter, Viewer};
use super::{FlowSource, LookupError, ServiceLookup};
use crate::testkit::{SseReader, TestApp, form, header_str, seed, text};

// ---------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------

/// Replays `events` to every viewer and then stays open.
struct FakeFlows(Vec<FlowEvent>);

impl FlowSource for FakeFlows {
    fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let backlog_end = FlowEvent {
            backlog_end: true,
            ..Default::default()
        };
        stream::iter(self.0.clone())
            .chain(stream::iter([backlog_end]))
            .chain(stream::pending())
            .boxed()
    }
}

/// Knows exactly one Service: `nativelink/scheduler`.
struct FakeServices;

#[async_trait]
impl ServiceLookup for FakeServices {
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError> {
        Ok((namespace, service) == ("nativelink", "scheduler"))
    }
}

fn denied(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        time: Some(Timestamp {
            seconds: second,
            nanos: 0,
        }),
        host: host.into(),
        port: 443,
        method: "CONNECT".into(),
        project_key: project.into(),
        task_key: format!("{project}-1"),
        verdict: Verdict::Denied.into(),
        reason: "not-allowlisted".into(),
        ..Default::default()
    }
}

fn allowed(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        verdict: Verdict::Allowed.into(),
        reason: String::new(),
        ..denied(host, project, second)
    }
}

async fn app_with_egress(events: Vec<FlowEvent>) -> TestApp {
    TestApp::with_egress(
        None,
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await
}

// ---------------------------------------------------------------------------------------------
// Allowlist
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_instance_entry_applies_to_every_project_and_is_listed() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", ""), ("host", "API.Anthropic.com"), ("port", "")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    assert_eq!(header_str(&response, "hx-trigger"), "modal-close");
    let html = text(response).await;
    assert!(
        html.contains(">api.anthropic.com<") && html.contains(">443<") && html.contains("Instance"),
        "{html}"
    );

    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project_key, None);
    assert_eq!(
        entries[0].kind,
        EntryKind::HttpHost {
            host: "api.anthropic.com".into(),
            port: 443
        }
    );

    let page = text(app.get("/egress/allowlist", &cookie).await).await;
    assert!(page.contains(">api.anthropic.com<"), "{page}");
}

#[tokio::test]
async fn a_project_entry_is_stored_for_that_project_only() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", "DF"), ("host", "*.crates.io"), ("port", "443")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries[0].project_key.as_deref(), Some("DF"));
    assert!(text(response).await.contains(">*.crates.io<"));

    let unknown = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", "NOPE"), ("host", "pypi.org")]),
            &cookie,
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(unknown).await.contains("Unknown project."));
}

#[tokio::test]
async fn an_ip_literal_shows_a_validation_error_and_stores_nothing() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for ip in ["10.0.0.5", "[::1]"] {
        let response = app
            .post(
                "/egress/allowlist/host",
                &form(&[("project", ""), ("host", ip), ("port", "")]),
                &cookie,
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = text(response).await;
        assert!(html.contains("IP addresses are not allowed"), "{html}");
        assert!(html.contains(&format!("value=\"{ip}\"")), "{html}");
    }
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", ""), ("host", "pypi.org"), ("port", "99999")]),
            &cookie,
        )
        .await;
    assert!(text(response).await.contains("port between 1 and 65535"));
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_duplicate_entry_is_a_form_error() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let body = form(&[("project", ""), ("host", "pypi.org")]);
    assert_eq!(
        app.post("/egress/allowlist/host", &body, &cookie)
            .await
            .status(),
        StatusCode::OK
    );
    let again = app.post("/egress/allowlist/host", &body, &cookie).await;
    assert_eq!(again.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(again).await.contains("already exists"));
}

#[tokio::test]
async fn a_cluster_service_must_exist_when_it_is_saved() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let unknown = app
        .post(
            "/egress/allowlist/service",
            &form(&[
                ("project", ""),
                ("namespace", "nativelink"),
                ("service", "schedular"),
                ("ports", "50051"),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(unknown)
            .await
            .contains("nativelink/schedular does not exist")
    );
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());

    let known = app
        .post(
            "/egress/allowlist/service",
            &form(&[
                ("project", ""),
                ("namespace", "nativelink"),
                ("service", "scheduler"),
                ("ports", "50051, 50052"),
                ("allow_dns", "on"),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(known.status(), StatusCode::OK);
    let html = text(known).await;
    assert!(
        html.contains("nativelink/scheduler")
            && html.contains(">50051, 50052, DNS allowed<")
            && html.contains(">cluster service<"),
        "{html}"
    );
}

#[tokio::test]
async fn without_a_cluster_connection_services_cannot_be_added() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/egress/allowlist/service",
            &form(&[("namespace", "a"), ("service", "b")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("Kubernetes API is not available")
    );
}

/// A cluster that cannot be asked.
struct BrokenServices;

#[async_trait]
impl ServiceLookup for BrokenServices {
    async fn exists(&self, _: &str, _: &str) -> Result<bool, LookupError> {
        Err(LookupError::NamespaceNotAllowed("other".into()))
    }
}

#[tokio::test]
async fn a_failing_service_lookup_is_shown_to_the_board_member() {
    let app = TestApp::with_egress(None, None, Some(Arc::new(BrokenServices))).await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/egress/allowlist/service",
            &form(&[("namespace", "other"), ("service", "db")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("could not be checked") && html.contains("cluster target namespaces"),
        "{html}"
    );
}

#[tokio::test]
async fn removing_an_entry_is_audited_and_announced() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    app.post(
        "/egress/allowlist/host",
        &form(&[("project", "DF"), ("host", "pypi.org")]),
        &cookie,
    )
    .await;
    let id = df_db::list_entries(&app.pool).await.unwrap()[0].id;
    let notifier = df_db::Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let response = app
        .delete(&format!("/egress/allowlist/{id}"), &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!text(response).await.contains("pypi.org"));

    let announced = tokio::time::timeout(Duration::from_secs(5), notifications.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(announced, Notification::EgressPolicy);
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_log WHERE resource_type = 'egress_allowlist' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        actions,
        ["egress_allowlist.create", "egress_allowlist.delete"]
    );

    let again = app
        .delete(&format!("/egress/allowlist/{id}"), &cookie)
        .await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forms_are_modal_fragments_and_state_changes_need_htmx() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let plain = app.get("/egress/allowlist/host/new", &cookie).await;
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
    let modal = text(app.get_htmx("/egress/allowlist/host/new", &cookie).await).await;
    assert!(
        modal.contains("<dialog")
            && modal.contains("name=\"host\"")
            && modal.contains("name=\"project\"")
    );
    let service = text(app.get_htmx("/egress/allowlist/service/new", &cookie).await).await;
    assert!(service.contains("name=\"namespace\"") && service.contains("name=\"allow_dns\""));
}

/// Adds an `http-host` entry through the form.
async fn add_host(app: &TestApp, cookie: &str, project: &str, host: &str, port: &str) {
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", project), ("host", host), ("port", port)]),
            cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{host}");
}

#[tokio::test]
async fn the_rules_page_shows_title_count_and_header_actions() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "", "pypi.org", "443").await;
    let page = text(app.get("/egress/allowlist", &cookie).await).await;
    assert!(
        page.contains("<title>Egress rules - Dark Factory</title>"),
        "{page}"
    );
    assert!(
        page.contains("Egress rules <span class=\"font-normal text-faint\">2</span>"),
        "{page}"
    );
    assert!(page.contains("<a href=\"/egress/log\"") && page.contains(">Live traffic<"));
    assert!(
        page.contains("hx-get=\"/egress/allowlist/host/new\"") && page.contains("New egress rule")
    );
    assert!(
        page.contains("hx-get=\"/egress/allowlist/service/new\"")
            && page.contains("Add cluster service")
    );
    assert!(page.contains("aria-label=\"Egress rules\""), "rail entry");
}

#[tokio::test]
async fn rule_rows_show_icon_port_scope_and_a_named_delete_button() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "DF", "ghcr.io", "443").await;
    app.post(
        "/egress/allowlist/service",
        &form(&[
            ("namespace", "nativelink"),
            ("service", "scheduler"),
            ("ports", "8980"),
        ]),
        &cookie,
    )
    .await;
    let page = text(app.get("/egress/allowlist", &cookie).await).await;
    let github = &page[page.find(">github.com<").unwrap()..];
    let github = &github[..github.find("</tr>").unwrap()];
    assert!(
        github.contains(">443<") && github.contains("Instance"),
        "{github}"
    );
    let github_row = &page[..page.find(">github.com<").unwrap()];
    assert!(github_row.rfind("data-host-icon=\"GH\"").is_some());
    let ghcr = &page[page.find(">ghcr.io<").unwrap() - 600..page.find(">ghcr.io<").unwrap()];
    assert!(ghcr.contains("data-host-icon=\"globe\""), "{ghcr}");
    assert!(page.contains(">DF<"), "project scope");
    assert!(page.contains(">nativelink/scheduler<"), "{page}");
    assert!(
        page.contains(">cluster service<") && page.contains(">8980<"),
        "{page}"
    );
    assert!(
        page.contains("aria-label=\"Delete egress rule github.com:443\""),
        "{page}"
    );
    assert!(
        page.contains("aria-label=\"Delete egress rule nativelink/scheduler\""),
        "{page}"
    );
}

#[tokio::test]
async fn an_empty_rules_page_explains_and_offers_both_actions() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/allowlist", &cookie).await).await;
    assert!(
        page.contains("No egress rules yet") && !page.contains("<table"),
        "{page}"
    );
    assert!(page.contains("Blocked attempts appear in the live traffic view"));
    assert!(page.contains(">Open live traffic<"));
    assert_eq!(
        page.matches("New egress rule").count(),
        2,
        "header and empty state"
    );
}

#[tokio::test]
async fn highlight_marks_every_row_of_the_target_and_scrolls_to_the_first() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    add_host(&app, &cookie, "", "pypi.org", "443").await;
    let page = text(
        app.get("/egress/allowlist?highlight=github.com:443", &cookie)
            .await,
    )
    .await;
    assert_eq!(page.matches("aria-current=\"true\"").count(), 2, "{page}");
    assert_eq!(
        page.matches("scrollIntoView").count(),
        1,
        "only the first row scrolls"
    );
    assert_eq!(page.matches("bg-accent-soft").count(), 2);

    for none in [
        "?highlight=nowhere.org:443",
        "?highlight=",
        "?highlight=github.com:80",
    ] {
        let response = app.get(&format!("/egress/allowlist{none}"), &cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        let page = text(response).await;
        assert!(
            !page.contains("aria-current=\"true\"") && !page.contains("scrollIntoView"),
            "{none}"
        );
    }
}

#[tokio::test]
async fn the_rule_link_of_an_event_highlights_its_rule_on_the_rules_page() {
    // Scenario "Link to the matched rule".
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "*.crates.io", "443").await;
    let mut event = allowed("index.crates.io", "DF", 1);
    event.matched_rule = "*.crates.io".into();
    let row = Viewer::new(Filter::default()).backlog_message(&[event]);
    let href = row
        .split("href=\"")
        .map(|p| &p[..p.find('"').unwrap_or(0)])
        .find(|h| h.starts_with("/egress/allowlist?"))
        .unwrap();
    let page = text(app.get(href, &cookie).await).await;
    assert_eq!(
        page.matches("aria-current=\"true\"").count(),
        1,
        "{href}: {page}"
    );
}

// ---------------------------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------------------------

/// The content of the `<hx-partial>` for `target` in a message; `None` if the message has none.
fn part<'a>(body: &'a str, target: &str) -> Option<&'a str> {
    let open = format!("<hx-partial hx-target=\"{target}\"");
    let at = body.find(&open)?;
    let content = &body[at + body[at..].find('>')? + 1..];
    Some(&content[..content.find("</hx-partial>")?])
}

fn live(filter: Filter, batch: &[FlowEvent]) -> Option<String> {
    Viewer::new(filter).live_message(batch)
}

#[test]
fn new_rows_come_newest_first_and_the_graph_marks_the_blocked_destination() {
    let batch = [
        denied("statsig.anthropic.com", "DF", 1),
        allowed("api.anthropic.com", "DF", 2),
        denied("statsig.anthropic.com", "DF", 3),
        denied("evil.example.com", "DF", 5),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert!(body.contains("afterbegin"));
    assert!(
        rows.find("evil.example.com").unwrap() < rows.find("api.anthropic.com").unwrap(),
        "{rows}"
    );
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(graph.matches("data-blocked=\"true\"").count(), 2, "{graph}");
    assert!(graph.contains(">statsig.anthropic.com<") && graph.contains("class=\"fill-danger\""));
    assert!(!body.contains("denied-hosts"), "the summary is gone");
}

#[test]
fn a_repeatedly_denied_domain_is_a_blocked_destination_and_a_blocked_row() {
    // Scenario "Missing domain".
    let events: Vec<_> = (1..=3)
        .map(|i| denied("statsig.anthropic.com", "DF", i))
        .collect();
    let mut viewer = Viewer::new(Filter {
        verdict: Some(VerdictFilter::Blocked),
        ..Default::default()
    });
    let body = viewer.backlog_message(&events);
    assert_eq!(
        part(&body, "#flows tbody")
            .unwrap()
            .matches("data-outcome=\"blocked\"")
            .count(),
        3
    );
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(
        graph.matches("data-blocked=\"true\"").count(),
        1,
        "one destination: {graph}"
    );
    assert_eq!(part(&body, "#count-blocked"), Some("3"));
}

#[test]
fn segment_counts_ignore_the_verdict_filter() {
    // Scenario "Segment counts ignore the verdict": pypi, 3 blocked and 12 allowed.
    let mut events: Vec<_> = (0..3).map(|i| denied("pypi.org", "DF", i)).collect();
    events.extend((0..12).map(|i| allowed("pypi.org", "DF", 10 + i)));
    events.push(allowed("github.com", "DF", 30));
    for verdict in [
        None,
        Some(VerdictFilter::Blocked),
        Some(VerdictFilter::Allowed),
    ] {
        let filter = Filter {
            q: "pypi".into(),
            verdict,
            ..Default::default()
        };
        let body = Viewer::new(filter).backlog_message(&events);
        assert_eq!(part(&body, "#count-all"), Some("15"), "{verdict:?}");
        assert_eq!(part(&body, "#count-blocked"), Some("3"), "{verdict:?}");
        assert_eq!(part(&body, "#count-allowed"), Some("12"), "{verdict:?}");
    }
    let body = Viewer::new(Filter {
        q: "pypi".into(),
        verdict: Some(VerdictFilter::Blocked),
        ..Default::default()
    })
    .backlog_message(&events);
    assert_eq!(part(&body, "#event-count"), Some("3 of 16"));
}

#[test]
fn filters_limit_rows_but_not_the_graph() {
    let batch = [
        denied("a.example.com", "DF", 1),
        allowed("b.example.com", "DF", 2),
        denied("c.example.com", "WEB", 3),
    ];
    let filter = Filter {
        denied: "1".into(),
        task: "DF-1".into(),
        ..Default::default()
    };
    let body = live(filter, &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert!(rows.contains("a.example.com"));
    assert!(
        !rows.contains("b.example.com") && !rows.contains("c.example.com"),
        "{rows}"
    );
    // Scenario "Filtered out": the others are in the graph, dimmed.
    let graph = part(&body, "#graph").unwrap();
    assert!(graph.contains(">b.example.com<") && graph.contains(">c.example.com<"));
    assert_eq!(
        graph.matches("opacity-30").count(),
        4,
        "2 nodes and 2 edges of the other two: {graph}"
    );
    assert_eq!(part(&body, "#event-count"), Some("1 of 3"));

    // Nothing passes: no row, but the counts and the graph still follow.
    let nothing = Filter {
        q: "nope".into(),
        ..Default::default()
    };
    let body = live(nothing, &batch).unwrap();
    assert!(part(&body, "#flows tbody").is_none());
    assert_eq!(part(&body, "#event-count"), Some("0 of 3"));
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("No events match these filters")
    );
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("Clear filters")
    );
}

#[test]
fn a_gap_is_a_row_that_changes_neither_graph_nor_counts() {
    let gap = FlowEvent {
        gap: true,
        ..Default::default()
    };
    let mut viewer = Viewer::new(Filter {
        q: "never".into(),
        ..Default::default()
    });
    let body = viewer.backlog_message(&[]);
    assert_eq!(part(&body, "#event-count"), Some("0 of 0"));
    let message = viewer.live_message(&[gap]).unwrap();
    assert!(message.contains("Events are missing"), "{message}");
    assert!(
        part(&message, "#graph").is_none() && part(&message, "#event-count").is_none(),
        "{message}"
    );
}

#[test]
fn the_filter_query_carries_only_set_fields_and_maps_old_parameters() {
    assert_eq!(Filter::default().query(), "");
    // Old link: `denied=1` is Blocked, `host` the search; the dropped `project` and `sandbox`
    // filters are ignored, so the link still opens.
    let old: Filter = filter_from_query("denied=1&project=DF&sandbox=a4c2&host=a+b");
    assert_eq!(old.query(), "?verdict=blocked&q=a+b");
    assert_eq!(old.verdict_filter().param(), "blocked");
    assert_eq!(old.advanced_count(), 0);
    // Shared link.
    let shared: Filter = filter_from_query("verdict=blocked&q=pypi&task=DF-7");
    assert_eq!(shared.query(), "?verdict=blocked&q=pypi&task=DF-7");
    assert_eq!(shared.advanced_count(), 1);
    // `all` and unknown values are the default.
    assert_eq!(filter_from_query("verdict=all").query(), "");
    assert_eq!(filter_from_query("verdict=bogus").query(), "");
    assert_eq!(filter_from_query("q=a&host=b").search(), "a");
}

/// Parses a query string like the `Query` extractor does.
fn filter_from_query(query: &str) -> Filter {
    let uri: axum::http::Uri = format!("/x?{query}").parse().unwrap();
    axum::extract::Query::<Filter>::try_from_uri(&uri)
        .unwrap()
        .0
}

#[tokio::test]
async fn a_repeatedly_denied_domain_appears_in_the_stream() {
    let events = vec![
        denied("statsig.anthropic.com", "DF", 1_700_000_001),
        denied("statsig.anthropic.com", "DF", 1_700_000_002),
        denied("statsig.anthropic.com", "DF", 1_700_000_003),
    ];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let response = app
        .get("/egress/log/events?verdict=blocked&task=DF-1", &cookie)
        .await;
    assert_eq!(header_str(&response, "content-type"), "text/event-stream");
    let frame = SseReader::new(response).next().await;
    let rows = part(&frame.data, "#flows tbody").unwrap();
    assert_eq!(
        rows.matches("data-outcome=\"blocked\"").count(),
        3,
        "{rows}"
    );
    assert!(
        part(&frame.data, "#graph")
            .unwrap()
            .contains("data-blocked=\"true\"")
    );
    assert_eq!(part(&frame.data, "#event-count"), Some("3 of 3"));
}

#[tokio::test]
async fn the_stream_is_unfiltered_so_the_graph_sees_everything() {
    let events = vec![denied("bad.com", "DF", 1), allowed("good.com", "WEB", 2)];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let frame = SseReader::new(app.get("/egress/log/events?task=DF-1", &cookie).await)
        .next()
        .await;
    assert!(part(&frame.data, "#graph").unwrap().contains(">good.com<"));
    assert!(
        !part(&frame.data, "#flows tbody")
            .unwrap()
            .contains("good.com")
    );
}

#[tokio::test]
async fn the_log_page_has_header_filter_bar_and_the_stream_with_its_filters() {
    let app = app_with_egress(vec![]).await;
    let persona = seed(&app.pool).await.persona;
    let cookie = app.login().await;
    let page = text(
        app.get(
            &format!("/egress/log?denied=1&task=DF-7&persona={persona}"),
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        page.contains("<title>Network traffic - Dark Factory</title>"),
        "{page}"
    );
    assert!(
        page.contains(
            &format!("hx-sse:connect=\"/egress/log/events?verdict=blocked&amp;task=DF-7&amp;persona={persona}\"")
        ),
        "{page}"
    );
    // Header: title, stream badge, pause, link to the rules.
    assert!(page.contains(">Network traffic</h1>") && page.contains("x-text=\"badge()\">Live<"));
    assert!(page.contains("togglePause()") && page.contains(">Pause<"));
    assert!(page.contains("<a href=\"/egress/allowlist\"") && page.contains(">Egress rules<"));
    assert!(
        page.contains("aria-label=\"Network traffic\""),
        "rail entry"
    );
    // Filter bar: before and outside `#traffic`, so typing never loses focus.
    let traffic = page.find("id=\"traffic\"").unwrap();
    for control in [
        "name=\"q\"",
        "name=\"verdict\"",
        "name=\"persona\"",
        "name=\"task\"",
        "Clear filters",
        "Advanced",
    ] {
        assert!(
            page.find(control).unwrap() < traffic,
            "{control} is outside #traffic"
        );
    }
    assert!(page.contains("hx-select=\"#traffic\"") && page.contains("hx-push-url=\"true\""));
    assert!(page.contains("hx-trigger=\"input changed delay:300ms, change\""));
    // Segments with counts; the selected one is checked; one advanced filter is in use besides project.
    assert!(
        page.contains("id=\"count-all\"")
            && page.contains("id=\"count-blocked\"")
            && page.contains("id=\"count-allowed\"")
    );
    assert!(page.contains("value=\"blocked\" checked"), "{page}");
    assert!(page.contains("value=\"DF-7\""));
    // Clear filters only shows while a filter is set.
    assert!(page.contains("dirty: true,") && page.contains("x-show=\"dirty\""));
    assert!(page.contains(">2</span>"), "advanced badge");
    // As in the design: one right-aligned row, Advanced opens an inline row below it.
    assert!(page.contains("id=\"filter-row\" class=\"flex flex-wrap items-center justify-end"));
    assert!(
        page.contains("aria-controls=\"adv-filters\"")
            && page.contains(":aria-expanded=\"advOpen\"")
    );
    assert!(page.contains("id=\"adv-filters\" class=\"flex flex-wrap items-center justify-end"));
    // The design's filter icon (Lucide `list-filter`) before the label.
    let advanced_button = &page[page.find("aria-controls=\"adv-filters\"").unwrap()..];
    assert!(
        advanced_button.find("M7 12h10").unwrap() < advanced_button.find("Advanced").unwrap(),
        "{advanced_button}"
    );
    // Persona by name; the dropped filters are gone.
    assert!(
        page.contains("<option value=\"\">All personas</option>"),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            "<option value=\"{persona}\" selected>Linus</option>"
        )),
        "{page}"
    );
    assert!(!page.contains("name=\"sandbox\"") && !page.contains("name=\"project\""));
    // The advanced controls keep their own width, so label and control stay on one line.
    let advanced = &page[page.find("id=\"adv-filters\"").unwrap()..page.find("</form>").unwrap()];
    assert!(!advanced.contains("w-full"), "{advanced}");
    // Events panel and empty state.
    assert!(page.contains("Events · newest first") && page.contains("0 of 0"));
    assert!(page.contains("No network traffic yet") && !page.contains("data-node="));
    assert!(
        page.contains("id=\"graph\">"),
        "the graph sits on the panel, as in the design"
    );
    for column in [">Time<", ">Sandbox<", ">Destination<", ">Verdict<"] {
        assert!(page.contains(column), "{column}");
    }
    assert!(!page.contains("id=\"denied-hosts\""));
}

#[tokio::test]
async fn the_split_view_has_an_accessible_separator_and_the_details_panel_is_a_dialog() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    let separator = &page[page.find("role=\"separator\"").unwrap()..];
    let separator = &separator[..separator.find('>').unwrap()];
    for attribute in [
        "aria-valuenow=\"520\"",
        "aria-valuemin=\"397\"",
        "aria-valuemax=",
        "tabindex=\"0\"",
        "aria-orientation=\"vertical\"",
    ] {
        assert!(separator.contains(attribute), "{attribute} in {separator}");
    }
    assert!(
        separator.contains("divKey($event)") && separator.contains("setW(520)"),
        "{separator}"
    );
    assert!(
        page.contains("grid-template-columns:var(--graph-w,520px) 16px minmax(0,1fr)"),
        "{page}"
    );
    assert!(
        page.contains("df.traffic.graph-w") && page.contains("try {"),
        "stored in try/catch"
    );
    // The markup does not depend on a stored value: the width is applied by Alpine.
    // Object form: Alpine sets the property and keeps the static grid style.
    assert!(
        page.contains(":style=\"{'--graph-w': w + 'px'}\""),
        "{page}"
    );
    assert!(
        page.contains("x-on:resize.window=\"setW(want, false)\""),
        "re-clamped on resize"
    );
    assert!(page.contains("role=\"dialog\"") && page.contains("aria-labelledby=\"flow-title\""));
    assert!(page.contains("Requests inside TLS tunnels are not inspected."));
    assert!(page.contains("button.closest('tr')"));
    assert!(page.contains("htmx:after:swap") && page.contains("keydown.escape.window"));
    // Pause removes the connect element and Resume inserts it again.
    assert!(
        page.contains("id=\"sse-slot\"")
            && page.contains("slot.replaceChildren()")
            && page.contains("htmx.process")
    );
}

#[tokio::test]
async fn pause_stops_the_stream_explicitly_and_the_stored_width_is_clamped() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    // Nothing observes the removal of the connect element: the stream is aborted by hand.
    let pause = &page[page.find("togglePause() {").unwrap()..];
    let abort = pause.find("abortController.abort()").unwrap();
    assert!(
        abort < pause.find("slot.replaceChildren()").unwrap(),
        "{pause}"
    );
    assert!(pause.contains("sse.reader.cancel()"));
    // The stored width is passed through the limits once the layout exists.
    assert!(page.contains("this.$nextTick(() =&gt; this.setW(this.want, false))"));
    // Only a drag, a key or a double click remembers a width; fitting does not.
    assert!(page.contains("setW(value, persist = true)"));
    // Clamp first, then remember what is shown.
    assert!(page.contains("Math.round(persist ? value : this.want)"));
    assert!(page.contains("if (persist) {"));
    assert!(page.contains("this.want = this.w"));
    assert_eq!(page.matches("localStorage.setItem").count(), 1);
}

#[tokio::test]
async fn the_advanced_badge_follows_the_inputs_and_not_only_the_page_load() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?persona=p-1&task=DF-1", &cookie).await).await;
    assert!(page.contains("adv: 2,"), "{page}");
    assert!(
        page.contains("x-on:input=\"sync($el)\"") && page.contains("x-on:change=\"sync($el)\""),
        "the persona select only fires change"
    );
    assert!(page.contains("this.adv = this.advancedCount(form)"));
    assert!(page.contains("x-show=\"adv &gt; 0\""));
    assert!(
        page.contains("['persona', 'task'].filter("),
        "one list for both"
    );
    assert!(page.contains("x-text=\"adv\">2</span>"), "{page}");
    let none = text(app.get("/egress/log", &cookie).await).await;
    assert!(
        none.contains("adv: 0,") && none.contains("x-cloak"),
        "hidden until Alpine shows it"
    );
}

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

#[test]
fn the_host_search_ignores_case() {
    for (query, host) in [("GitHub", "api.github.com"), ("github", "API.GitHub.com")] {
        let filter = Filter {
            q: query.into(),
            ..Default::default()
        };
        let body = live(filter, &[allowed(host, "DF", 1)]).unwrap();
        assert!(part(&body, "#flows tbody").is_some(), "{query} vs {host}");
    }
}

#[tokio::test]
async fn without_gateway_discovery_the_log_is_unavailable() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let page = app.get("/egress/log", &cookie).await;
    assert_eq!(page.status(), StatusCode::SERVICE_UNAVAILABLE);
    let events = app.get("/egress/log/events", &cookie).await;
    assert_eq!(events.status(), StatusCode::SERVICE_UNAVAILABLE);
}

fn opened(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        phase: Phase::Opened.into(),
        ..allowed(host, project, second)
    }
}

#[test]
fn opened_events_are_open_rows_and_count_as_allowed() {
    let batch = [
        opened("api.anthropic.com", "DF", 1),
        denied("statsig.anthropic.com", "DF", 2),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("data-outcome=\"open\"").count(), 1, "{rows}");
    assert!(rows.contains(">Open<"));
    // Scenario "Open connections": an open tunnel is allowed for the segments and the graph.
    assert_eq!(part(&body, "#count-allowed"), Some("1"));
    assert_eq!(part(&body, "#count-blocked"), Some("1"));
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(graph.matches("data-blocked=\"true\"").count(), 1);
    let allowed_only = live(
        Filter {
            verdict: Some(VerdictFilter::Allowed),
            ..Default::default()
        },
        &batch,
    )
    .unwrap();
    assert!(
        part(&allowed_only, "#flows tbody")
            .unwrap()
            .contains("api.anthropic.com")
    );
}

#[test]
fn the_marker_is_never_a_row() {
    let marker = FlowEvent {
        backlog_end: true,
        ..Default::default()
    };
    let mut viewer = Viewer::new(Filter::default());
    viewer.backlog_message(&[]);
    assert!(viewer.live_message(&[marker]).is_none());
}

#[test]
fn a_replacing_message_is_sent_even_without_events() {
    let body = Viewer::new(Filter::default()).backlog_message(&[]);
    assert!(
        body.contains("hx-target=\"#flows tbody\"") && body.contains("innerHTML"),
        "{body}"
    );
    // Graph and counts too, so that an earlier connection's state is cleared.
    assert!(
        !body.contains("afterbegin") && part(&body, "#graph").is_some(),
        "{body}"
    );
    assert_eq!(part(&body, "#event-count"), Some("0 of 0"));
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("No network traffic yet")
    );
}

/// An event of flow `id` of pod `gw-0`.
fn flow(mut event: FlowEvent, id: u64) -> FlowEvent {
    event.gateway_pod = "gw-0".into();
    event.flow_id = id;
    event
}

fn closed_of(host: &str, id: u64, second: i64) -> FlowEvent {
    flow(
        FlowEvent {
            phase: Phase::Closed.into(),
            bytes_out: 7,
            duration: Some(df_proto::pbjson_types::Duration {
                seconds: 2,
                nanos: 500_000_000,
            }),
            ..allowed(host, "DF", second)
        },
        id,
    )
}

fn row_id_of(pod: &str, id: u64) -> String {
    format!("flow-{}-{id}", hex::encode(pod))
}

#[test]
fn a_closing_event_replaces_the_open_row_of_its_flow() {
    let mut viewer = Viewer::new(Filter::default());
    let mut opened = flow(opened("api.anthropic.com", "DF", 1), 5);
    // Whatever the opening event carries, an open row shows no duration.
    opened.duration = Some(df_proto::pbjson_types::Duration {
        seconds: 1,
        nanos: 0,
    });
    let first = viewer.live_message(&[opened]).unwrap();
    let id = row_id_of("gw-0", 5);
    assert!(
        first.contains(&format!("id=\"{id}\"")) && first.contains("data-outcome=\"open\""),
        "{first}"
    );
    assert!(first.contains("afterbegin"));

    let second = viewer
        .live_message(&[closed_of("api.anthropic.com", 5, 9)])
        .unwrap();
    assert!(second.contains(&format!("hx-target=\"#{id}\"")), "{second}");
    assert!(
        second.contains("outerMorph") && !second.contains("afterbegin"),
        "{second}"
    );
    assert!(
        !second.contains("data-outcome=\"open\"") && second.contains("data-outcome=\"allowed\""),
        "{second}"
    );
    assert!(
        second.contains("2.5 s"),
        "the closed event carries the duration for the panel: {second}"
    );
    assert!(
        !first.contains("1.0 s") && first.contains("&quot;duration&quot;:&quot;&quot;"),
        "an open row has none: {first}"
    );
    // The closing event replaces the opened state of the edge: the graph is not sent again, nor
    // are the counts (the flow was counted once).
    assert!(
        part(&second, "#graph").is_none() && part(&second, "#event-count").is_none(),
        "{second}"
    );

    // The row is no longer open: a repeated closing event is a new row.
    let again = viewer
        .live_message(&[closed_of("api.anthropic.com", 5, 10)])
        .unwrap();
    assert!(again.contains("afterbegin"));
}

#[test]
fn a_closing_event_without_a_known_open_row_is_a_new_row() {
    let mut viewer = Viewer::new(Filter::default());
    let message = viewer
        .live_message(&[closed_of("api.anthropic.com", 8, 3)])
        .unwrap();
    assert!(
        message.contains("afterbegin") && !message.contains("outerMorph"),
        "{message}"
    );
    assert!(message.contains(&format!("id=\"{}\"", row_id_of("gw-0", 8))));

    // Opened events filtered out of the view leave nothing to replace either.
    let mut denied_only = Viewer::new(Filter {
        denied: "1".into(),
        ..Default::default()
    });
    let hidden = denied_only
        .live_message(&[flow(opened("a.com", "DF", 1), 1)])
        .unwrap();
    assert!(
        part(&hidden, "#flows tbody").is_none(),
        "no row, only counts and graph"
    );
    let denied_close = flow(denied("a.com", "DF", 2), 1);
    let message = denied_only.live_message(&[denied_close]).unwrap();
    assert!(message.contains("afterbegin"), "{message}");
}

#[test]
fn the_backlog_has_one_row_per_flow_and_tracks_the_open_ones() {
    let mut viewer = Viewer::new(Filter::default());
    let backlog = [
        flow(opened("done.example.com", "DF", 1), 1),
        flow(opened("still-open.example.com", "DF", 2), 2),
        closed_of("done.example.com", 1, 3),
        flow(denied("evil.example.com", "DF", 4), 3),
    ];
    let body = viewer.backlog_message(&backlog);
    assert!(
        body.contains("innerHTML") && !body.contains("afterbegin"),
        "{body}"
    );
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("<tr ").filter(|_| true).count(), 3, "{rows}");
    assert_eq!(rows.matches("data-outcome").count(), 3, "{rows}");
    assert_eq!(rows.matches("data-outcome=\"open\"").count(), 1, "{body}");
    assert!(rows.contains("still-open.example.com") && rows.contains("evil.example.com"));
    // Three flows, counted once each although two have an opening and a closing event.
    assert_eq!(part(&body, "#event-count"), Some("3 of 3"));

    // The flow that is still open is replaced when it closes; the collapsed one is not tracked.
    let close_open = viewer
        .live_message(&[closed_of("still-open.example.com", 2, 9)])
        .unwrap();
    assert!(close_open.contains(&format!("hx-target=\"#{}\"", row_id_of("gw-0", 2))));
    let late = viewer
        .live_message(&[closed_of("done.example.com", 1, 10)])
        .unwrap();
    assert!(late.contains("afterbegin"), "{late}");
}

#[test]
fn a_rollout_does_not_make_hosts_blocked() {
    let shutdown = FlowEvent {
        reason: "gateway-shutdown".into(),
        ..closed_of("api.anthropic.com", 4, 2)
    };
    let batch = [denied("statsig.anthropic.com", "DF", 1), shutdown];
    let default = Filter::default();
    for body in [
        live(default.clone(), &batch).unwrap(),
        Viewer::new(default).backlog_message(&batch),
    ] {
        let graph = part(&body, "#graph").unwrap();
        assert_eq!(graph.matches("data-blocked=\"true\"").count(), 1, "{graph}");
        let blocked = &graph[graph.find("data-blocked=\"true\"").unwrap()..];
        assert!(
            blocked[..blocked.find("</div>").unwrap()].contains("statsig"),
            "{graph}"
        );
        assert_eq!(part(&body, "#count-blocked"), Some("1"));
        // The shutdown still has its row.
        assert!(
            part(&body, "#flows tbody")
                .unwrap()
                .contains("gateway-shutdown")
        );
    }
}

#[tokio::test]
async fn every_connection_starts_by_replacing_the_rows_with_the_backlog() {
    let app = app_with_egress(vec![denied("statsig.anthropic.com", "DF", 1_700_000_001)]).await;
    let cookie = app.login().await;
    // The reconnect after a token refresh replays the same backlog: it must replace, not prepend.
    for _ in 0..2 {
        let response = app.get("/egress/log/events", &cookie).await;
        let frame = SseReader::new(response).next().await;
        assert!(frame.data.contains("innerHTML"), "{}", frame.data);
        assert!(!frame.data.contains("afterbegin"), "{}", frame.data);
        assert!(frame.data.contains("statsig.anthropic.com"));
    }
    let empty = app_with_egress(vec![]).await;
    let cookie = empty.login().await;
    let frame = SseReader::new(empty.get("/egress/log/events", &cookie).await)
        .next()
        .await;
    assert!(
        frame.data.contains("innerHTML") && !frame.data.contains("<tr"),
        "{}",
        frame.data
    );
}

fn message_of(event: &FlowEvent) -> String {
    live(Filter::default(), std::slice::from_ref(event)).unwrap()
}

/// The `duration` the details panel gets from a row of `message`.
fn duration_of(message: &str) -> String {
    let key = "&quot;duration&quot;:&quot;";
    let rest = &message[message.find(key).unwrap() + key.len()..];
    rest[..rest.find("&quot;").unwrap()].to_string()
}

#[test]
fn durations_are_short_and_human() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        duration_of(&message_of(&event))
    };
    assert_eq!(at(0, 12_000_000), "12 ms");
    assert_eq!(at(3, 400_000_000), "3.4 s");
    assert_eq!(at(125, 0), "2:05");
}

#[test]
fn opened_and_closed_in_one_batch_make_one_closed_row() {
    let mut viewer = Viewer::new(Filter::default());
    let batch = [
        flow(opened("a.example.com", "DF", 1), 9),
        closed_of("a.example.com", 9, 2),
    ];
    let body = viewer.live_message(&batch).unwrap();
    let id = row_id_of("gw-0", 9);
    assert_eq!(body.matches(&format!("id=\"{id}\"")).count(), 1, "{body}");
    assert!(
        !body.contains("outerMorph") && !body.contains("data-outcome=\"open\""),
        "{body}"
    );
    // Nothing stays tracked: a later close of that flow is a new row.
    let later = viewer
        .live_message(&[closed_of("a.example.com", 9, 3)])
        .unwrap();
    assert!(later.contains("afterbegin"));
}

#[test]
fn a_viewer_stops_tracking_open_rows_past_the_cap() {
    let mut viewer = Viewer::new(Filter::default());
    let cap = super::log::MAX_OPEN_ROWS as u64;
    let batch: Vec<_> = (1..=cap + 1)
        .map(|id| flow(opened("a.example.com", "DF", 1), id))
        .collect();
    viewer.live_message(&batch).unwrap();
    // The early flows were dropped from the set: their close is a new row. The latest is kept.
    let early = viewer
        .live_message(&[closed_of("a.example.com", 1, 5)])
        .unwrap();
    assert!(
        early.contains("afterbegin") && !early.contains("outerMorph"),
        "{early}"
    );
    let latest = viewer
        .live_message(&[closed_of("a.example.com", cap + 1, 5)])
        .unwrap();
    assert!(latest.contains("outerMorph"), "{latest}");
}

#[test]
fn a_rollout_is_not_shown_by_the_blocked_filter() {
    let shutdown = FlowEvent {
        reason: "gateway-shutdown".into(),
        ..closed_of("api.anthropic.com", 4, 2)
    };
    let blocked_only = Filter {
        denied: "1".into(),
        ..Default::default()
    };
    let message = live(blocked_only.clone(), std::slice::from_ref(&shutdown)).unwrap();
    assert!(part(&message, "#flows tbody").is_none(), "{message}");
    let backlog = Viewer::new(blocked_only).backlog_message(&[shutdown]);
    assert!(
        !part(&backlog, "#flows tbody")
            .unwrap()
            .contains("api.anthropic.com"),
        "{backlog}"
    );
    assert_eq!(part(&backlog, "#event-count"), Some("0 of 1"));
}

#[test]
fn absurd_durations_neither_overflow_nor_go_negative() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        duration_of(&message_of(&event))
    };
    assert!(at(i64::MAX, 999_999_999).contains(':'));
    assert_eq!(at(-5, 0), "0 ms", "negative values clamp to zero");
    assert_eq!(at(0, -7_000_000), "0 ms");
}

#[test]
fn the_row_limit_is_a_view_setting_with_fixed_choices() {
    assert_eq!(Filter::default().row_limit(), 500);
    let rows = filter_from_query("rows=100");
    assert_eq!(rows.row_limit(), 100);
    assert_eq!(rows.query(), "?rows=100", "kept in the URL and the stream");
    assert_eq!(rows.advanced_count(), 0);
    assert!(rows.is_empty(), "no filter: Clear filters stays hidden");
    // The default is not written; other values fall back to it.
    assert_eq!(filter_from_query("rows=500").query(), "");
    for odd in ["rows=7", "rows=all", "rows=-1"] {
        assert_eq!(filter_from_query(odd).row_limit(), 500, "{odd}");
    }
}

#[test]
fn the_backlog_sends_only_the_newest_rows_up_to_the_limit() {
    let backlog: Vec<_> = (1..=150)
        .map(|i| flow(allowed(&format!("h{i}.com"), "DF", i), i as u64))
        .collect();
    let body = Viewer::new(filter_from_query("rows=100")).backlog_message(&backlog);
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("data-outcome=").count(), 100, "{rows}");
    assert!(rows.contains("h150.com:443") && rows.contains("h51.com:443"));
    assert!(!rows.contains("h50.com:443"));
    // The counts still cover every event.
    assert_eq!(part(&body, "#event-count"), Some("150 of 150"));
}

#[test]
fn a_flow_whose_row_was_trimmed_closes_as_a_new_row() {
    // Flow 1 is open and the oldest of 100 rows: the next new row pushes it out of the table.
    let mut backlog = vec![flow(opened("a.example.com", "DF", 1), 1)];
    backlog.extend((2..=100).map(|i| flow(allowed(&format!("h{i}.com"), "DF", i), i as u64)));
    let mut viewer = Viewer::new(filter_from_query("rows=100"));
    let body = viewer.backlog_message(&backlog);
    assert_eq!(
        part(&body, "#flows tbody")
            .unwrap()
            .matches("data-outcome=")
            .count(),
        100
    );
    viewer
        .live_message(&[flow(allowed("new.example.com", "DF", 200), 200)])
        .unwrap();
    // The page no longer has its row: the close is a new row, not a replacement.
    let close = viewer
        .live_message(&[closed_of("a.example.com", 1, 201)])
        .unwrap();
    assert!(
        close.contains("afterbegin") && !close.contains("outerMorph"),
        "{close}"
    );
    // A flow still in the table is replaced in place.
    let mut viewer = Viewer::new(filter_from_query("rows=100"));
    viewer.backlog_message(&backlog);
    let close = viewer
        .live_message(&[closed_of("a.example.com", 1, 201)])
        .unwrap();
    assert!(close.contains("outerMorph"), "{close}");
}

#[tokio::test]
async fn the_rows_choice_sits_in_the_filter_bar_and_the_page_trims_the_table() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?rows=1000", &cookie).await).await;
    let select = &page[page.find("name=\"rows\"").unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    for value in ["100", "250", "500", "2500", "5000"] {
        assert!(
            select.contains(&format!("<option value=\"{value}\">")),
            "{value}: {select}"
        );
    }
    assert!(
        select.contains("<option value=\"1000\" selected>"),
        "{select}"
    );
    assert!(
        page.find("name=\"rows\"").unwrap() < page.find("id=\"traffic\"").unwrap(),
        "part of the filter form"
    );
    assert!(page.contains("<tbody data-limit=\"1000\">"), "{page}");
    // Live rows past the limit are dropped from the bottom after every swap.
    assert!(page.contains("tbody.rows.length &gt; limit"), "{page}");
    // Clear filters resets the persona choice, not the row limit.
    assert!(
        page.contains("input[type=text], select[name=persona]')"),
        "{page}"
    );
    let default = text(app.get("/egress/log", &cookie).await).await;
    assert!(
        default.contains("<option value=\"500\" selected>")
            && default.contains("data-limit=\"500\"")
    );
}
