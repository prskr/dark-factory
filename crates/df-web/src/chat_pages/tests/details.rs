//! The center and the details of a chat: header, transcript rows, the tabs and the artifact split
//! view.

use df_core::{PersonaId, Principal, ProjectId, TaskStatus};
use df_db::requests::{Raise, raise_question};
use serde_json::json;

use super::{between, idle_chat};
use crate::testkit::{TestApp, add_event, new_task, seed, set_task_status, text};

/// A subtask of the chat.
async fn subtask(
    app: &TestApp,
    seed: &crate::testkit::Seed,
    chat: &df_db::TaskRow,
    title: &str,
) -> df_db::TaskRow {
    df_db::create_task(
        &app.pool,
        df_db::NewTask {
            project_id: ProjectId::from_uuid(seed.project),
            persona_id: PersonaId::from_uuid(seed.persona),
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: Some(chat.id),
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap()
    .0
}

async fn artifact(app: &TestApp, chat: &df_db::TaskRow, name: &str) {
    sqlx::query(
        "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, content, sha256)
         VALUES ($1, $2, 'text', 'text/markdown', 5, convert_to('# Hi', 'UTF8'), $3)",
    )
    .bind(chat.id.as_uuid())
    .bind(name)
    .bind(format!("{:0>64}", name.len()))
    .execute(&app.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn the_header_of_the_center_has_only_the_action_new_chat() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let head = between(&html, "<header id=\"chat-head\"", "</header>");
    let new_chat = between(head, "hx-get=\"/chats/compose?project=DF\"", "</button>");
    assert!(
        new_chat.contains("New chat") && new_chat.contains("max-md:hidden"),
        "{head}"
    );
    assert!(
        !head.contains("Rename") && !head.contains("Archive"),
        "{head}"
    );
}

// Scenario "Delegation in the transcript".
#[tokio::test]
async fn a_create_task_call_shows_the_key_title_and_persona_of_the_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let run: uuid::Uuid = sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1")
        .bind(chat.id.as_uuid())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    add_event(
        &app.pool,
        chat.id,
        run,
        1,
        "turn_1",
        json!({"toolCall": {"toolCallId": "c1", "title": "mcp__factory__create_task",
            "kind": "TOOL_KIND_OTHER", "status": "TOOL_CALL_STATUS_COMPLETED",
            "rawInputJson": json!({"persona": "Linus", "title": "Implement dependencies",
                "prompt": "Do it."}).to_string()}}),
    )
    .await;
    add_event(
        &app.pool,
        chat.id,
        run,
        2,
        "turn_1",
        json!({"toolCallUpdate": {"toolCallId": "c1",
            "rawOutputJson": "\"Created DF-20 with status queued. End your turn.\""}}),
    )
    .await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(
        html.contains("Created DF-20 · Implement dependencies · Linus"),
        "{html}"
    );
}

// Scenario "Expand the summary".
#[tokio::test]
async fn the_compaction_notice_is_one_line_expandable_to_the_summary() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    let run: uuid::Uuid = sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1")
        .bind(chat.id.as_uuid())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    add_event(
        &app.pool,
        chat.id,
        run,
        1,
        "turn_1",
        json!({"notice": {"level": "NOTICE_LEVEL_INFO", "code": "context_compacted",
            "message": "We agreed on the Helm chart layout.\nOpen: values schema."}}),
    )
    .await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let notice = between(&html, "<details id=\"ev-", "</details>");
    assert!(
        notice.contains("context_compacted") && notice.contains("<summary"),
        "{notice}"
    );
    assert!(
        notice.contains("We agreed on the Helm chart layout."),
        "{notice}"
    );
    assert!(notice.contains("Open: values schema."), "{notice}");
}

#[tokio::test]
async fn the_overview_shows_agent_status_topic_project_persona_key_and_checkout() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, _) = idle_chat(&app).await;
    sqlx::query("UPDATE personas SET description = 'Plans and reviews releases' WHERE id = $1")
        .bind(seed.persona)
        .execute(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let overview = between(&html, "<div id=\"chat-overview\"", "id=\"runs\"");
    for needle in [
        "Agent",
        "Linus",
        "Plans and reviews releases",
        "Now",
        "Status",
        "Archive",
        "Topic",
        "Plan the Helm chart release",
        "Project",
        "Dark Factory",
        "Persona",
        "Claude Code",
        "claude-sonnet-5-5",
        "Key",
        "DF-1-CHAT",
        "Checkout",
        "main",
        "read-only",
    ] {
        assert!(overview.contains(needle), "{needle}: {overview}");
    }
    let archive = between(overview, "hx-post=\"/chats/DF-1-CHAT/archive\"", ">");
    assert!(
        !crate::testkit::has_disabled_attribute(archive),
        "{archive}"
    );
}

