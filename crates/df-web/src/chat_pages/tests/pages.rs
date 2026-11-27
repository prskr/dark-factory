//! The chat routes: opening a chat, the default project, the 404 between tasks and chats, and the
//! round trips of message, stop, rename, archive and unarchive.

use axum::http::StatusCode;
use df_core::TaskStatus;
use serde_json::json;

use super::{agent_says, between, idle_chat};
use crate::testkit::{
    TestApp, form, header_str, insert_run, new_chat, new_task, set_task_status, text,
};

// Scenario "Open a chat".
#[tokio::test]
async fn a_chat_opens_with_its_project_and_row_selected_and_its_transcript_shown() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    agent_says(&app, &chat, "Here is the plan.").await;

    let response = app.get("/chats/DF-1-CHAT", &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    let project = between(&html, "href=\"/chats?project=DF\"", ">");
    assert!(project.contains("aria-current=\"true\""), "{project}");
    let row = super::row(&html, "DF-1-CHAT");
    assert!(row.contains("aria-current=\"page\""), "{row}");
    assert!(html.contains("Here is the plan."), "the transcript: {html}");
    assert!(html.contains("hx-sse:connect=\"/chats/DF-1-CHAT/events?after="));
    assert!(html.contains("data-view=\"task\""));
}

// The chat shares the transcript component of the task page (`transcript-readability`).
#[tokio::test]
async fn a_chat_transcript_groups_tool_calls_and_shows_thinking_like_a_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let run: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1 ORDER BY number LIMIT 1")
            .bind(chat.id.as_uuid())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let events = [
        json!({"agentThoughtChunk": {"messageId": "th", "content": {"text": {"text": "Look at the chart"}}}}),
        json!({"toolCall": {"toolCallId": "c1", "title": "Read Chart.yaml"}}),
        json!({"toolCall": {"toolCallId": "c2", "title": "Read values.yaml"}}),
    ];
    for (seq, event) in (1..).zip(events) {
        crate::testkit::add_event(&app.pool, chat.id, run, seq, "turn_1", event).await;
    }

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(
        html.contains("2 tool calls") && html.contains("data-tool-group"),
        "{html}"
    );
    assert!(
        html.contains("Thinking") && html.contains("Look at the chart"),
        "{html}"
    );
}

// Scenario "Default project".
#[tokio::test]
async fn chats_without_a_project_open_the_project_of_the_chat_active_last() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = crate::testkit::seed(&app.pool).await;
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES (gen_random_uuid(), 'WEB', 'Web', 'https://g/w.git', 'img', 'main')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    new_chat(&app.pool, &seed, "In Dark Factory").await;
    let web: uuid::Uuid = sqlx::query_scalar("SELECT id FROM projects WHERE key = 'WEB'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    df_db::chats::create_chat(
        &app.pool,
        df_db::chats::NewChat {
            project_id: df_core::ProjectId::from_uuid(web),
            persona_id: df_core::PersonaId::from_uuid(seed.persona),
            author: crate::testkit::actor().0,
            text: "In the web project".into(),
        },
    )
    .await
    .unwrap();

    let html = text(app.get("/chats", &cookie).await).await;

    let selected = between(&html, "href=\"/chats?project=WEB\"", ">");
    assert!(selected.contains("aria-current=\"true\""), "{selected}");
    assert!(html.contains("In the web project"), "{html}");
    assert!(
        !html.contains("In Dark Factory"),
        "only the project's chats"
    );
}

#[tokio::test]
async fn without_a_chat_the_first_project_is_selected() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    crate::testkit::seed(&app.pool).await;

    let html = text(app.get("/chats", &cookie).await).await;

    let selected = between(&html, "href=\"/chats?project=DF\"", ">");
    assert!(selected.contains("aria-current=\"true\""), "{selected}");
}

#[tokio::test]
async fn task_routes_are_not_found_for_a_chat_key_and_chat_routes_for_a_task_key() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = crate::testkit::seed(&app.pool).await;
    new_task(&app.pool, &seed, "A task").await;
    new_chat(&app.pool, &seed, "A chat").await;
    let message = form(&[("message", "hello")]);

    for uri in [
        "/tasks/DF-2-CHAT",
        "/tasks/DF-2-CHAT/events",
        "/tasks/DF-2-CHAT/artifacts",
        "/chats/DF-1",
        "/chats/DF-1/events",
        "/chats/DF-1/artifacts",
    ] {
        let response = app.get(uri, &cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri}");
    }
    for uri in [
        "/tasks/DF-2-CHAT/messages",
        "/tasks/DF-2-CHAT/stop",
        "/tasks/DF-2-CHAT/done",
        "/tasks/DF-2-CHAT/cancel",
        "/chats/DF-1/messages",
        "/chats/DF-1/stop",
        "/chats/DF-1/archive",
        "/chats/DF-1/title",
    ] {
        let response = app.post(uri, &message, &cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "POST {uri}");
    }
}

