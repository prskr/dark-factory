//! The new chat dialog and starting a chat from it.

use axum::http::StatusCode;

use super::between;
use crate::testkit::{TestApp, add_persona, form, header_str, seed, text};

// Scenario "Start from the dialog".
#[tokio::test]
async fn the_dialog_has_the_fixed_project_a_message_and_every_persona_with_its_details() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    add_persona(&app.pool, &seed, "Grace", "Plans the release").await;

    let html = text(app.get_htmx("/chats/compose?project=DF", &cookie).await).await;

    assert!(html.contains("<dialog id=\"composer\""), "{html}");
    assert!(
        html.contains("<input type=\"hidden\" name=\"project\" value=\"DF\">"),
        "{html}"
    );
    assert!(html.contains("Dark Factory"), "the fixed project: {html}");
    assert!(
        !html.contains("<select"),
        "the project can't be changed: {html}"
    );
    assert!(html.contains("name=\"message\""), "{html}");
    let picker = between(&html, "<fieldset", "</fieldset>");
    for needle in [
        "Grace",
        "Plans the release",
        "Linus",
        "Claude Code",
        "claude-sonnet-5-5",
    ] {
        assert!(picker.contains(needle), "{needle}: {picker}");
    }
    assert_eq!(
        picker.matches("type=\"radio\" name=\"persona\"").count(),
        2,
        "{picker}"
    );
    assert_eq!(
        picker.matches(" checked").count(),
        1,
        "the first persona is chosen"
    );
    assert!(html.contains("hx-post=\"/chats\"") && html.contains("aria-label=\"Start chat\""));
}

#[tokio::test]
async fn the_dialog_of_an_unknown_project_is_not_found_and_without_personas_it_says_so() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let unknown = app.get_htmx("/chats/compose?project=NOPE", &cookie).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    sqlx::query("DELETE FROM personas WHERE id = $1")
        .bind(seed.persona)
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get_htmx("/chats/compose?project=DF", &cookie).await).await;
    assert!(html.contains("Create a persona first."), "{html}");
}

#[tokio::test]
async fn sending_starts_the_chat_with_the_persona_and_opens_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let grace = add_persona(&app.pool, &seed, "Grace", "Plans the release").await;

    let response = app
        .post(
            "/chats",
            &form(&[
                ("project", "DF"),
                ("persona", &grace.to_string()),
                ("message", "Plan the Helm chart release"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/chats/DF-1-CHAT");
    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(
        page.contains("Plan the Helm chart release"),
        "the message in the transcript: {page}"
    );
    let list = between(&page, "<div id=\"chat-rows\"", "</ul>");
    assert!(
        list.contains("row-DF-1-CHAT") && list.contains("Grace"),
        "{list}"
    );
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'chat.create'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(audited, 1);
}

// Scenario "Empty first message".
#[tokio::test]
async fn an_empty_message_or_a_missing_persona_keeps_the_dialog_with_errors_and_starts_nothing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let empty = app
        .post(
            "/chats",
            &form(&[
                ("project", "DF"),
                ("persona", &seed.persona.to_string()),
                ("message", "  "),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(empty).await.contains("Write a first message."));

    let nobody = app
        .post(
            "/chats",
            &form(&[("project", "DF"), ("persona", ""), ("message", "Hello")]),
            &cookie,
        )
        .await;
    assert_eq!(nobody.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(nobody).await.contains("Choose a persona."));

    let chats: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE kind = 'chat'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(chats, 0);
}
