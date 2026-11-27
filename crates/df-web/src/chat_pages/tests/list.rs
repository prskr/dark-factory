//! The project and chat list: badges, the "needs you" marker and order, the topic placeholder, the
//! archived entry, the persona strip and the empty state.

use df_core::TaskStatus;
use df_db::requests::{Raise, raise_question};

use super::{agent_says, between, idle_chat, row};
use crate::testkit::{TestApp, add_persona, new_chat, new_chat_with, set_task_status, text};

async fn needs_you(app: &TestApp, chat: &df_db::TaskRow) {
    let actor = crate::testkit::actor().0;
    raise_question(
        &app.pool,
        Raise {
            task_id: chat.id,
            run_id: None,
            actor: &actor,
            blocking: false,
        },
        df_core::QuestionSpec::text_question("Which Helm version?"),
    )
    .await
    .unwrap();
}

// Scenario "Idle chat without badge".
#[tokio::test]
async fn an_idle_chat_shows_no_status_badge() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;

    let html = text(app.get("/chats", &cookie).await).await;

    let row = row(&html, "DF-1-CHAT");
    assert!(row.contains("Linus"), "{row}");
    for badge in ["waiting", "running", "queued"] {
        assert!(!row.contains(&format!(">{badge}<")), "{badge}: {row}");
    }
    assert!(!row.contains("Needs you"), "{row}");
}

// Scenario "Running chat".
#[tokio::test]
async fn a_running_chat_shows_the_running_badge_and_its_last_message() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (_, chat) = idle_chat(&app).await;
    agent_says(&app, &chat, "Checking the chart values.").await;
    df_db::set_task_title(
        &app.pool,
        chat.id,
        "Helm chart",
        df_core::TitleSource::Agent,
    )
    .await
    .unwrap();
    set_task_status(&app.pool, chat.id, TaskStatus::Running).await;

    let html = text(app.get("/chats", &cookie).await).await;

    let row = row(&html, "DF-1-CHAT");
    assert!(row.contains(">running<"), "{row}");
    assert!(row.contains("Checking the chart values."), "{row}");
}

// Scenario "Topic not set yet".
#[tokio::test]
async fn a_chat_without_a_topic_shows_the_first_message_in_italics_and_who_sets_the_topic() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = crate::testkit::seed(&app.pool).await;
    new_chat(&app.pool, &seed, "Plan the Helm chart release").await;

    let html = text(app.get("/chats", &cookie).await).await;

    let row = row(&html, "DF-1-CHAT");
    let topic = between(row, "<div title=\"Plan the Helm chart release\"", "</div>");
    assert!(topic.contains("italic"), "{topic}");
    assert!(
        row.contains("Linus sets the topic when the run starts"),
        "{row}"
    );
    assert!(row.contains(">queued<"), "{row}");
}

// Scenario "Open approval request".
#[tokio::test]
async fn a_chat_that_needs_you_is_listed_first_with_the_dot_and_marks_its_project() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, quiet) = idle_chat(&app).await;
    let needs = new_chat(&app.pool, &seed, "Needs an answer").await;
    set_task_status(&app.pool, needs.id, TaskStatus::Waiting).await;
    let recent = new_chat(&app.pool, &seed, "Most recent").await;
    set_task_status(&app.pool, recent.id, TaskStatus::Waiting).await;
    needs_you(&app, &needs).await;
    agent_says(&app, &quiet, "Latest activity").await;

    let html = text(app.get("/chats", &cookie).await).await;

    let rows = between(&html, "<div id=\"chat-rows\"", "</ul>");
    let order: Vec<_> = rows
        .split("<li id=\"row-")
        .skip(1)
        .map(|r| &r[..r.find('"').unwrap()])
        .collect();
    assert_eq!(order, ["DF-2-CHAT", "DF-1-CHAT", "DF-3-CHAT"], "{rows}");
    let first = row(&html, "DF-2-CHAT");
    assert!(first.contains("aria-label=\"Needs you\""), "{first}");
    assert!(
        !first.contains(">waiting<") && !first.contains(">queued<"),
        "no badge: {first}"
    );
    let projects = between(&html, "<nav aria-label=\"Projects\"", "</nav>");
    let project = between(projects, "href=\"/chats?project=DF\"", "</a>");
    assert!(project.contains("aria-label=\"Needs you\""), "{project}");
}

