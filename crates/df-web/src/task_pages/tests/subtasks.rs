//! The Subtasks tab of the details column, the base task link and the done/total chip of task
//! rows (`task-ui`, "Details tabs" and "Subtask progress on task rows").

use std::time::Duration;

use df_core::{PersonaId, Principal, ProjectId, TaskId, TaskStatus};
use df_db::TaskPlacement;

use crate::testkit::{
    Seed, SseReader, TestApp, new_task, row, seed, set_task_status, tag_around, text,
};

/// A task of the seeded project with the given parent and base task.
async fn create(
    app: &TestApp,
    seed: &Seed,
    title: &str,
    parent: Option<TaskId>,
    base: Option<&str>,
) -> df_db::TaskRow {
    df_db::create_task_on(
        &app.pool,
        df_db::NewTask {
            project_id: ProjectId::from_uuid(seed.project),
            persona_id: PersonaId::from_uuid(seed.persona),
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: parent,
            depends_on: Vec::new(),
        },
        TaskPlacement {
            base_task: base.map(|key| key.parse().unwrap()),
            ..TaskPlacement::default()
        },
    )
    .await
    .unwrap()
    .0
}

/// DF-1 with the subtasks DF-2 (`completed`) and DF-3 (`running`, with a subtask DF-4 of its
/// own).
async fn feature(app: &TestApp) -> Seed {
    let seed = seed(&app.pool).await;
    let parent = new_task(&app.pool, &seed, "Feature").await;
    let done = create(app, &seed, "First iteration", Some(parent.id), None).await;
    let running = create(app, &seed, "Second iteration", Some(parent.id), None).await;
    create(app, &seed, "Research", Some(running.id), None).await;
    set_task_status(&app.pool, done.id, TaskStatus::Completed).await;
    set_task_status(&app.pool, running.id, TaskStatus::Running).await;
    seed
}

