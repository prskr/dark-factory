//! Tests of the persona settings page `/personas/{id}`: its tabs, the URL of the open tab, saving
//! and the Markdown field on the System prompt tab (`configuration-management`, "Persona settings
//! page").

use axum::http::StatusCode;
use uuid::Uuid;

use super::{assert_persona_created, assert_persona_saved, persona_page};
use crate::testkit::{TestApp, form, seed_provider, text};

/// The body of a save of persona `handle` with `commit_email`, the model and the `system_prompt`.
fn save_body(
    provider: &str,
    handle: &str,
    commit_email: &str,
    model: &str,
    system_prompt: &str,
    tab: &str,
) -> String {
    form(&[
        ("display_name", "Linus"),
        ("handle", handle),
        ("commit_email", commit_email),
        ("system_prompt", system_prompt),
        ("harness", "claude-code"),
        ("model_provider_id", provider),
        ("model", model),
        ("tab", tab),
    ])
}

/// Persona `handle` created through the modal's fields; returns its id.
async fn create(app: &TestApp, cookie: &str, handle: &str, email: &str) -> Uuid {
    let provider = seed_provider(&app.pool).await.to_string();
    let body = form(&[
        ("display_name", "Linus"),
        ("handle", handle),
        ("commit_email", email),
        ("harness", "claude-code"),
        ("model_provider_id", &provider),
        ("model", "claude-sonnet-5-5"),
    ]);
    assert_persona_created(&app.post("/personas", &body, cookie).await);
    sqlx::query_scalar("SELECT id FROM personas WHERE handle = $1")
        .bind(handle)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// The tab a page or a re-rendered form opens: the argument of its `tabs(...)` component.
fn open_tab(html: &str) -> &str {
    let start =
        html.find("x-data=\"tabs(&quot;").expect("tabs component") + "x-data=\"tabs(&quot;".len();
    &html[start..start + html[start..].find("&quot;").unwrap()]
}

/// The content of the panel of `tab`.
fn panel<'a>(html: &'a str, tab: &str) -> &'a str {
    let start = html
        .find(&format!("id=\"panel-{tab}\""))
        .unwrap_or_else(|| panic!("panel {tab}: {html}"));
    // `role` comes before `id` in the tag, so the next `role="tabpanel"` opens the next panel.
    let rest = &html[start..];
    &rest[..rest.find("role=\"tabpanel\"").unwrap_or(rest.len())]
}

#[tokio::test]
async fn the_page_has_the_sidebar_a_back_link_the_heading_the_tabs_and_one_save_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;

    let html = persona_page(&app, &cookie, id, "").await;

    let sidebar = &html[html.find("id=\"settings-menu\"").unwrap()..];
    let marked = &sidebar[..sidebar.find("aria-current").unwrap()];
    assert!(marked.ends_with("<a href=\"/personas\" "), "{marked}");
    assert!(html.contains("<a href=\"/personas\"") && html.contains(">Personas</a>"));
    assert!(
        html.contains("id=\"persona-heading\"") && html.contains(">Linus</h1>"),
        "{html}"
    );
    assert!(html.contains("Claude Code · claude-sonnet-5-5"), "{html}");
    for tab in ["general", "system-prompt", "commits"] {
        assert!(html.contains(&format!("data-tab=\"{tab}\"")), "{tab}");
    }
    assert_eq!(
        html.matches("role=\"tab\"").count(),
        3,
        "the field's switch is no tab"
    );
    assert_eq!(
        html.matches(">Save persona</button>").count(),
        2,
        "header and phone"
    );
    assert_eq!(open_tab(&html), "general");
}

#[tokio::test]
async fn the_groups_are_identity_and_agent_on_general_and_commits_on_its_own_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;

    let html = persona_page(&app, &cookie, id, "").await;

    let general = panel(&html, "general");
    let identity = general.find(">Identity</h3>").expect("Identity");
    let agent = general.find(">Agent</h3>").expect("Agent");
    assert!(
        identity < agent && !general.contains(">Commits</h3>"),
        "{general}"
    );
    assert!(general.contains("id=\"agent-fields\""));
    let commits = panel(&html, "commits");
    assert!(commits.contains(">Commits</h3>") && commits.contains("name=\"commit_email\""));
    assert!(!panel(&html, "system-prompt").contains(">Identity</h3>"));
}

#[tokio::test]
async fn the_url_names_the_open_tab_and_an_unknown_tab_opens_general() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;

    for (tab, open) in [
        ("system-prompt", "system-prompt"),
        ("commits", "commits"),
        ("nope", "general"),
        ("general", "general"),
    ] {
        let html = persona_page(&app, &cookie, id, tab).await;
        assert_eq!(open_tab(&html), open, "?tab={tab}");
    }
    // The tab syncs the URL (`tabs(..., true)`), General is left out of it.
    let html = persona_page(&app, &cookie, id, "commits").await;
    assert!(html.contains("tabs(&quot;commits&quot;, true"), "{html}");
}

