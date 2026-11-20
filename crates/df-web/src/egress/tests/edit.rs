//! Editing rules: the edit button, the prefilled dialogs and `POST /egress/allowlist/{id}`.

use axum::http::StatusCode;
use df_db::EntryKind;

use super::rules::row_of;
use super::{add_deny_rule, add_host, add_project, add_task_host, app_with_egress, rule_id};
use crate::testkit::{TestApp, form, header_str, new_task, seed, tag_around, text};

async fn post_edit(
    app: &TestApp,
    cookie: &str,
    id: uuid::Uuid,
    fields: &[(&str, &str)],
) -> axum::response::Response {
    app.post(&format!("/egress/allowlist/{id}"), &form(fields), cookie)
        .await
}

#[tokio::test]
async fn every_row_has_an_edit_button_named_after_its_target() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    add_deny_rule(&app, "*.datadoghq.com").await;
    let id = rule_id(&app, "pypi.org").await;

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    let button = tag_around(&page, "aria-label=\"Edit egress rule pypi.org:443\"");
    assert!(
        button.contains(&format!("hx-get=\"/egress/allowlist/{id}/edit\"")),
        "{button}"
    );
    assert!(row_of(&page, ">pypi.org<").contains("aria-label=\"Delete egress rule pypi.org:443\""));
    assert!(
        page.contains("aria-label=\"Edit egress rule *.datadoghq.com:any\""),
        "{page}"
    );
}

#[tokio::test]
async fn the_edit_dialog_is_prefilled_with_a_fixed_effect_and_no_delete_button() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.orgg", "8443").await;
    let id = rule_id(&app, "pypi.orgg").await;

    let dialog = text(
        app.get_htmx(&format!("/egress/allowlist/{id}/edit"), &cookie)
            .await,
    )
    .await;

    assert!(dialog.contains("Edit egress rule"), "{dialog}");
    assert!(dialog.contains(">Save rule<"), "{dialog}");
    assert!(!dialog.contains("Delete rule"), "{dialog}");
    assert!(
        dialog.contains(&format!("hx-post=\"/egress/allowlist/{id}\"")),
        "{dialog}"
    );
    assert!(
        dialog.contains("name=\"host\" type=\"text\" value=\"pypi.orgg\""),
        "{dialog}"
    );
    assert!(dialog.contains("value=\"8443\""), "{dialog}");
    assert!(
        dialog.contains("<option value=\"DF\" selected>"),
        "{dialog}"
    );
    // The effect shows as selected and disabled; a hidden field carries it.
    let allow = tag_around(&dialog, "name=\"effect\" value=\"allow\"");
    assert!(
        allow.contains(" checked") && allow.contains(" disabled"),
        "{allow}"
    );
    let deny = tag_around(&dialog, "name=\"effect\" value=\"deny\"");
    assert!(
        deny.contains(" disabled") && !deny.contains(" checked"),
        "{deny}"
    );
    assert!(
        dialog.contains("<input type=\"hidden\" name=\"effect\" value=\"allow\">"),
        "{dialog}"
    );
    assert!(!dialog.contains("value=\"task:"), "{dialog}");
}

#[tokio::test]
async fn the_edit_dialog_of_a_deny_rule_fixes_effect_scope_and_port() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_deny_rule(&app, "*.datadoghq.co").await;
    let id = rule_id(&app, "*.datadoghq.co").await;

    let dialog = text(
        app.get_htmx(&format!("/egress/allowlist/{id}/edit"), &cookie)
            .await,
    )
    .await;

    let deny = tag_around(&dialog, "name=\"effect\" value=\"deny\"");
    assert!(
        deny.contains(" checked") && deny.contains(" disabled"),
        "{deny}"
    );
    assert!(dialog.contains("value=\"*.datadoghq.co\""), "{dialog}");
    assert!(
        dialog.contains("<input type=\"hidden\" name=\"effect\" value=\"deny\">")
            && dialog.contains(">Save rule<"),
        "{dialog}"
    );
    let select = tag_around(&dialog, "select id=\"project\"");
    let port = tag_around(&dialog, "input id=\"port\"");
    assert!(
        crate::testkit::has_disabled_attribute(select)
            && crate::testkit::has_disabled_attribute(port),
        "{select} {port}"
    );
}