#[tokio::test]
async fn a_message_to_a_chat_is_queued_from_the_board_member() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;

    let response = app
        .post(
            "/chats/DF-1-CHAT/messages",
            &form(&[("message", "And the CI?")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "message-queued");
    let queued = df_db::list_undelivered_messages(&app.pool, chat.id)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(page.contains("And the CI?"), "{page}");
    assert!(
        page.contains("hx-post=\"/chats/DF-1-CHAT/messages\""),
        "{page}"
    );
}

// Scenario "Reply instead of answer".
#[tokio::test]
async fn a_message_on_a_chat_with_a_pending_question_leaves_the_question_answerable() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let actor = crate::testkit::actor().0;
    let (question, _) = df_db::requests::raise_question(
        &app.pool,
        df_db::requests::Raise {
            task_id: chat.id,
            run_id: None,
            actor: &actor,
            blocking: true,
        },
        df_core::QuestionSpec::text_question("Which one?"),
    )
    .await
    .unwrap();

    let response = app
        .post(
            "/chats/DF-1-CHAT/messages",
            &form(&[("message", "Let me think about it")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let row = df_db::requests::get_request(&app.pool, question.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, df_db::requests::RequestState::Pending);
    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(page.contains("data-question="), "{page}");
    let list = text(app.get("/chats", &cookie).await).await;
    assert!(list.contains("aria-label=\"Needs you\""), "{list}");

    let answered = app
        .post(
            &format!("/requests/{}/answer", question.id),
            &form(&[("text", "Postgres")]),
            &cookie,
        )
        .await;
    assert_eq!(answered.status(), StatusCode::OK);
    let list = text(app.get("/chats", &cookie).await).await;
    assert!(!list.contains("aria-label=\"Needs you\""), "{list}");
}

// Scenario "Message to an archived chat".
#[tokio::test]
async fn a_message_to_an_archived_chat_is_rejected_and_nothing_is_queued() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    df_db::chats::archive_chat(&app.pool, chat.id, crate::testkit::actor().0)
        .await
        .unwrap();

    let response = app
        .post(
            "/chats/DF-1-CHAT/messages",
            &form(&[("message", "Hello?")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(text(response).await.contains("archived"));
    let queued = df_db::list_undelivered_messages(&app.pool, chat.id)
        .await
        .unwrap();
    assert!(queued.is_empty());
}

// Scenario "Stop".
#[tokio::test]
async fn stopping_a_chat_stops_its_active_run() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    set_task_status(&app.pool, chat.id, TaskStatus::Running).await;
    insert_run(&app.pool, chat.id, 2, "running").await;

    let response = app.post("/chats/DF-1-CHAT/stop", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let stopped: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM runs WHERE number = 2 AND stop_requested_at IS NOT NULL",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(stopped, 1);
}

// Scenario "Renamed by a board member".
#[tokio::test]
async fn renaming_a_chat_sets_the_topic_and_locks_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;

    let response = app
        .post(
            "/chats/DF-1-CHAT/title",
            &form(&[("title", "CRD migration")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("CRD migration"));
    let changed = df_db::set_task_title(
        &app.pool,
        chat.id,
        "Agent topic",
        df_core::TitleSource::Agent,
    )
    .await
    .unwrap();
    assert!(!changed, "a later agent title does not change it");

    let empty = app
        .post("/chats/DF-1-CHAT/title", &form(&[("title", "  ")]), &cookie)
        .await;
    assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// Scenarios "Archive an idle chat" and "Archive while running".
#[tokio::test]
async fn archiving_an_idle_chat_goes_back_to_the_list_and_a_running_chat_is_refused() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    set_task_status(&app.pool, chat.id, TaskStatus::Running).await;
    let refused = app.post("/chats/DF-1-CHAT/archive", "", &cookie).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert!(text(refused).await.contains("id=\"chat-overview\""));

    set_task_status(&app.pool, chat.id, TaskStatus::Waiting).await;
    let archived = app.post("/chats/DF-1-CHAT/archive", "", &cookie).await;

    assert_eq!(archived.status(), StatusCode::OK);
    assert_eq!(header_str(&archived, "hx-redirect"), "/chats?project=DF");
    let list = text(app.get("/chats?project=DF", &cookie).await).await;
    assert!(!list.contains("<li id=\"row-DF-1-CHAT\""), "{list}");

    let unarchived = app.post("/chats/DF-1-CHAT/unarchive", "", &cookie).await;
    assert_eq!(header_str(&unarchived, "hx-redirect"), "/chats/DF-1-CHAT");
    let list = text(app.get("/chats?project=DF", &cookie).await).await;
    assert!(list.contains("<li id=\"row-DF-1-CHAT\""), "{list}");
}

#[tokio::test]
async fn an_archived_chat_page_has_no_message_box_but_an_unarchive_action() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    df_db::chats::archive_chat(&app.pool, chat.id, crate::testkit::actor().0)
        .await
        .unwrap();

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(!html.contains("id=\"message-box\""), "{html}");
    assert!(
        html.contains("id=\"archived-notice\"")
            && html.contains("hx-post=\"/chats/DF-1-CHAT/unarchive\"")
    );
}

#[tokio::test]
async fn there_is_no_way_to_delete_a_chat() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(
        !page.to_lowercase().contains("delete chat")
            && !page.contains("hx-delete=\"/chats/DF-1-CHAT\"")
    );
    let delete = app.delete("/chats/DF-1-CHAT", &cookie).await;
    assert_eq!(delete.status(), StatusCode::METHOD_NOT_ALLOWED);
}