#[tokio::test]
async fn projects_show_their_chat_counts_and_the_archive_entry_its_own() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, archived) = idle_chat(&app).await;
    new_chat(&app.pool, &seed, "Open one").await;
    df_db::chats::archive_chat(&app.pool, archived.id, crate::testkit::actor().0)
        .await
        .unwrap();

    let html = text(app.get("/chats", &cookie).await).await;

    let projects = between(&html, "<nav aria-label=\"Projects\"", "</nav>");
    let project = between(projects, "href=\"/chats?project=DF\"", "</a>");
    assert!(
        project.contains("Dark Factory") && project.contains(">1<"),
        "{project}"
    );
    assert!(
        !html.contains("<li id=\"row-DF-1-CHAT\""),
        "archived chats are not listed"
    );
    let entry = between(&html, "hx-get=\"/chats/archive?project=DF\"", "</button>");
    assert!(
        entry.contains("Archived chats") && entry.contains(">1<"),
        "{entry}"
    );
    assert!(html.contains("hx-sse:connect=\"/chats/events?project=DF\""));
}

// Scenario "Persona with a chat that needs you".
#[tokio::test]
async fn the_collapsed_strip_links_each_persona_to_its_chat_that_needs_you_else_its_latest() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let (seed, latest) = idle_chat(&app).await;
    let grace = add_persona(&app.pool, &seed, "Grace", "Plans the release").await;
    let needing = new_chat(&app.pool, &seed, "Linus needs you").await;
    set_task_status(&app.pool, needing.id, TaskStatus::Waiting).await;
    needs_you(&app, &needing).await;
    let other = new_chat_with(&app.pool, &seed, grace, "Grace chat").await;
    set_task_status(&app.pool, other.id, TaskStatus::Waiting).await;
    agent_says(&app, &latest, "Newer than the one that needs you").await;

    let html = text(app.get("/chats", &cookie).await).await;

    let strip = between(&html, "class=\"ws-list-strip", "</section>");
    let linus = between(strip, "aria-label=\"Chat with Linus\"", ">");
    assert!(linus.contains("title=\"Linus\""), "{linus}");
    let at = strip.find("aria-label=\"Chat with Linus\"").unwrap();
    assert!(
        strip[..at].ends_with("<a href=\"/chats/DF-2-CHAT\" "),
        "{strip}"
    );
    assert!(
        strip.contains("<a href=\"/chats/DF-3-CHAT\" aria-label=\"Chat with Grace\""),
        "{strip}"
    );
    assert_eq!(
        strip.matches("aria-label=\"Chat with ").count(),
        2,
        "{strip}"
    );
    assert!(html.contains("setListCollapsed(true)"), "{html}");
}

// Scenario "No chats yet".
#[tokio::test]
async fn a_project_without_chats_shows_the_empty_state_with_new_chat() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    idle_chat(&app).await;
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES (gen_random_uuid(), 'DOC', 'Docs', 'https://g/d.git', 'img', 'main')",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let html = text(app.get("/chats?project=DOC", &cookie).await).await;

    assert!(html.contains("No chats in Docs yet"), "{html}");
    let action = between(&html, "hx-get=\"/chats/compose?project=DOC\"", "</button>");
    assert!(action.contains("New chat"), "{action}");
    assert!(!html.contains("id=\"chat-rows\"><ul></ul><button") || html.contains("Archived chats"));
}

#[tokio::test]
async fn the_list_has_no_chat_when_no_project_exists() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get("/chats", &cookie).await).await;

    assert!(
        html.contains("No projects yet") && html.contains("href=\"/projects\""),
        "{html}"
    );
}
