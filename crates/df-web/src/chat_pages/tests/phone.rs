//! The phone layouts of the chat screens (below 768 px): the markup the stylesheet and the
//! components turn into full-screen pages and bottom sheets. The look is checked in a browser.

use super::{between, idle_chat};
use crate::testkit::{TestApp, text};

// Scenario "Chat list on a phone" (web-ui-shell) and the list screen of "Chats on phones".
#[tokio::test]
async fn the_chat_list_is_a_page_with_a_project_picker_the_chats_the_archive_and_new_chat() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats", &cookie).await).await;

    assert!(html.contains("data-view=\"list\""), "{html}");
    let list = between(&html, "<section class=\"ws-list\"", "</section>");
    // The project picker is a menu on phones and the project list a block from 768 px.
    let picker = between(list, "<div class=\"ml-auto md:hidden\"", "</div>");
    assert!(
        picker.contains("Dark Factory") && picker.contains("aria-haspopup"),
        "{picker}"
    );
    assert!(
        list.contains("<nav aria-label=\"Projects\" class=\"hidden md:block\""),
        "{list}"
    );
    assert!(
        list.contains("row-DF-1-CHAT") && list.contains("Archived chats"),
        "{list}"
    );
    let bar = between(&html, "id=\"mobile-menu\"", "</nav>");
    let current = &bar[bar.find("aria-current").unwrap()..];
    assert!(current.contains("Chats"), "{bar}");
    let floating = crate::testkit::tag_around(&html, "fixed bottom-20 right-4");
    assert!(floating.contains("aria-label=\"New chat\""), "{floating}");
}

// Scenario "Open a chat on a phone".
#[tokio::test]
async fn a_chat_fills_the_screen_with_persona_status_and_topic_a_way_back_and_the_message_box() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    crate::testkit::set_task_status(&app.pool, chat.id, df_core::TaskStatus::Running).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(html.contains("data-view=\"task\""), "{html}");
    let head = between(&html, "<header id=\"chat-head\"", "</header>");
    let back = between(head, "aria-label=\"Back to chats\"", ">");
    assert!(back.contains("md:hidden"), "{head}");
    assert!(head.contains("href=\"/chats?project=DF\""), "{head}");
    let identity = between(
        head,
        "<div class=\"flex min-w-0 flex-1 items-center gap-2 md:hidden\"",
        "Details\"",
    );
    for needle in ["Linus", "Plan the Helm chart release", ">running<"] {
        assert!(identity.contains(needle), "{needle}: {identity}");
    }
    assert!(html.contains("id=\"message-box\""), "{html}");
    // No bottom bar and no floating button on the transcript screen.
    let bar = between(&html, "id=\"mobile-menu\"", ">");
    assert!(
        bar.contains("[body[data-view=task]_&amp;]:hidden")
            || html.contains("[body[data-view=task]_&amp;]:hidden")
    );
}

#[tokio::test]
async fn the_details_open_as_a_bottom_sheet_with_the_three_tabs() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let details = between(&html, "<aside id=\"ws-details\"", "</aside>");
    assert!(
        details.contains("\":data-open\"") || details.contains(":data-open=\"details\""),
        "{details}"
    );
    assert!(
        details.contains("aria-label=\"Resize details\"") && details.contains("md:hidden"),
        "{details}"
    );
    for tab in ["Overview", "Subtasks", "Artifacts"] {
        assert!(details.contains(tab), "{tab}: {details}");
    }
    let button = between(
        &html,
        "aria-label=\"Details\" x-on:click=\"details = true; sheet = 'half'\"",
        ">",
    );
    assert!(button.contains("xl:hidden"), "{button}");
}

// Scenario "Artifact on a phone".
#[tokio::test]
async fn an_artifact_opens_full_screen_with_a_way_back_to_the_chat() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    sqlx::query(
        "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, content, sha256)
         VALUES ($1, 'plan.md', 'text', 'text/markdown', 4, convert_to('# Hi', 'UTF8'), $2)",
    )
    .bind(chat.id.as_uuid())
    .bind(format!("{:0>64}", 1))
    .execute(&app.pool)
    .await
    .unwrap();

    let split = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    // Below 768 px the split view is hidden and the script opens the artifact page instead.
    assert!(between(&split, "id=\"artifact-pane\"", ">").contains("max-md:hidden"));
    assert!(
        crate::assets::script_source("artifactSplit.ts")
            .contains("window.location.assign(this.url(name))")
    );

    let page = text(app.get("/chats/DF-1-CHAT/artifacts/plan.md", &cookie).await).await;
    let back = between(
        &page,
        "<nav class=\"flex items-center gap-2 px-4 py-3 text-sm\"",
        "</nav>",
    );
    assert!(back.contains("href=\"/chats/DF-1-CHAT\""), "{back}");
    assert!(
        page.contains("max-w-3xl") && page.contains("min-h-dvh"),
        "{page}"
    );
}

#[tokio::test]
async fn new_chat_and_the_archive_are_a_bottom_sheet_and_a_full_screen_list() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    df_db::chats::archive_chat(&app.pool, chat.id, crate::testkit::actor().0)
        .await
        .unwrap();

    let compose = text(app.get_htmx("/chats/compose?project=DF", &cookie).await).await;
    let dialog = between(&compose, "<dialog id=\"composer\"", ">");
    assert!(
        dialog.contains("fixed inset-x-0 bottom-0"),
        "a bottom sheet: {dialog}"
    );

    let archive = text(app.get_htmx("/chats/archive?project=DF", &cookie).await).await;
    let dialog = between(&archive, "<dialog id=\"archive\"", ">");
    assert!(
        dialog.contains("fixed inset-0") && dialog.contains("h-dvh"),
        "full screen: {dialog}"
    );
    let chat = text(
        app.get_htmx(
            "/chats/archive?project=DF&part=body&chat=DF-1-CHAT",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        chat.contains("aria-label=\"Back to the archived chats\""),
        "{chat}"
    );
    assert!(
        chat.contains("max-md:hidden"),
        "the list gives way to the chat: {chat}"
    );
}
