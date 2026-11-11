//! Tests of the model provider settings page and the create dialog.

use axum::http::StatusCode;

use super::{credential, provider_form, stored};
use crate::testkit::{TestApp, header_str, open_tab, tab_labels, tag_around, text};

/// A stored provider named "Anthropic work".
async fn stored_provider(app: &TestApp, cookie: &str) -> uuid::Uuid {
    let credential = credential(app, "anthropic-key").await;
    app.post("/model-providers", &provider_form(&credential, &[]), cookie)
        .await;
    stored(app, "Anthropic work").await
}

#[tokio::test]
async fn the_page_shows_sidebar_back_link_header_tabs_and_save() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;

    let response = app.get(&format!("/model-providers/{id}"), &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("<html"), "a full page: {html}");
    assert!(!html.contains("<dialog id=\"modal\""), "{html}");
    let sidebar = &html[html.find("id=\"settings-menu\"").unwrap()..];
    let sidebar = &sidebar[..sidebar.find("</nav>").unwrap()];
    assert!(
        sidebar[..sidebar.find("aria-current").unwrap()].ends_with("<a href=\"/model-providers\" "),
        "{sidebar}"
    );
    let back = tag_around(&html, "class=\"mb-3 inline-flex min-h-11");
    assert!(back.contains("href=\"/model-providers\""), "{back}");
    let heading = &html[html.find("id=\"provider-heading\"").unwrap()..];
    let heading = &heading[..heading.find("</div>").unwrap()];
    assert!(
        heading.contains("<h1") && heading.contains("Anthropic work"),
        "{heading}"
    );
    assert_eq!(tab_labels(&html), ["General", "Models"]);
    assert_eq!(open_tab(&html), "general");
    assert!(html.contains(">Save model provider</button>"), "{html}");
    assert!(!html.contains("id=\"mobile-menu\""), "{html}");
    let status = tag_around(&html, "id=\"save-status\"");
    assert!(
        status.contains("role=\"status\"") && status.contains("aria-live=\"polite\""),
        "{status}"
    );
}

#[tokio::test]
async fn the_query_chooses_the_open_tab_and_an_unknown_value_opens_general() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;
    for (query, open) in [
        ("?tab=models", "models"),
        ("?tab=general", "general"),
        ("?tab=nonsense", "general"),
        ("", "general"),
    ] {
        let html = text(
            app.get(&format!("/model-providers/{id}{query}"), &cookie)
                .await,
        )
        .await;
        assert_eq!(open_tab(&html), open, "{query}");
    }
}

#[tokio::test]
async fn an_unknown_provider_and_the_old_edit_route_answer_404() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;
    for uri in [
        format!("/model-providers/{}", uuid::Uuid::now_v7()),
        format!("/model-providers/{id}/edit"),
    ] {
        assert_eq!(
            app.get_htmx(&uri, &cookie).await.status(),
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn the_list_links_name_and_edit_action_to_the_page() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;

    let html = text(app.get("/model-providers", &cookie).await).await;

    let edit = tag_around(&html, "aria-label=\"Edit model provider Anthropic work\"");
    assert!(
        edit.starts_with(&format!("<a href=\"/model-providers/{id}\"")),
        "{edit}"
    );
    let name = tag_around(&html, ">Anthropic work</a>");
    assert!(
        name.contains(&format!("href=\"/model-providers/{id}\"")),
        "{name}"
    );
    assert!(!html.contains("/edit"), "{html}");
    assert!(
        html.contains("hx-delete="),
        "delete stays in the list: {html}"
    );
}

#[tokio::test]
async fn a_save_re_renders_the_form_and_says_model_provider_saved_without_closing_a_modal() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;
    let credential = credential(&app, "other-key").await;
    let body = format!(
        "{}&tab=models&entity_id={id}",
        provider_form(
            &credential,
            &[
                ("name", "Renamed"),
                ("model_id_0", "e2e-extra-model"),
                ("model_api_0", "anthropic-messages"),
            ]
        )
    );

    let response = app
        .post(&format!("/model-providers/{id}"), &body, &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "");
    assert_eq!(header_str(&response, "hx-retarget"), "");
    let html = text(response).await;
    assert!(html.starts_with("<form id=\"form\""), "{html}");
    assert!(html.contains("value=\"Renamed\""), "{html}");
    assert_eq!(open_tab(&html), "models");
    assert!(html.contains("value=\"e2e-extra-model\""), "{html}");
    let status = &html[html.find("hx-target=\"#save-status\"").unwrap()..];
    assert!(status.contains("Model provider saved"), "{status}");
    let heading = &html[html.find("hx-target=\"#provider-heading\"").unwrap()..];
    assert!(heading.contains("Renamed"), "{heading}");
    assert!(!html.contains("modal-close"), "{html}");
    assert_eq!(stored(&app, "Renamed").await, id);
}

#[tokio::test]
async fn a_validation_error_opens_the_tab_of_the_first_error_and_keeps_the_values() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;
    let credential = credential(&app, "other-key").await;
    // The user is on Models; the error is on General.
    let body = format!(
        "{}&tab=models&entity_id={id}",
        provider_form(
            &credential,
            &[("url_anthropic_messages", "http://insecure.example.org")]
        )
    );

    let response = app
        .post(&format!("/model-providers/{id}"), &body, &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "general");
    assert!(
        html.contains("value=\"http://insecure.example.org\""),
        "{html}"
    );
    assert!(!html.contains("Model provider saved"), "{html}");
}

#[tokio::test]
async fn refreshing_in_edit_mode_keeps_the_page_tab_style_and_the_current_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;
    let body = format!("{}&op=add&entity_id={id}", provider_form("", &[]));

    let html = text(app.post("/model-providers/refresh", &body, &cookie).await).await;

    assert_eq!(open_tab(&html), "models");
    assert!(
        html.contains("name=\"tab\""),
        "the page style posts its tab: {html}"
    );
    assert!(html.contains("tabs(&quot;models&quot;, true)"), "{html}");
    assert!(html.contains("model_id_0"), "{html}");
}

#[tokio::test]
async fn the_models_tab_lists_every_model_on_one_wide_line_without_an_inner_scroll_area() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;

    let html = text(
        app.get(&format!("/model-providers/{id}?tab=models"), &cookie)
            .await,
    )
    .await;

    let panel = &html[html.find("id=\"panel-models\"").unwrap()..];
    assert!(
        !panel.contains("overflow-y-auto") && !panel.contains("max-h-96"),
        "{panel}"
    );
    // Wire API 220 px, both limits 140 px; the ID takes the rest of the line.
    assert!(panel.contains("w-55") && panel.contains("w-35"), "{panel}");
}

#[tokio::test]
async fn the_new_provider_dialog_is_896_px_wide() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/model-providers/new", &cookie).await).await;

    assert!(
        html.contains("md:max-w-4xl") && !html.contains("md:max-w-3xl"),
        "{html}"
    );
}
