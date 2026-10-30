//! Allowlist entries and the rules page.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use df_db::{EntryKind, Notification};

use super::{BrokenServices, add_host, allowed, app_with_egress, new_viewer};
use crate::egress::log::Filter;
use crate::testkit::{TestApp, form, header_str, seed, text};

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

#[tokio::test]
async fn a_failing_service_lookup_is_shown_to_the_board_member() {
    let app = TestApp::with_egress(
        df_core::routing::ForgeRoutes::default(),
        None,
        Some(Arc::new(BrokenServices)),
    )
    .await;
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
    let delete =
        crate::testkit::tag_around(&page, "aria-label=\"Delete egress rule github.com:443\"");
    assert!(
        delete.contains(crate::components::ICON_BUTTON_CLASS),
        "{delete}"
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
    let row = new_viewer(Filter::default()).backlog_message(&[event]);
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
