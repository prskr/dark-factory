use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use df_db::{EntryKind, Notification};
use df_proto::gateway::v1::{FlowEvent, Phase, Verdict};
use df_proto::pbjson_types::Timestamp;
use futures::StreamExt;
use futures::stream::{self, BoxStream};

use super::log::{Filter, Viewer};
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
        html.contains("api.anthropic.com:443") && html.contains("Instance"),
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
    assert!(page.contains("api.anthropic.com:443"), "{page}");
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
    assert!(text(response).await.contains("*.crates.io:443"));

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
        html.contains("nativelink/scheduler: 50051, 50052, DNS allowed"),
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

// ---------------------------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------------------------

#[test]
fn denied_hosts_are_counted_per_host_with_their_projects() {
    let batch = [
        denied("statsig.anthropic.com", "DF", 1),
        allowed("api.anthropic.com", "DF", 2),
        denied("statsig.anthropic.com", "DF", 3),
        denied("statsig.anthropic.com", "WEB", 4),
        denied("evil.example.com", "DF", 5),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    // Rows newest first.
    let first = body.find("evil.example.com").unwrap();
    let last = body.find("api.anthropic.com").unwrap();
    assert!(first < last, "{body}");
    assert!(body.contains("hx-target=\"#flows tbody\"") && body.contains("afterbegin"));
    // Summary: the most denied host first, with count and projects.
    let summary_part = &body[body.find("id=\"denied-hosts\"").unwrap()..];
    assert!(
        summary_part.find("statsig.anthropic.com").unwrap()
            < summary_part.find("evil.example.com").unwrap()
    );
    assert!(
        summary_part.contains(">3<") && summary_part.contains("DF, WEB"),
        "{summary_part}"
    );
    assert!(
        !summary_part.contains("api.anthropic.com"),
        "allowed hosts are not summarized"
    );
}

#[test]
fn filters_limit_rows_and_the_summary() {
    let batch = [
        denied("a.example.com", "DF", 1),
        allowed("b.example.com", "DF", 2),
        denied("c.example.com", "WEB", 3),
    ];
    let filter = Filter {
        denied: "1".into(),
        project: "DF".into(),
        ..Default::default()
    };
    let body = live(filter, &batch).unwrap();
    assert!(body.contains("a.example.com"));
    assert!(
        !body.contains("b.example.com") && !body.contains("c.example.com"),
        "{body}"
    );
    let nothing = Filter {
        host: "nope".into(),
        ..Default::default()
    };
    assert!(live(nothing, &batch).is_none());
}

#[test]
fn a_gap_is_shown_as_a_row() {
    let gap = FlowEvent {
        gap: true,
        ..Default::default()
    };
    let body = live(Filter::default(), &[gap]).unwrap();
    assert!(body.contains("Events are missing"), "{body}");
    assert!(!body.contains("denied-hosts"));
}

#[test]
fn the_filter_query_carries_only_set_fields() {
    assert_eq!(Filter::default().query(), "");
    let filter = Filter {
        denied: "1".into(),
        host: "a b".into(),
        ..Default::default()
    };
    assert_eq!(filter.query(), "?denied=1&host=a+b");
}

#[tokio::test]
async fn a_repeatedly_denied_domain_appears_in_the_stream_and_the_summary() {
    let events = vec![
        denied("statsig.anthropic.com", "DF", 1_700_000_001),
        denied("statsig.anthropic.com", "DF", 1_700_000_002),
        denied("statsig.anthropic.com", "DF", 1_700_000_003),
    ];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let response = app
        .get("/egress/log/events?denied=1&project=DF", &cookie)
        .await;
    assert_eq!(header_str(&response, "content-type"), "text/event-stream");
    let frame = SseReader::new(response).next().await;
    assert!(
        frame.data.contains("statsig.anthropic.com"),
        "{}",
        frame.data
    );
    let summary = &frame.data[frame.data.find("id=\"denied-hosts\"").unwrap()..];
    assert!(
        summary.contains(">3<") && summary.contains(">DF<"),
        "{summary}"
    );
}

#[tokio::test]
async fn the_log_page_connects_to_the_stream_with_its_filters() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?denied=1&project=DF", &cookie).await).await;
    assert!(
        page.contains("hx-sse:connect=\"/egress/log/events?denied=1&amp;project=DF\""),
        "{page}"
    );
    assert!(page.contains("id=\"denied-hosts\"") && page.contains("id=\"flows\""));
    assert!(page.contains("aria-label=\"Egress log\""), "rail entry");
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
fn opened_events_are_open_rows_and_never_count_as_denied() {
    let batch = [
        opened("api.anthropic.com", "DF", 1),
        denied("statsig.anthropic.com", "DF", 2),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    assert!(body.contains(">open<"), "{body}");
    let summary_part = &body[body.find("id=\"denied-hosts\"").unwrap()..];
    assert!(
        summary_part.contains("statsig.anthropic.com")
            && !summary_part.contains("api.anthropic.com")
    );
}

#[test]
fn the_marker_is_never_a_row() {
    let marker = FlowEvent {
        backlog_end: true,
        ..Default::default()
    };
    assert!(live(Filter::default(), &[marker]).is_none());
}

#[test]
fn a_replacing_message_is_sent_even_without_events() {
    let body = Viewer::new(Filter::default()).backlog_message(&[]);
    assert!(
        body.contains("hx-target=\"#flows tbody\"") && body.contains("innerHTML"),
        "{body}"
    );
    assert!(
        !body.contains("afterbegin") && body.contains("id=\"denied-hosts\""),
        "{body}"
    );
}

fn live(filter: Filter, batch: &[FlowEvent]) -> Option<String> {
    Viewer::new(filter).live_message(batch)
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
        first.contains(&format!("id=\"{id}\"")) && first.contains(">open<"),
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
        !second.contains(">open<") && second.contains(">allowed<"),
        "{second}"
    );
    assert!(
        second.contains("2.5 s"),
        "the closed row shows the duration: {second}"
    );
    assert!(
        !first.contains(" s<") && !first.contains(" ms<"),
        "an open row has none: {first}"
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
    assert!(
        denied_only
            .live_message(&[flow(opened("a.com", "DF", 1), 1)])
            .is_none()
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
    assert_eq!(body.matches("done.example.com").count(), 1, "{body}");
    assert_eq!(body.matches(">open<").count(), 1, "{body}");
    assert!(body.contains("still-open.example.com") && body.contains("evil.example.com"));

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
fn a_rollout_does_not_make_hosts_denied() {
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
        let summary = &body[body.find("id=\"denied-hosts\"").unwrap()..];
        assert!(summary.contains("statsig.anthropic.com"), "{summary}");
        assert!(!summary.contains("api.anthropic.com"), "{summary}");
        // The shutdown still has its row.
        assert!(body[..body.find("id=\"denied-hosts\"").unwrap()].contains("gateway-shutdown"));
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

#[test]
fn durations_are_short_and_human() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        message_of(&event)
    };
    assert!(at(0, 12_000_000).contains("12 ms"));
    assert!(at(3, 400_000_000).contains("3.4 s"));
    assert!(at(125, 0).contains("2:05"));
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
        !body.contains("outerMorph") && !body.contains(">open<"),
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
fn a_rollout_is_not_shown_by_the_denied_only_filter() {
    let shutdown = FlowEvent {
        reason: "gateway-shutdown".into(),
        ..closed_of("api.anthropic.com", 4, 2)
    };
    let denied_only = Filter {
        denied: "1".into(),
        ..Default::default()
    };
    assert!(live(denied_only.clone(), std::slice::from_ref(&shutdown)).is_none());
    let backlog = Viewer::new(denied_only).backlog_message(&[shutdown]);
    assert!(!backlog.contains("api.anthropic.com"), "{backlog}");
}

#[test]
fn absurd_durations_neither_overflow_nor_go_negative() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        message_of(&event)
    };
    assert!(at(i64::MAX, 999_999_999).contains(':'));
    assert!(
        at(-5, 0).contains(">0 ms<"),
        "negative values clamp to zero"
    );
    assert!(at(0, -7_000_000).contains(">0 ms<"));
}