#[tokio::test]
async fn the_edit_dialog_of_a_cluster_service_has_its_own_title_and_values() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    app.post(
        "/egress/allowlist/service",
        &form(&[
            ("namespace", "nativelink"),
            ("service", "scheduler"),
            ("ports", "50051, 50052"),
            ("allow_dns", "on"),
        ]),
        &cookie,
    )
    .await;
    let id = df_db::list_entries(&app.pool).await.unwrap()[0].id;

    let dialog = text(
        app.get_htmx(&format!("/egress/allowlist/{id}/edit"), &cookie)
            .await,
    )
    .await;

    assert!(dialog.contains("Edit cluster service"), "{dialog}");
    assert!(dialog.contains(">Save rule<"), "{dialog}");
    assert!(
        dialog.contains(&format!("hx-post=\"/egress/allowlist/{id}\"")),
        "{dialog}"
    );
    for value in [
        "value=\"nativelink\"",
        "value=\"scheduler\"",
        "value=\"50051, 50052\"",
    ] {
        assert!(dialog.contains(value), "{value}: {dialog}");
    }
    assert!(
        tag_around(&dialog, "name=\"allow_dns\"").contains(" checked"),
        "{dialog}"
    );
}

#[tokio::test]
async fn a_task_rule_offers_its_task_first_and_preselected_before_instance_and_projects() {
    let app = app_with_egress(vec![]).await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix CI").await;
    let cookie = app.login().await;
    let entry = add_task_host(&app, task.id.as_uuid(), "pypi.org").await;

    let dialog = text(
        app.get_htmx(&format!("/egress/allowlist/{}/edit", entry.id), &cookie)
            .await,
    )
    .await;

    let task_option = format!(
        "<option value=\"task:{}\" selected>Task {}</option>",
        task.key, task.key
    );
    let at = |needle: &str| {
        dialog
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {dialog}"))
    };
    assert!(
        at(&task_option) < at("<option value=\"\">Every project (instance)")
            && at("<option value=\"\">Every project (instance)") < at("<option value=\"DF\""),
        "{dialog}"
    );
}

