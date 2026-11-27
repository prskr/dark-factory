//! The Alpine components of the chat pages: their sources and their registration.

use super::idle_chat;
use crate::assets::script_source;
use crate::layout::COMPONENT_SCRIPTS;
use crate::testkit::{TestApp, text};

#[test]
fn the_components_are_registered_with_the_layout() {
    assert!(COMPONENT_SCRIPTS.contains(&"chatLayout.js"));
    assert!(COMPONENT_SCRIPTS.contains(&"archiveModal.js"));
}

// Scenario "Collapse details" and the persona strip: the state transitions of `chatLayout`.
#[test]
fn chat_layout_collapses_both_columns_and_remembers_them_in_this_browser_only() {
    let js = script_source("chatLayout.ts");
    assert!(js.contains("Alpine.data('chatLayout'"), "{js}");
    for method in [
        "setCollapsed(value",
        "setListCollapsed(value",
        "select(row",
        "sheetToggle()",
        "sheetDragStart(",
        "sheetDragEnd(",
        "sheetDragCancel(",
    ] {
        assert!(js.contains(method), "{method}");
    }
    // The choices are separate from the task workspace's, and every storage access is guarded.
    assert!(js.contains("df.chat.details.collapsed") && js.contains("df.chat.list.collapsed"));
    assert!(!js.contains("'df.details.collapsed'") && !js.contains("'df.list.collapsed'"));
    let accesses = js.matches("localStorage.").count();
    assert_eq!(accesses, 4, "two reads and two writes");
    assert_eq!(
        js.matches("try {").count(),
        accesses,
        "each inside try/catch"
    );
    // Collapsing the details is for 1280 px and more; `collapsed` and `details` are the fields the
    // artifact split view toggles on `.ws`.
    assert!(js.contains("collapsed: false") && js.contains("details: false"));
    assert!(js.contains("listCollapsed: false"));
}

#[test]
fn the_archive_modal_component_keeps_which_screen_a_phone_shows() {
    let js = script_source("archiveModal.ts");
    assert!(js.contains("Alpine.data('archiveModal'"), "{js}");
    assert!(js.contains("chat: cfg.selected"), "{js}");
}

#[tokio::test]
async fn the_chat_page_is_a_chat_layout_with_the_artifact_split_inside() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(html.contains("x-data=\"chatLayout\""), "{html}");
    assert!(
        html.contains(":data-list-collapsed=\"listCollapsed\""),
        "{html}"
    );
    assert!(html.contains("x-data=\"artifactSplit("), "{html}");
}

#[test]
fn the_artifact_split_takes_the_page_of_the_chat_or_task() {
    let js = script_source("artifactSplit.ts");
    assert!(js.contains("`${cfg.path}/artifacts/"), "{js}");
}

#[tokio::test]
async fn chat_rows_select_themselves_and_hand_focus_to_the_head_after_loading() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, _chat) = idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(html.contains("<div id=\"chat-rows\" data-rows>"), "{html}");
    let row = super::row(&html, "DF-1-CHAT");
    assert!(row.contains("x-on:click=\"select($el)\""), "{row}");
    assert!(!row.contains("querySelectorAll"), "{row}");
    assert!(
        row.contains("data-focus-after-swap=\"#chat-head\""),
        "{row}"
    );
}

#[test]
fn both_workspaces_clear_the_marked_row_of_their_own_list_only() {
    for source in ["workspace.ts", "chatLayout.ts"] {
        let js = script_source(source);
        assert!(js.contains("select(row"), "{source}");
        assert!(js.contains("closest('[data-rows]')"), "{source}");
        assert!(!js.contains("document.querySelectorAll"), "{source}");
    }
}
