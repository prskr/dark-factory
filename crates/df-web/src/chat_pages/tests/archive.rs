//! The archive modal: list, search, a selected chat with its tabs, an opened artifact and
//! Unarchive.

use axum::http::StatusCode;

use super::{agent_says, between, idle_chat};
use crate::testkit::{TestApp, new_chat, text};

/// Archives the chat and returns it.
async fn archived(app: &TestApp, chat: &df_db::TaskRow) {
    df_db::chats::archive_chat(&app.pool, chat.id, crate::testkit::actor().0)
        .await
        .unwrap();
}

async fn artifact(app: &TestApp, chat: &df_db::TaskRow, name: &str, content: &str) {
    sqlx::query(
        "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, content, sha256)
         VALUES ($1, $2, 'text', 'text/markdown', $3, convert_to($4, 'UTF8'), $5)",
    )
    .bind(chat.id.as_uuid())
    .bind(name)
    .bind(i64::try_from(content.len()).unwrap())
    .bind(content)
    .bind(format!("{:0>64}", name.len()))
    .execute(&app.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn the_modal_lists_the_archived_chats_with_persona_topic_key_message_artifacts_and_date() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, chat) = idle_chat(&app).await;
    agent_says(&app, &chat, "The plan is ready.").await;
    artifact(&app, &chat, "plan.md", "# Plan").await;
    archived(&app, &chat).await;
    new_chat(&app.pool, &seed, "Still open").await;

    let html = text(app.get_htmx("/chats/archive?project=DF", &cookie).await).await;

    assert!(html.contains("<dialog id=\"archive\""), "{html}");
    let list = between(&html, "<ul class=\"min-h-0", "</ul>");
    for needle in [
        "Linus",
        "Plan the Helm chart release",
        "DF-1-CHAT",
        "The plan is ready.",
        "1 artifacts",
        "archived",
    ] {
        assert!(list.contains(needle), "{needle}: {list}");
    }
    assert!(!html.contains("Still open"), "{html}");
    assert!(
        html.contains("name=\"q\"") && html.contains("archiveModal("),
        "{html}"
    );
}

// Scenario "Search".
#[tokio::test]
async fn the_search_lists_the_archived_chats_whose_topic_persona_or_messages_match() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, helm) = idle_chat(&app).await;
    let other = new_chat(&app.pool, &seed, "Triage the backlog").await;
    crate::testkit::set_task_status(&app.pool, other.id, df_core::TaskStatus::Waiting).await;
    archived(&app, &helm).await;
    archived(&app, &other).await;

    let html = text(
        app.get_htmx("/chats/archive?project=DF&part=body&q=HELM", &cookie)
            .await,
    )
    .await;

    assert!(
        html.starts_with("<div id=\"archive-body\""),
        "the body alone: {html}"
    );
    assert!(
        html.contains("DF-1-CHAT") && !html.contains("DF-2-CHAT"),
        "{html}"
    );
    assert!(html.contains("1 of 2 archived"), "{html}");
    let none = text(
        app.get_htmx("/chats/archive?project=DF&part=body&q=zzz", &cookie)
            .await,
    )
    .await;
    assert!(none.contains("No archived chat matches."), "{none}");
}

// Scenario "Chats on phones": the archive fills the screen below its header.
#[tokio::test]
async fn the_archive_body_fills_the_screen_on_a_phone() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(
        app.get_htmx("/chats/archive?project=DF&part=body", &cookie)
            .await,
    )
    .await;

    assert!(html.contains("h-[calc(100dvh-4.625rem)]"), "{html}");
}

#[tokio::test]
async fn selecting_a_chat_turns_the_list_into_a_sidebar_beside_a_read_only_transcript() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    agent_says(&app, &chat, "Everything is decided.").await;
    archived(&app, &chat).await;

    let html = text(
        app.get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT",
            &cookie,
        )
        .await,
    )
    .await;

    assert!(html.contains("md:w-80"), "the list is a sidebar: {html}");
    let pane = between(
        &html,
        "<section class=\"flex min-h-0 min-w-0 flex-1 flex-col\"",
        "</section>",
    );
    assert!(
        pane.contains("Everything is decided.") && pane.contains("id=\"transcript\""),
        "{pane}"
    );
    for tab in ["Transcript", "Artifacts"] {
        assert!(
            between(pane, "role=\"tablist\"", "</div>").contains(tab),
            "{tab}"
        );
    }
    assert!(pane.contains("Unarchive"), "{pane}");
    assert!(
        !pane.contains("id=\"message-box\"") && !pane.contains("name=\"message\""),
        "read-only: {pane}"
    );
}

// Scenario "Open an archived artifact".
#[tokio::test]
async fn the_artifacts_tab_shows_cards_and_an_opened_artifact_replaces_them_with_a_way_back() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    artifact(&app, &chat, "triage-summary.md", "# Triage\n\nAll sorted.").await;
    artifact(&app, &chat, "second.md", "# Second").await;
    archived(&app, &chat).await;

    let cards = text(
        app.get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT&tab=artifacts",
            &cookie,
        )
        .await,
    )
    .await;
    let grid = between(&cards, "<ul class=\"grid", "</ul>");
    assert!(
        grid.contains("sm:grid-cols-2") && grid.contains("xl:grid-cols-3"),
        "several per row: {grid}"
    );
    assert_eq!(grid.matches("<li>").count(), 2, "{grid}");
    assert!(grid.contains("artifact=triage-summary.md"), "{grid}");

    let opened = text(
        app.get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT&tab=artifacts&artifact=triage-summary.md",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        opened.contains("<h1>Triage</h1>") || opened.contains("Triage"),
        "{opened}"
    );
    assert!(opened.contains("All sorted."), "{opened}");
    assert!(
        !opened.contains("<ul class=\"grid"),
        "the cards are replaced: {opened}"
    );
    let back = between(
        &opened,
        "class=\"flex min-h-11 items-center gap-1 border-b",
        "</a>",
    );
    assert!(back.contains("Artifacts"), "{back}");
    assert!(
        opened.contains("tab=artifacts"),
        "the way back leads to the cards: {opened}"
    );
}

#[tokio::test]
async fn the_modal_refuses_a_chat_that_is_not_archived_or_a_project_that_does_not_exist() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let open = app
        .get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT",
            &cookie,
        )
        .await;
    assert_eq!(open.status(), StatusCode::NOT_FOUND);
    let unknown = app.get_htmx("/chats/archive?project=NOPE", &cookie).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

// Scenario "Unarchive from the modal".
#[tokio::test]
async fn unarchiving_from_the_modal_opens_the_chat_in_the_chat_view() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    archived(&app, &chat).await;

    let modal = text(
        app.get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        modal.contains("hx-post=\"/chats/DF-1-CHAT/unarchive\""),
        "{modal}"
    );
    let response = app.post("/chats/DF-1-CHAT/unarchive", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        crate::testkit::header_str(&response, "hx-redirect"),
        "/chats/DF-1-CHAT"
    );
    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(page.contains("id=\"message-box\""), "{page}");
}