#[tokio::test]
async fn an_unknown_rule_has_no_edit_dialog_and_a_plain_request_goes_to_the_list() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let unknown = uuid::Uuid::now_v7();

    let missing = app
        .get_htmx(&format!("/egress/allowlist/{unknown}/edit"), &cookie)
        .await;
    let plain = app
        .get(&format!("/egress/allowlist/{unknown}/edit"), &cookie)
        .await;

    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn saving_an_edit_fixes_the_host_in_place_and_keeps_search_and_page() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.orgg", "443").await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    let id = rule_id(&app, "pypi.orgg").await;

    let response = post_edit(
        &app,
        &cookie,
        id,
        &[
            ("project", "DF"),
            ("host", "pypi.org"),
            ("port", "443"),
            ("q", "pypi"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    assert_eq!(header_str(&response, "hx-trigger"), "modal-close");
    let html = text(response).await;
    assert!(
        html.contains(">pypi.org<") && !html.contains("pypi.orgg"),
        "{html}"
    );
    assert!(html.contains("value=\"pypi\""), "the search stays: {html}");
    assert!(!html.contains(">github.com<"), "the search filters: {html}");
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert!(entries.iter().any(|e| e.id == id), "same rule");
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn saving_an_edit_can_widen_a_project_rule_to_the_instance() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    let id = rule_id(&app, "github.com").await;

    // The posted effect is ignored: the stored kind decides.
    let response = post_edit(
        &app,
        &cookie,
        id,
        &[
            ("effect", "deny"),
            ("project", ""),
            ("host", "github.com"),
            ("port", "443"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        row_of(&html, ">github.com<").contains(">Everywhere<") && html.contains(">Allow<"),
        "{html}"
    );
    let entry = &df_db::list_entries(&app.pool).await.unwrap()[0];
    assert_eq!(entry.project_key, None);
}

#[tokio::test]
async fn a_task_rule_moves_to_its_project_and_not_to_another_task() {
    let app = app_with_egress(vec![]).await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix CI").await;
    let other = new_task(&app.pool, &seed, "Other").await;
    let cookie = app.login().await;
    let entry = add_task_host(&app, task.id.as_uuid(), "pypi.org").await;
    let task_value = format!("task:{}", task.key);

    let kept = post_edit(
        &app,
        &cookie,
        entry.id,
        &[
            ("project", &task_value),
            ("host", "pypi.org"),
            ("port", "80"),
        ],
    )
    .await;
    let foreign = post_edit(
        &app,
        &cookie,
        entry.id,
        &[
            ("project", &format!("task:{}", other.key)),
            ("host", "pypi.org"),
            ("port", "443"),
        ],
    )
    .await;
    let moved = post_edit(
        &app,
        &cookie,
        entry.id,
        &[("project", "DF"), ("host", "pypi.org"), ("port", "80")],
    )
    .await;

    assert_eq!(kept.status(), StatusCode::OK);
    assert_eq!(foreign.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(foreign).await.contains("Unknown scope."));
    assert_eq!(moved.status(), StatusCode::OK);
    assert!(text(moved).await.contains(">Project DF<"));
    let stored = &df_db::list_entries(&app.pool).await.unwrap()[0];
    assert_eq!(
        (stored.project_key.as_deref(), stored.task_key.as_deref()),
        (Some("DF"), None)
    );
}

#[tokio::test]
async fn a_project_rule_cannot_be_moved_to_a_task_by_a_forged_value() {
    let app = app_with_egress(vec![]).await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix CI").await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    let id = rule_id(&app, "pypi.org").await;

    let response = post_edit(
        &app,
        &cookie,
        id,
        &[
            ("project", &format!("task:{}", task.key)),
            ("host", "pypi.org"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        df_db::list_entries(&app.pool).await.unwrap()[0]
            .project_key
            .as_deref(),
        Some("DF")
    );
}

#[tokio::test]
async fn an_invalid_edit_shows_the_error_with_the_typed_values_and_changes_nothing() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    let id = rule_id(&app, "pypi.org").await;

    let response = post_edit(
        &app,
        &cookie,
        id,
        &[("project", "DF"), ("host", "10.0.0.5"), ("port", "443")],
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains("IP addresses are not allowed"), "{html}");
    assert!(
        html.contains("value=\"10.0.0.5\"") && html.contains(">Save rule<"),
        "{html}"
    );
    assert_eq!(
        df_db::list_entries(&app.pool).await.unwrap()[0].kind,
        EntryKind::HttpHost {
            host: "pypi.org".into(),
            port: 443
        }
    );
}

#[tokio::test]
async fn an_edit_that_duplicates_a_rule_is_a_form_error_with_the_create_message() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    let project_rule = df_db::list_entries(&app.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.project_key.is_some())
        .unwrap();

    let response = post_edit(
        &app,
        &cookie,
        project_rule.id,
        &[("project", ""), ("host", "github.com"), ("port", "443")],
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("this entry already exists"));
    assert_eq!(df_db::list_entries(&app.pool).await.unwrap().len(), 2);
}

#[tokio::test]
async fn saving_a_rule_that_was_deleted_meanwhile_says_so_and_creates_nothing() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "pypi.org", "443").await;
    let id = rule_id(&app, "pypi.org").await;
    app.delete(&format!("/egress/allowlist/{id}"), &cookie)
        .await;

    let response = post_edit(
        &app,
        &cookie,
        id,
        &[("project", ""), ("host", "pypi.org"), ("port", "443")],
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("This rule no longer exists"));
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_deny_rule_and_a_cluster_service_are_edited_through_the_same_route() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_deny_rule(&app, "*.datadoghq.co").await;
    let deny_id = rule_id(&app, "*.datadoghq.co").await;
    app.post(
        "/egress/allowlist/service",
        &form(&[
            ("namespace", "nativelink"),
            ("service", "scheduler"),
            ("ports", "50051"),
        ]),
        &cookie,
    )
    .await;
    let service_id = df_db::list_entries(&app.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|e| matches!(e.kind, EntryKind::ClusterService { .. }))
        .unwrap()
        .id;

    let deny = post_edit(
        &app,
        &cookie,
        deny_id,
        &[("effect", "deny"), ("host", "*.datadoghq.com")],
    )
    .await;
    let service = post_edit(
        &app,
        &cookie,
        service_id,
        &[
            ("project", "DF"),
            ("namespace", "nativelink"),
            ("service", "scheduler"),
            ("ports", "50051, 50052"),
        ],
    )
    .await;
    let unknown = post_edit(
        &app,
        &cookie,
        service_id,
        &[("namespace", "nativelink"), ("service", "schedular")],
    )
    .await;

    assert_eq!(deny.status(), StatusCode::OK);
    assert_eq!(service.status(), StatusCode::OK);
    assert_eq!(unknown.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(unknown)
            .await
            .contains("nativelink/schedular does not exist")
    );
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert!(entries.iter().any(|e| e.kind
        == EntryKind::HttpDeny {
            host: "*.datadoghq.com".into()
        }));
    assert!(entries.iter().any(|e| e.project_key.as_deref() == Some("DF")
        && matches!(&e.kind, EntryKind::ClusterService { ports, .. } if ports == &[50051, 50052])));
}

#[tokio::test]
async fn editing_a_rule_is_audited_with_both_values() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    add_project(&app, "OPS").await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    let id = rule_id(&app, "github.com").await;

    post_edit(
        &app,
        &cookie,
        id,
        &[("project", "OPS"), ("host", "github.com"), ("port", "443")],
    )
    .await;

    let details: serde_json::Value = sqlx::query_scalar(
        "SELECT details FROM audit_log WHERE action = 'egress_allowlist.update'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(details["before"]["project_key"], "DF");
    assert_eq!(details["after"]["project_key"], "OPS");
}
