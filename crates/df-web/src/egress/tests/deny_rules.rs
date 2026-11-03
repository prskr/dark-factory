//! Deny rules in the rules dialog and the rules table.

use axum::http::StatusCode;
use df_db::{EntryKind, Notification};

use super::{add_host, app_with_egress, notifications_before_marker};
use crate::testkit::{TestApp, count_disabled_attributes, form, text};

async fn add_deny(app: &TestApp, cookie: &str, host: &str) -> axum::response::Response {
    app.post(
        "/egress/allowlist/host",
        &form(&[
            ("effect", "deny"),
            ("project", "DF"),
            ("host", host),
            ("port", "8443"),
        ]),
        cookie,
    )
    .await
}

#[tokio::test]
async fn the_dialog_opens_on_allow_with_enabled_fields() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let modal = text(app.get_htmx("/egress/allowlist/host/new", &cookie).await).await;

    assert!(modal.contains("New egress rule"), "{modal}");
    assert!(
        modal.contains("name=\"effect\" value=\"allow\" checked")
            && modal.contains("name=\"effect\" value=\"deny\"")
            && !modal.contains("value=\"deny\" checked"),
        "{modal}"
    );
    assert!(count_disabled_attributes(&modal) == 0, "{modal}");
    assert!(
        modal.contains("egressRule(") && modal.contains(">Add rule<"),
        "{modal}"
    );
}

#[tokio::test]
async fn a_deny_form_shown_again_keeps_applies_to_and_port_disabled() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let response = add_deny(&app, &cookie, "10.0.0.5").await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains("IP addresses are not allowed"), "{html}");
    assert!(
        html.contains("name=\"effect\" value=\"deny\" checked")
            && html.contains("value=\"any\"")
            && count_disabled_attributes(&html) >= 2
            && html.contains(">Add deny rule<"),
        "{html}"
    );
}

#[tokio::test]
async fn saving_deny_creates_an_instance_wide_rule_ignoring_project_and_port() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let notifier = df_db::Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let response = add_deny(&app, &cookie, "*.DatadogHQ.com").await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains(">*.datadoghq.com<") && html.contains(">Deny<"),
        "{html}"
    );
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project_key, None);
    assert_eq!(
        entries[0].kind,
        EntryKind::HttpDeny {
            host: "*.datadoghq.com".into()
        }
    );
    // A deny rule is not published: the gateway does not know it.
    let published = notifications_before_marker(&app, &mut notifications).await;
    assert!(
        !published.contains(&Notification::EgressPolicy),
        "{published:?}"
    );
    let duplicate = add_deny(&app, &cookie, "*.datadoghq.com").await;
    assert_eq!(duplicate.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(duplicate).await.contains("already exists"));
}

#[tokio::test]
async fn a_bare_wildcard_is_no_deny_rule() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let response = add_deny(&app, &cookie, "*").await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn deny_rules_are_listed_with_effect_any_port_everywhere_and_a_delete_button() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_deny(&app, &cookie, "*.datadoghq.com").await;

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    let allow = super::rules::row_of(&page, ">github.com<");
    assert!(
        allow.contains(">Allow<") && allow.contains(">443<"),
        "{allow}"
    );
    let deny = super::rules::row_of(&page, ">*.datadoghq.com<");
    assert!(
        deny.contains(">Deny<")
            && deny.contains(">any<")
            && deny.contains(">Everywhere<")
            && deny.contains("aria-label=\"Delete egress rule *.datadoghq.com:any\""),
        "{deny}"
    );
    assert!(
        page.contains("Egress rules <span class=\"font-normal text-faint\">2</span>"),
        "title counts deny rules: {page}"
    );
}

#[tokio::test]
async fn deleting_a_deny_rule_is_audited_without_a_policy_notification() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_deny(&app, &cookie, "*.datadoghq.com").await;
    let id = df_db::list_entries(&app.pool).await.unwrap()[0].id;
    let notifier = df_db::Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let response = app
        .delete(&format!("/egress/allowlist/{id}"), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!text(response).await.contains("datadoghq"));
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_log WHERE resource_type = 'egress_allowlist' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        actions,
        ["egress_deny_rule.create", "egress_deny_rule.delete"]
    );
    let published = notifications_before_marker(&app, &mut notifications).await;
    assert!(
        !published.contains(&Notification::EgressPolicy),
        "{published:?}"
    );
}

#[tokio::test]
async fn the_deny_rule_link_highlights_its_row() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_deny(&app, &cookie, "*.datadoghq.com").await;
    add_host(&app, &cookie, "", "datadoghq.com", "443").await;

    let page = text(
        app.get("/egress/allowlist?highlight=*.datadoghq.com:any", &cookie)
            .await,
    )
    .await;

    assert_eq!(page.matches("aria-current=\"true\"").count(), 1, "{page}");
    let row = super::rules::row_of(&page, ">*.datadoghq.com<");
    assert!(row.contains("aria-current=\"true\""), "{row}");
}

#[test]
fn the_dialog_component_disables_applies_to_and_port_and_shows_any_for_deny() {
    let script = crate::assets::script_source("egress-rule.js");

    assert!(script.contains("Alpine.data('egressRule'"), "{script}");
    assert!(script.contains("this.effect === 'deny'"), "{script}");
    // The values shown while disabled: every project, port `any`.
    assert!(
        script.contains("['project', '']") && script.contains("['port', 'any']"),
        "{script}"
    );
}

#[tokio::test]
async fn the_dialog_binds_applies_to_and_port_to_the_deny_state() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let modal = text(app.get_htmx("/egress/allowlist/host/new", &cookie).await).await;

    assert_eq!(modal.matches(":disabled=\"deny\"").count(), 2, "{modal}");
    assert!(modal.contains("x-ref=\"project\"") && modal.contains("x-ref=\"port\""));
}
