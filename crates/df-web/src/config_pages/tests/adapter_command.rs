//! The adapter command field of the project form: shell-style quoting (`configuration-management`,
//! "Projects").

use axum::http::StatusCode;

use super::{REPO, assert_project_saved, stored_adapter_command};
use crate::testkit::{TestApp, form, seed, text};

async fn save_command(
    app: &TestApp,
    cookie: &str,
    command: &str,
) -> axum::http::Response<axum::body::Body> {
    let body = form(&[
        ("key", "DF"),
        ("name", "Dark Factory"),
        ("repo_url", REPO),
        ("issue_tracker_url", ""),
        ("image", "ghcr.io/example/sandbox:1"),
        ("base_branch", "main"),
        ("adapter_command", command),
        ("forge_credential_id", ""),
        ("webhook_wake", "members"),
    ]);
    app.post("/projects/DF", &body, cookie).await
}

/// Saves `command` and returns the arguments stored for it.
async fn stored_after_saving(command: &str) -> Vec<String> {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    assert_project_saved(&save_command(&app, &cookie, command).await);
    stored_adapter_command(&app).await
}

#[tokio::test]
async fn a_quoted_argument_with_a_space_is_one_argument() {
    assert_eq!(
        stored_after_saving("/opt/adapter '--config=/workspace/my config.json'").await,
        ["/opt/adapter", "--config=/workspace/my config.json"]
    );
}

#[tokio::test]
async fn the_settings_page_shows_the_command_quoted_and_saving_it_unchanged_keeps_the_arguments() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    save_command(
        &app,
        &cookie,
        "/opt/adapter '--config=/workspace/my config.json'",
    )
    .await;

    let html = text(app.get("/projects/DF", &cookie).await).await;

    let shown = "/opt/adapter '--config=/workspace/my config.json'";
    assert!(html.contains(&format!("value=\"{shown}\"")), "{html}");
    assert!(html.contains("shell-style quoting"), "{html}");
    assert_project_saved(&save_command(&app, &cookie, shown).await);
    assert_eq!(
        stored_adapter_command(&app).await,
        ["/opt/adapter", "--config=/workspace/my config.json"]
    );
}

#[tokio::test]
async fn an_unclosed_quote_is_rejected_at_the_field_and_nothing_is_stored() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    assert_project_saved(&save_command(&app, &cookie, "/opt/adapter --ok").await);

    let response = save_command(&app, &cookie, "/opt/adapter 'unclosed").await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains("The quotes are not closed."), "{html}");
    assert!(
        html.contains("value=\"/opt/adapter &#39;unclosed\"")
            || html.contains("value=\"/opt/adapter 'unclosed\""),
        "the typed value stays: {html}"
    );
    assert_eq!(stored_adapter_command(&app).await, ["/opt/adapter", "--ok"]);
}

#[tokio::test]
async fn an_empty_field_stores_no_arguments() {
    assert!(stored_after_saving("").await.is_empty());
    assert!(stored_after_saving("   ").await.is_empty());
}

#[tokio::test]
async fn quoted_empty_arguments_and_escaped_quotes_are_kept() {
    assert_eq!(stored_after_saving("'--empty' ''").await, ["--empty", ""]);
    assert_eq!(
        stored_after_saving(r#""say \"hi\"""#).await,
        [r#"say "hi""#]
    );
}

#[tokio::test]
async fn an_unquoted_hash_starts_a_comment_and_a_quoted_one_is_kept() {
    assert_eq!(
        stored_after_saving("/opt/adapter #fff").await,
        ["/opt/adapter"]
    );
    assert_eq!(
        stored_after_saving("/opt/adapter '#fff'").await,
        ["/opt/adapter", "#fff"]
    );
}

#[tokio::test]
async fn a_double_backslash_stores_one_backslash_and_nothing_is_expanded() {
    assert_eq!(
        stored_after_saving(r"/opt/adapter \\ $HOME").await,
        ["/opt/adapter", r"\", "$HOME"]
    );
}

#[tokio::test]
async fn project_adapter_command_is_stored_shown_again_and_clearable() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = |command: &str| {
        form(&[
            ("key", "DF"),
            ("name", "Dark Factory"),
            ("repo_url", REPO),
            ("issue_tracker_url", ""),
            ("image", "ghcr.io/example/sandbox:1"),
            ("base_branch", "main"),
            ("adapter_command", command),
            ("forge_credential_id", ""),
            ("webhook_wake", "members"),
        ])
    };

    let response = app
        .post(
            "/projects/DF",
            &body("/usr/local/bin/my-acp-adapter --flag"),
            &cookie,
        )
        .await;
    assert_project_saved(&response);
    assert_eq!(
        stored_adapter_command(&app).await,
        ["/usr/local/bin/my-acp-adapter", "--flag"]
    );
    let edit = text(app.get("/projects/DF", &cookie).await).await;
    assert!(
        edit.contains("value=\"/usr/local/bin/my-acp-adapter --flag\""),
        "{edit}"
    );
    assert!(edit.contains("Adapter command (optional)"));
    assert!(edit.contains("Leave empty to use the instance"));

    let response = app.post("/projects/DF", &body(""), &cookie).await;
    assert_project_saved(&response);
    assert!(stored_adapter_command(&app).await.is_empty());
}