/// The Subtasks panel of the page.
fn panel(html: &str) -> &str {
    let start = html.find(r#"id="panel-subtasks""#).expect("subtasks panel");
    let rest = &html[start..];
    &rest[..rest.find(r#"id="panel-artifacts""#).unwrap_or(rest.len())]
}

// Scenario "Feature task".
#[tokio::test]
async fn the_subtasks_tab_lists_the_direct_subtasks_with_status_and_progress() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    feature(&app).await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(html.contains(r#"id="tab-subtasks""#), "{html}");
    let panel = panel(&html);
    assert!(
        panel.contains("1 of 2 done") && panel.contains("50%"),
        "{panel}"
    );
    let bar = tag_around(panel, "role=\"progressbar\"");
    assert!(bar.contains("aria-valuenow=\"50\""), "{bar}");
    for needle in [
        "DF-2",
        "First iteration",
        "completed",
        "DF-3",
        "Second iteration",
        "running",
    ] {
        assert!(panel.contains(needle), "{needle}: {panel}");
    }
    // Only the direct subtasks: DF-4 belongs to DF-3.
    assert!(!panel.contains("Research"), "{panel}");
    // DF-3 has a subtask of its own: its chip.
    let row = &panel[panel.find("DF-3").unwrap()..];
    assert!(row.contains(r#"data-subtasks="0/1""#), "{row}");
    assert!(
        !panel[..panel.find("DF-3").unwrap()].contains("data-subtasks"),
        "{panel}"
    );
}

// Scenario "Task without subtasks".
#[tokio::test]
async fn a_task_without_subtasks_has_no_subtasks_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    feature(&app).await;

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;

    assert!(!html.contains(r#"id="tab-subtasks""#), "{html}");
    assert!(
        !html.contains(r#"role="tablist""#),
        "no tabs at all: {html}"
    );
}

#[tokio::test]
async fn the_tabs_are_overview_subtasks_and_artifacts_in_this_order() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    feature(&app).await;
    sqlx::query(
        "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, content, sha256)
         SELECT id, 'plan.md', 'text', 'text/markdown', 1, 'x', 'h' FROM tasks WHERE key = 'DF-1'",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(
        at(r#"id="tab-overview""#) < at(r#"id="tab-subtasks""#)
            && at(r#"id="tab-subtasks""#) < at(r#"id="tab-artifacts""#),
        "{html}"
    );
    let tab = tag_around(&html, r#"id="tab-subtasks""#);
    assert!(tab.contains(r#"aria-controls="panel-subtasks""#), "{tab}");
}

// Scenario "Base task link".
#[tokio::test]
async fn the_overview_links_the_base_task_below_the_parent() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let parent = new_task(&app.pool, &seed, "Coordinator").await;
    let implementation = create(&app, &seed, "Implement", Some(parent.id), None).await;
    create(
        &app,
        &seed,
        "Review",
        Some(parent.id),
        Some(&implementation.key.to_string()),
    )
    .await;

    let html = text(app.get("/tasks/DF-3", &cookie).await).await;

    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle}: {html}"))
    };
    assert!(at(">Parent<") < at(">Base task<"), "{html}");
    assert!(
        html.contains(
            r#"<a href="/tasks/DF-2" class="text-accent-text hover:underline">DF-2 · Implement</a>"#
        ),
        "{html}"
    );
    let plain = text(app.get("/tasks/DF-2", &cookie).await).await;
    assert!(!plain.contains(">Base task<"), "{plain}");
}

// Scenarios "Partial progress" and "No subtasks".
#[tokio::test]
async fn task_rows_show_the_done_and_total_chip_only_for_tasks_with_subtasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let parent = new_task(&app.pool, &seed, "Feature").await;
    for status in [
        TaskStatus::InReview,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ] {
        let child = create(&app, &seed, "Sub", Some(parent.id), None).await;
        set_task_status(&app.pool, child.id, status).await;
    }

    let html = text(app.get("/tasks?status=all&open=DF-1", &cookie).await).await;

    let feature_row = row(&html, "DF-1");
    assert!(
        feature_row.contains(r#"data-subtasks="2/3""#),
        "{feature_row}"
    );
    assert!(feature_row.contains(">2/3<"), "{feature_row}");
    // An open row lists its subtasks below it, without a chip of their own.
    assert!(!row(&html, "DF-2").contains("data-subtasks"));
}

#[tokio::test]
async fn a_task_row_shows_no_chip_when_every_subtask_is_cancelled() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let parent = new_task(&app.pool, &seed, "Feature").await;
    let child = create(&app, &seed, "Sub", Some(parent.id), None).await;
    set_task_status(&app.pool, child.id, TaskStatus::Cancelled).await;

    let html = text(app.get("/tasks?status=all", &cookie).await).await;

    assert!(!html.contains("data-subtasks"), "{html}");
}

const LIVE: Duration = Duration::from_secs(2);

// "both tabs SHALL update live": a subtask that changes status is sent to the parent's stream.
#[tokio::test]
async fn a_subtask_that_changes_updates_the_parents_details_stream() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    feature(&app).await;
    let mut sse = SseReader::new(app.get("/tasks/DF-1/events?after=0", &cookie).await);
    let first = sse.next().await;
    assert!(
        first.data.contains(r##"hx-target="#panel-subtasks""##),
        "{}",
        first.data
    );
    assert!(first.data.contains("1 of 2 done"), "{}", first.data);

    let running: TaskId = sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-3'")
        .fetch_one(&app.pool)
        .await
        .map(TaskId::from_uuid)
        .unwrap();
    // The notification the transition of DF-3 publishes for its parent.
    sqlx::query("UPDATE tasks SET status = 'in_review' WHERE id = $1")
        .bind(running.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let parent: TaskId = sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-1'")
        .fetch_one(&app.pool)
        .await
        .map(TaskId::from_uuid)
        .unwrap();
    df_db::publish(
        &app.pool,
        &df_db::Notification::TaskChanged { task_id: parent },
    )
    .await
    .unwrap();

    let frame = sse.next_within(LIVE).await;
    assert!(
        frame.data.contains(r##"hx-target="#panel-subtasks""##)
            && frame.data.contains("2 of 2 done"),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn the_task_list_stream_updates_the_chip() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    feature(&app).await;
    let mut sse = SseReader::new(app.get("/tasks/events?status=all", &cookie).await);
    let first = sse.next().await;
    assert!(
        first.data.contains(r#"data-subtasks="1/2""#),
        "{}",
        first.data
    );

    let running: TaskId = sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-3'")
        .fetch_one(&app.pool)
        .await
        .map(TaskId::from_uuid)
        .unwrap();
    set_task_status(&app.pool, running, TaskStatus::InReview).await;

    let frame = sse.next_within(LIVE).await;
    assert!(
        frame.data.contains(r#"data-subtasks="2/2""#),
        "{}",
        frame.data
    );
}
