//! Deny rules on the traffic page: labels, the missing actions and "Deny always".

use axum::http::StatusCode;
use df_db::EntryKind;
use df_proto::gateway::v1::FlowEvent;

use super::{add_deny_rule, allowed, app_with_egress, denied, part, viewer_with_deny};
use crate::testkit::{SseReader, form, text};

const DATADOG: &str = "http-intake.logs.us5.datadoghq.com";

/// A denial of a run, which offers the allow action.
fn run_denial(host: &str) -> FlowEvent {
    FlowEvent {
        run_id: "run-1".into(),
        ..denied(host, "DF", 1)
    }
}

fn rows(viewer: &mut super::Viewer, events: &[FlowEvent]) -> String {
    let body = viewer.backlog_message(events);
    part(&body, "#flows tbody").unwrap().to_string()
}

#[test]
fn a_denied_host_shows_the_deny_rule_and_offers_no_actions() {
    let mut viewer = viewer_with_deny(&["*.datadoghq.com"]);

    let rows = rows(&mut viewer, &[run_denial(DATADOG)]);

    assert!(
        rows.contains("aria-label=\"Deny rule *.datadoghq.com\"")
            && rows.contains("href=\"/egress/allowlist?highlight=*.datadoghq.com%3Aany\"")
            && rows.contains(">Deny rule<"),
        "{rows}"
    );
    assert!(
        !rows.contains(">Deny always<")
            && !rows.contains("/egress/log/allow")
            && !rows.contains("/egress/log/deny"),
        "{rows}"
    );
}

#[test]
fn the_details_of_a_denied_host_name_the_rule_with_a_link() {
    let mut viewer = viewer_with_deny(&["*.datadoghq.com"]);

    let rows = rows(&mut viewer, &[denied(DATADOG, "DF", 1)]);

    let flow = rows
        .split("data-flow=\"")
        .nth(1)
        .and_then(|rest| rest.split("\" data-outcome").next())
        .unwrap()
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    let flow: serde_json::Value = serde_json::from_str(&flow).unwrap();
    assert_eq!(flow["reason"], "not-allowlisted");
    assert_eq!(flow["rule"], "*.datadoghq.com");
    assert_eq!(
        flow["rule_href"],
        "/egress/allowlist?highlight=*.datadoghq.com%3Aany"
    );
}

#[test]
fn other_not_allowlisted_rows_offer_deny_always_next_to_allow() {
    let mut viewer = viewer_with_deny(&["*.datadoghq.com"]);

    let rows = rows(&mut viewer, &[run_denial("statsig.anthropic.com")]);

    let row = super::rules::row_of(&rows, "statsig.anthropic.com:443");
    assert!(
        row.contains("hx-post=\"/egress/log/deny\"")
            && row.contains("&quot;host&quot;:&quot;statsig.anthropic.com&quot;")
            && row.contains("aria-label=\"Allow statsig.anthropic.com:443"),
        "{row}"
    );
    assert_eq!(
        row.matches(">Deny always<").count(),
        2,
        "wide slot and phone menu: {row}"
    );
    assert!(!row.contains(">Deny rule<"));
}

#[test]
fn allowed_rows_and_other_denial_reasons_offer_no_deny_always() {
    let mut viewer = viewer_with_deny(&[]);
    let mut mismatch = denied("evil.example", "DF", 2);
    mismatch.reason = "sni-mismatch".into();

    let rows = rows(&mut viewer, &[allowed("pypi.org", "DF", 1), mismatch]);

    assert!(!rows.contains("Deny always"), "{rows}");
}

#[test]
fn a_blocked_graph_node_of_a_denied_host_offers_no_allow_action() {
    let events = [run_denial(DATADOG), run_denial("statsig.anthropic.com")];
    let mut viewer = viewer_with_deny(&["*.datadoghq.com"]);

    let body = viewer.backlog_message(&events);

    let graph = part(&body, "#graph").unwrap();
    assert_eq!(graph.matches("data-blocked=\"true\"").count(), 2, "{graph}");
    assert!(graph.contains("Allow statsig.anthropic.com:443"), "{graph}");
    assert!(!graph.contains(&format!("Allow {DATADOG}")), "{graph}");
}

#[tokio::test]
async fn the_served_page_labels_a_denied_host_and_offers_no_actions() {
    let app = app_with_egress(vec![run_denial(DATADOG)]).await;
    let cookie = app.login().await;
    add_deny_rule(&app, "*.datadoghq.com").await;

    let page = text(app.get("/egress/log", &cookie).await).await;

    assert!(
        page.contains("aria-label=\"Deny rule *.datadoghq.com\""),
        "{page}"
    );
    assert!(!page.contains("/egress/log/deny"), "{page}");
}

#[tokio::test]
async fn a_deny_rule_added_while_the_page_is_open_shows_after_the_stream_reconnects() {
    let app = app_with_egress(vec![run_denial(DATADOG)]).await;
    let cookie = app.login().await;
    let before = text(app.get("/egress/log", &cookie).await).await;
    assert!(before.contains("hx-post=\"/egress/log/deny\""), "{before}");

    add_deny_rule(&app, "*.datadoghq.com").await;
    let frame = SseReader::new(app.get("/egress/log/events", &cookie).await)
        .next()
        .await;

    let rows = part(&frame.data, "#flows tbody").unwrap();
    assert!(
        rows.contains("aria-label=\"Deny rule *.datadoghq.com\"")
            && !rows.contains("/egress/log/allow")
            && !rows.contains("/egress/log/deny"),
        "{rows}"
    );
}

#[tokio::test]
async fn deny_always_creates_a_deny_rule_for_exactly_the_host() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let response = app
        .post("/egress/log/deny", &form(&[("host", DATADOG)]), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("hx-retarget").unwrap(),
        "#allow-toast"
    );
    let toast = text(response).await;
    assert!(
        toast.contains("Deny rule created") && toast.contains("View rule"),
        "{toast}"
    );
    let rules = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].kind,
        EntryKind::HttpDeny {
            host: DATADOG.into()
        }
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'egress_deny_rule.create'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
    let again = app
        .post("/egress/log/deny", &form(&[("host", DATADOG)]), &cookie)
        .await;
    assert_eq!(again.status(), StatusCode::OK, "an existing rule is fine");
}

#[tokio::test]
async fn deny_always_rejects_wildcards_and_ip_addresses() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for host in ["*.datadoghq.com", "10.0.0.5", ""] {
        let response = app
            .post("/egress/log/deny", &form(&[("host", host)]), &cookie)
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{host}"
        );
    }
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}