// Scenario "Archive disabled while running".
#[tokio::test]
async fn archive_is_disabled_with_its_reason_while_the_chat_runs_or_has_an_open_request() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    set_task_status(&app.pool, chat.id, TaskStatus::Running).await;

    let running = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    let button = between(&running, "hx-post=\"/chats/DF-1-CHAT/archive\"", ">");
    assert!(crate::testkit::has_disabled_attribute(button), "{button}");
    assert!(
        running.contains("A running chat cannot be archived"),
        "{running}"
    );

    set_task_status(&app.pool, chat.id, TaskStatus::Waiting).await;
    let actor = crate::testkit::actor().0;
    raise_question(
        &app.pool,
        Raise {
            task_id: chat.id,
            run_id: None,
            actor: &actor,
            blocking: false,
        },
        "Which one?",
        &[],
    )
    .await
    .unwrap();
    let asked = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    let button = between(&asked, "hx-post=\"/chats/DF-1-CHAT/archive\"", ">");
    assert!(crate::testkit::has_disabled_attribute(button), "{button}");
    assert!(
        asked.contains("The chat has an open request; answer it before archiving"),
        "{asked}"
    );
}

// Scenario "Subtasks tab".
#[tokio::test]
async fn the_subtasks_tab_lists_the_subtasks_with_the_progress() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, chat) = idle_chat(&app).await;
    let running = subtask(&app, &seed, &chat, "Implement").await;
    set_task_status(&app.pool, running.id, TaskStatus::Running).await;
    let waiting = subtask(&app, &seed, &chat, "Review").await;
    set_task_status(&app.pool, waiting.id, TaskStatus::AwaitingDependencies).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let tab = between(&html, "id=\"tab-subtasks\"", "</button>");
    assert!(tab.contains(">2<"), "{tab}");
    let panel = between(&html, "id=\"panel-subtasks\"", "id=\"panel-artifacts\"");
    for needle in [
        "0 of 2 done",
        "DF-2",
        "Implement",
        "running",
        "DF-3",
        "Review",
        "awaiting dependencies",
    ] {
        assert!(panel.contains(needle), "{needle}: {panel}");
    }
}

#[tokio::test]
async fn the_tabs_are_always_overview_subtasks_and_artifacts_with_their_counts() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let at = |marker: &str| html.find(marker).unwrap_or_else(|| panic!("{marker}"));
    assert!(
        at("id=\"tab-overview\"") < at("id=\"tab-subtasks\"")
            && at("id=\"tab-subtasks\"") < at("id=\"tab-artifacts\"")
    );
    assert!(between(&html, "id=\"tab-subtasks\"", "</button>").contains(">0<"));
    assert!(between(&html, "id=\"tab-artifacts\"", "</button>").contains(">0<"));
}

// Scenario "Many artifacts".
#[tokio::test]
async fn the_artifacts_tab_counts_the_artifacts_and_scrolls_its_cards() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    for n in 1..=7 {
        artifact(&app, &chat, &format!("note-{n}.md")).await;
    }

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    let tab = between(&html, "id=\"tab-artifacts\"", "</button>");
    assert!(tab.contains(">7<"), "{tab}");
    let panel = between(&html, "id=\"panel-artifacts\"", ">");
    assert!(
        panel.contains("overflow-y-auto") && panel.contains("min-h-0"),
        "{panel}"
    );
    assert_eq!(
        html.matches("href=\"/chats/DF-1-CHAT/artifacts/note-")
            .count(),
        7
    );
    // The tab bar is in the head, which stays above the scrolling body.
    assert!(html.find("ws-details-head").unwrap() < html.find("ws-details-body").unwrap());
    // The cards of a chat reload from the chat's own route.
    assert!(
        panel.contains("hx-get=\"/chats/DF-1-CHAT/artifacts\""),
        "{panel}"
    );
}

// Scenarios "Open from the transcript" (server side).
#[tokio::test]
async fn an_artifact_of_a_chat_opens_in_the_split_view_from_the_chat_route() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    artifact(&app, &chat, "plan.md").await;

    let page = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;
    assert!(page.contains("artifactSplit("), "{page}");
    assert!(
        page.contains("&quot;path&quot;:&quot;/chats/DF-1-CHAT&quot;"),
        "{page}"
    );
    assert!(page.contains("id=\"artifact-pane\""), "{page}");

    let view = text(
        app.get_htmx("/chats/DF-1-CHAT/artifacts/plan.md", &cookie)
            .await,
    )
    .await;
    assert!(
        view.contains("plan.md") && view.contains("Close artifact"),
        "{view}"
    );
    let full = text(app.get("/chats/DF-1-CHAT/artifacts/plan.md", &cookie).await).await;
    assert!(full.contains("href=\"/chats/DF-1-CHAT\""), "{full}");
    let cards = text(app.get_htmx("/chats/DF-1-CHAT/artifacts", &cookie).await).await;
    assert!(
        cards.contains("id=\"artifact-cards\"") && cards.contains("id=\"ws-details-heading\""),
        "{cards}"
    );
}

// Scenario "Collapse details".
#[tokio::test]
async fn the_details_collapse_to_a_strip_that_shows_them_again() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats/DF-1-CHAT", &cookie).await).await;

    assert!(html.contains("aria-label=\"Hide details\"") && html.contains("setCollapsed(true)"));
    let strip = between(&html, "class=\"ws-strip", "</div>");
    assert!(
        strip.contains("aria-label=\"Show details\"") && strip.contains("setCollapsed(false)"),
        "{strip}"
    );
    assert!(html.contains(":data-collapsed=\"collapsed\""), "{html}");
}

#[tokio::test]
async fn a_task_page_next_to_a_chat_is_unchanged_by_the_chat_regions() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "A task").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(html.contains("id=\"task-head\"") && !html.contains("id=\"chat-head\""));
    assert!(html.contains("hx-post=\"/tasks/DF-1/messages\""), "{html}");
}
