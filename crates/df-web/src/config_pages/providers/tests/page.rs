//! Tests of the model provider settings page and the create dialog.

use axum::http::StatusCode;

use super::{credential, provider_form, stored};
use crate::testkit::{
    TestApp, head_of, header_str, open_tab, tab_labels, table_of, tag_around, text,
};

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
async fn the_list_row_is_a_two_line_cover_link_to_the_page_with_its_own_delete() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = stored_provider(&app, &cookie).await;

    let html = text(app.get("/model-providers", &cookie).await).await;

    let table = table_of(&html);
    let head = head_of(table);
    assert!(
        head.contains(">Name</th>")
            && head.contains(">Used by</th>")
            && head.contains("<span class=\"sr-only\">Actions</span>")
            && !head.contains("Wire APIs")
            && !head.contains("Scheme"),
        "{head}"
    );
    let name = tag_around(table, ">Anthropic work</a>");
    assert!(
        name.contains(&format!("href=\"/model-providers/{id}\"")) && name.contains("after:inset-0"),
        "{name}"
    );
    assert!(
        table.contains("class=\"group relative border-t border-line hover:bg-sunk\""),
        "{table}"
    );
    // Wire APIs, scheme and credential on the second line, with decorative separators.
    assert!(
        table.contains("Anthropic Messages <span aria-hidden=\"true\" class=\"mx-1\">·</span> ")
            && table.contains("anthropic-key (llm-secret/api-key)"),
        "{table}"
    );
    // "Used by" stays on one line, in the header and in the cell.
    assert!(
        head.contains(
            "<th scope=\"col\" class=\"whitespace-nowrap px-3 py-3 font-medium\">Used by</th>"
        ),
        "{head}"
    );
    assert!(
        table.contains("<td class=\"whitespace-nowrap px-3 py-3.5\">"),
        "{table}"
    );
    let delete = tag_around(table, "aria-label=\"Delete model provider Anthropic work\"");
    assert!(
        delete.contains("hx-confirm=\"Delete model provider Anthropic work?\""),
        "{delete}"
    );
    let cell = &table[..table.find("hx-delete=").unwrap()];
    assert!(cell.ends_with("<div class=\"relative z-10 flex items-center justify-end gap-1\"><button type=\"button\" "), "{cell}");
    assert!(
        !table.contains("Edit model provider") && !html.contains("/edit"),
        "{html}"
    );
}

#[tokio::test]
async fn the_second_line_reads_wire_apis_scheme_and_credential_in_order() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "openai").await;
    let body = crate::testkit::form(&[
        ("name", "OpenAI"),
        ("credential_ref_id", &credential),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://api.openai.com"),
        ("api_openai_responses", "on"),
        ("url_openai_responses", "https://api.openai.com"),
    ]);
    let created = app.post("/model-providers", &body, &cookie).await;
    assert_eq!(created.status(), StatusCode::OK);

    let html = text(app.get("/model-providers", &cookie).await).await;

    let table = table_of(&html);
    let marker = "truncate text-[13px] text-muted\">";
    let line = &table[table.find(marker).unwrap() + marker.len()..];
    let line = &line[..line.find("</div>").unwrap()];
    assert_eq!(
        line.matches("<span aria-hidden=\"true\" class=\"mx-1\">·</span>")
            .count(),
        2
    );
    let mut visible = String::new();
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => visible.push(c),
            _ => {}
        }
    }
    assert_eq!(
        visible,
        "OpenAI Chat Completions, OpenAI Responses · Authorization: Bearer · openai (llm-secret/api-key)"
    );
}

#[tokio::test]
async fn a_provider_whose_credential_is_unknown_shows_missing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    stored_provider(&app, &cookie).await;
    let record = df_db::model_providers::list_model_providers(&app.pool)
        .await
        .unwrap()
        .remove(0);

    let html = super::super::region(
        &[super::super::ListEntry {
            record,
            personas: vec![],
        }],
        &[],
        None,
    )
    .into_string();

    assert!(html.contains("missing"), "{html}");
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
