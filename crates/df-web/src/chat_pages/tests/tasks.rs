//! Chats next to tasks: the task list shows none, a task of a chat links to it and requests of a
//! chat link to the chat page.

use df_core::{PersonaId, Principal, ProjectId};
use df_db::requests::{Raise, raise_question};

use super::{between, idle_chat};
use crate::testkit::{TestApp, new_task, text};

// Scenario "Chat in the project".
#[tokio::test]
async fn the_task_list_and_its_counts_contain_only_tasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, _) = idle_chat(&app).await;
    new_task(&app.pool, &seed, "A task").await;

    let html = text(app.get("/tasks", &cookie).await).await;

    assert!(html.contains("<li id=\"row-DF-2\""), "{html}");
    assert!(
        !html.contains("row-DF-1-CHAT") && !html.contains("Plan the Helm chart release"),
        "{html}"
    );
    let project = between(
        &html,
        "<span class=\"truncate\">Dark Factory</span>",
        "</a>",
    );
    assert!(
        project.contains("DF · 1"),
        "the count has no chat: {project}"
    );
}

// Scenario "Task created in a chat".
#[tokio::test]
async fn a_task_of_a_chat_links_its_parent_to_the_chat_page() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, chat) = idle_chat(&app).await;
    df_db::set_task_title(
        &app.pool,
        chat.id,
        "Task dependencies plan",
        df_core::TitleSource::Agent,
    )
    .await
    .unwrap();
    df_db::create_task(
        &app.pool,
        df_db::NewTask {
            project_id: ProjectId::from_uuid(seed.project),
            persona_id: PersonaId::from_uuid(seed.persona),
            title: Some("Implement".into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: Some(chat.id),
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;

    let parent = between(&html, "<a href=\"/chats/DF-1-CHAT\"", "</a>");
    assert!(
        parent.contains("DF-1-CHAT · Linus · Task dependencies plan"),
        "{parent}"
    );
}

// Scenario "Approval request from a chat".
#[tokio::test]
async fn a_request_of_a_chat_is_in_the_inbox_and_links_to_the_chat_page() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let task = new_task(&app.pool, &seed_of(&app).await, "A task").await;
    let actor = Principal::system("test");
    for id in [chat.id, task.id] {
        raise_question(
            &app.pool,
            Raise {
                task_id: id,
                run_id: None,
                actor: &actor,
                blocking: false,
            },
            &format!("Question of {id}"),
            &[],
        )
        .await
        .unwrap();
    }

    let html = text(app.get("/requests", &cookie).await).await;

    assert!(html.contains("<a href=\"/chats/DF-1-CHAT\""), "{html}");
    assert!(html.contains("<a href=\"/tasks/DF-2\""), "{html}");
    assert!(!html.contains("href=\"/tasks/DF-1-CHAT\""), "{html}");
    // The chat's page renders the request card in its transcript.
    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(page.contains("Question of"), "{page}");
}

async fn seed_of(app: &TestApp) -> crate::testkit::Seed {
    crate::testkit::Seed {
        project: sqlx::query_scalar("SELECT id FROM projects LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        persona: sqlx::query_scalar("SELECT id FROM personas LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        provider: sqlx::query_scalar("SELECT id FROM model_providers LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
    }
}
