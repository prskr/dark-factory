//! The forms of the rule dialogs have an id of their own: on the project settings page the dialog
//! mounts after the project form, whose id is `form`, so a shared id would swap the wrong form.

use super::app_with_egress;
use super::rule_id;
use crate::testkit::{form, seed, text};

#[tokio::test]
async fn the_dialogs_target_their_own_form_and_not_the_project_form() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    app.post(
        "/egress/allowlist/host",
        &form(&[("project", "DF"), ("host", "pypi.org")]),
        &cookie,
    )
    .await;
    let id = rule_id(&app, "pypi.org").await;

    for url in [
        "/egress/allowlist/host/new?return=project:DF".to_string(),
        "/egress/allowlist/service/new?return=project:DF".to_string(),
        format!("/egress/allowlist/{id}/edit?return=project:DF"),
    ] {
        let dialog = text(app.get_htmx(&url, &cookie).await).await;
        assert!(dialog.contains("id=\"rule-form\""), "{url}: {dialog}");
        assert!(
            dialog.contains("hx-target=\"#rule-form\""),
            "{url}: {dialog}"
        );
        assert!(!dialog.contains("id=\"form\""), "{url}: {dialog}");
        assert!(!dialog.contains("hx-target=\"#form\""), "{url}: {dialog}");
    }
}

#[tokio::test]
async fn a_rejected_save_from_the_project_tab_shows_the_message_in_the_dialog_form() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    for host in ["pypi.org", "github.com"] {
        app.post(
            "/egress/allowlist/host",
            &form(&[("project", "DF"), ("host", host)]),
            &cookie,
        )
        .await;
    }
    let id = rule_id(&app, "pypi.org").await;

    let response = app
        .post(
            &format!("/egress/allowlist/{id}"),
            &form(&[
                ("project", "DF"),
                ("host", "github.com"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;

    let html = text(response).await;
    assert!(html.contains("id=\"rule-form\""), "{html}");
    assert!(html.contains("this entry already exists"), "{html}");
}
