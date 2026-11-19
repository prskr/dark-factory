//! The live streams of the chat pages: the chat list and the regions of a chat.

use std::time::Duration;

use axum::http::StatusCode;
use df_core::TaskStatus;

use super::idle_chat;
use crate::testkit::{SseReader, new_chat, set_task_status};

const LIVE: Duration = Duration::from_secs(3);

#[tokio::test]
async fn the_list_stream_sends_the_rows_first_and_again_when_a_chat_arrives() {
    let app = crate::testkit::TestApp::new().await;
    let cookie = app.login().await;
    let (seed, _) = idle_chat(&app).await;
    let mut stream = SseReader::new(app.get("/chats/events?project=DF", &cookie).await);

    let first = stream.next_within(LIVE).await;
    assert!(
        first.data.contains("hx-target=\"#chat-rows\""),
        "{}",
        first.data
    );
    assert!(first.data.contains("row-DF-1-CHAT") && !first.data.contains("row-DF-2-CHAT"));

    new_chat(&app.pool, &seed, "A second chat").await;
    let second = stream.next_within(LIVE).await;
    assert!(second.data.contains("row-DF-2-CHAT"), "{}", second.data);
}

#[tokio::test]
async fn the_chat_stream_renders_the_chat_regions_and_follows_the_status() {
    let app = crate::testkit::TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let mut stream = SseReader::new(app.get("/chats/DF-1-CHAT/events?after=0", &cookie).await);

    let mut seen = String::new();
    while ![
        "#chat-head",
        "#chat-overview",
        "#runs",
        "#pending",
        "#ws-details-heading",
        "#panel-subtasks",
    ]
    .iter()
    .all(|region| seen.contains(&format!("hx-target=\"{region}\"")))
    {
        seen.push_str(&stream.next_within(LIVE).await.data);
    }
    assert!(
        !seen.contains("#task-head") && !seen.contains("#dependencies"),
        "{seen}"
    );

    set_task_status(&app.pool, chat.id, TaskStatus::Running).await;
    let changed = stream.next_within(LIVE).await;
    assert!(
        changed.data.contains("hx-target=\"#chat-overview\"") && changed.data.contains(">running<"),
        "{}",
        changed.data
    );
}

#[tokio::test]
async fn the_streams_are_not_found_for_the_wrong_kind_of_key() {
    let app = crate::testkit::TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let wrong = app.get("/tasks/DF-1-CHAT/events", &cookie).await;
    assert_eq!(wrong.status(), StatusCode::NOT_FOUND);
}