#[tokio::test]
async fn an_unknown_persona_and_the_old_edit_route_are_not_found() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;

    let unknown = app
        .get(&format!("/personas/{}", Uuid::now_v7()), &cookie)
        .await;
    let edit = app.get_htmx(&format!("/personas/{id}/edit"), &cookie).await;

    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(edit.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_system_prompt_tab_holds_only_the_markdown_field_with_a_rendered_first_preview() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;
    let provider = seed_provider(&app.pool).await.to_string();
    let prompt = "## Rules\n\n- be kind";
    let body = save_body(
        &provider,
        "linus",
        "linus@example.org",
        "claude-sonnet-5-5",
        prompt,
        "",
    );
    assert_persona_saved(&app.post(&format!("/personas/{id}"), &body, &cookie).await);

    let html = persona_page(&app, &cookie, id, "system-prompt").await;

    let tab = panel(&html, "system-prompt");
    assert!(
        tab.contains("markdownField({&quot;split&quot;:true})"),
        "{tab}"
    );
    // The label is for screen readers only; the field is the only one on the tab.
    assert!(
        tab.contains("class=\"sr-only\">System prompt</label>"),
        "{tab}"
    );
    assert_eq!(tab.matches("<textarea").count(), 1, "{tab}");
    assert!(tab.contains(">## Rules\n\n- be kind</textarea>"), "{tab}");
    // The preview is rendered into the page.
    assert!(
        tab.contains("<h2>Rules</h2>") && tab.contains("<li>be kind</li>"),
        "{tab}"
    );
    // The field's view switch is no tab, so the `tabs` component of the page stays as it is.
    assert!(!tab.contains("role=\"tab\""), "{tab}");
}

#[tokio::test]
async fn saving_stays_on_the_posted_tab_and_says_persona_saved() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;
    let provider = seed_provider(&app.pool).await.to_string();
    let body = save_body(
        &provider,
        "linus",
        "linus@example.org",
        "claude-sonnet-5-5",
        "New prompt.",
        "system-prompt",
    );

    let response = app.post(&format!("/personas/{id}"), &body, &cookie).await;

    assert_persona_saved(&response);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "system-prompt");
    assert!(html.contains(">New prompt.</textarea>"), "{html}");
    // The status line and the heading arrive out of band.
    assert!(
        html.contains("hx-target=\"#save-status\"") && html.contains("Persona saved"),
        "{html}"
    );
    assert!(html.contains("hx-target=\"#persona-heading\""), "{html}");
}

#[tokio::test]
async fn a_missing_model_opens_general_and_keeps_the_system_prompt() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;
    let provider = seed_provider(&app.pool).await.to_string();
    let body = save_body(
        &provider,
        "linus",
        "linus@example.org",
        "",
        "Typed on the prompt tab.",
        "system-prompt",
    );

    let response = app.post(&format!("/personas/{id}"), &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "general", "{html}");
    assert!(
        html.contains("Model is required.") && html.contains("id=\"model-error\""),
        "{html}"
    );
    assert!(
        html.contains(">Typed on the prompt tab.</textarea>"),
        "{html}"
    );
    // The re-rendered form is the page's form, so the URL follows the tab.
    assert!(html.contains("tabs(&quot;general&quot;, true"), "{html}");
}

#[tokio::test]
async fn a_taken_commit_email_opens_the_commits_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    create(&app, &cookie, "ada", "ada@example.org").await;
    let id = create(&app, &cookie, "linus", "linus@example.org").await;
    let provider = seed_provider(&app.pool).await.to_string();
    let body = save_body(
        &provider,
        "linus",
        "ada@example.org",
        "claude-sonnet-5-5",
        "x",
        "general",
    );

    let response = app.post(&format!("/personas/{id}"), &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "commits", "{html}");
    assert!(
        html.contains("Already used by @ada.") && html.contains("id=\"commit_email-error\""),
        "{html}"
    );
    assert!(
        html.contains("value=\"Linus\""),
        "other values are kept: {html}"
    );
}

#[tokio::test]
async fn an_unknown_persona_cannot_be_saved() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    let body = save_body(
        &provider,
        "linus",
        "linus@example.org",
        "claude-sonnet-5-5",
        "x",
        "",
    );

    let response = app
        .post(&format!("/personas/{}", Uuid::now_v7()), &body, &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_card_without_description_and_prompt_says_no_system_prompt_yet_and_links_to_the_page() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create(&app, &cookie, "ada", "ada@example.org").await;

    let html = text(app.get("/personas", &cookie).await).await;

    assert!(html.contains(&format!("href=\"/personas/{id}\"")), "{html}");
    assert!(html.contains("text-faint\">No system prompt yet"), "{html}");
}
