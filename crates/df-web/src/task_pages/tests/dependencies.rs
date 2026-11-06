//! Creator, parent and the Dependencies section of the details column.

use df_core::{DependencyKind, PersonaId, Principal, ProjectId, TaskId, TaskStatus};
use df_db::NewDependency;

use crate::testkit::{Seed, TestApp, new_task, seed, set_task_status, text};

async fn create(
    app: &TestApp,
    seed: &Seed,
    title: &str,
    created_by: Principal,
    parent: Option<TaskId>,
    depends_on: &[(&str, DependencyKind)],
) -> df_db::TaskRow {
    let depends_on = depends_on
        .iter()
        .map(|(key, kind)| NewDependency {
            key: key.parse().unwrap(),
            kind: *kind,
        })
        .collect();
    df_db::create_task(
        &app.pool,
        df_db::NewTask {
            project_id: ProjectId::from_uuid(seed.project),
            persona_id: PersonaId::from_uuid(seed.persona),
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by,
            parent_id: parent,
            depends_on,
        },
    )
    .await
    .unwrap()
    .0
}

fn system() -> Principal {
    Principal::system("test")
}

/// The part of the page that is the Dependencies section.
fn section(html: &str) -> &str {
    let start = html
        .find(r#"id="dependencies""#)
        .expect("dependencies region");
    let rest = &html[start..];
    &rest[..rest.find(r#"id="runs""#).unwrap_or(rest.len())]
}

#[tokio::test]
async fn an_agent_created_task_shows_its_creator_and_parent_after_the_branch() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let ceo = new_task(&app.pool, &seed, "Plan the release").await;
    let agent = Principal::agent(PersonaId::from_uuid(seed.persona), "CEO", None, ceo.id);
    create(&app, &seed, "Review DF-1", agent, Some(ceo.id), &[]).await;

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;

    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle}: {html}"))
    };
    assert!(at(">Branch<") < at(">Created by<") && at(">Created by<") < at(">Parent<"));
    assert!(
        html.contains(
            r#"<a href="/tasks/DF-1" class="text-accent-text hover:underline">DF-1</a> · CEO"#
        ),
        "{html}"
    );
    assert!(html.contains("Plan the release"), "{html}");
}

#[tokio::test]
async fn a_board_member_created_task_shows_the_member_and_no_parent() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let member = Principal::human(df_core::UserId::new(), "Ada Lovelace");
    create(&app, &seed, "By hand", member, None, &[]).await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(
        html.contains(">Created by<") && html.contains("Ada Lovelace"),
        "{html}"
    );
    assert!(!html.contains(">Parent<"), "{html}");
}

#[tokio::test]
async fn a_task_in_no_dependency_has_no_section() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "alone").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(!html.contains("Dependencies"), "{html}");
}

#[tokio::test]
async fn the_section_lists_dependants_with_kind_and_status() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "target").await;
    create(
        &app,
        &seed,
        "Follow-up",
        system(),
        None,
        &[("DF-1", DependencyKind::Done)],
    )
    .await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    let section = section(&html);
    assert!(
        section.contains("Needed by") && !section.contains("Waits for"),
        "{section}"
    );
    assert!(
        section.contains(r#"href="/tasks/DF-2""#) && section.contains("· done"),
        "{section}"
    );
    assert!(
        section.contains("Follow-up") && section.contains("awaiting dependencies"),
        "{section}"
    );
    assert!(!section.contains("<details open"), "{section}");
}

#[tokio::test]
async fn a_waiting_task_opens_the_section_with_one_dependency_satisfied() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    new_task(&app.pool, &seed, "second").await;
    sqlx::query("UPDATE tasks SET first_completed_at = now() WHERE key = 'DF-1'")
        .execute(&app.pool)
        .await
        .unwrap();
    create(
        &app,
        &seed,
        "Waiting",
        system(),
        None,
        &[
            ("DF-1", DependencyKind::Completion),
            ("DF-2", DependencyKind::Done),
        ],
    )
    .await;

    let html = text(app.get("/tasks/DF-3", &cookie).await).await;

    let section = section(&html);
    assert!(
        section.contains("<details") && section.contains("open"),
        "{section}"
    );
    assert!(
        section.contains("Waits for") && !section.contains("Needed by"),
        "{section}"
    );
    let rows: Vec<&str> = section.split("<li ").skip(1).collect();
    assert_eq!(rows.len(), 2, "{section}");
    assert!(rows[0].contains("DF-1") && rows[0].contains(">satisfied<"));
    assert!(rows[0].contains("bg-status-completed-fg"), "{}", rows[0]);
    assert!(rows[1].contains("DF-2") && rows[1].contains(">waiting<"));
    assert!(rows[1].contains("border-faint"), "{}", rows[1]);
    assert!(html.contains("awaiting dependencies"), "{html}");
}

#[tokio::test]
async fn after_release_the_section_is_collapsed_and_lists_the_dependencies_as_satisfied() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    create(
        &app,
        &seed,
        "Waiting",
        system(),
        None,
        &[("DF-1", DependencyKind::Completion)],
    )
    .await;
    let first = TaskId::from_uuid(
        sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-1'")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
    );
    set_task_status(&app.pool, first, TaskStatus::Waiting).await;
    df_db::mark_task_done(&app.pool, first, system())
        .await
        .unwrap();

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;

    let section = section(&html);
    assert!(
        section.contains("Waits for") && section.contains("satisfied"),
        "{section}"
    );
    assert!(
        !section.contains("<details open") && !section.contains("open class"),
        "{section}"
    );
    assert!(html.contains(">queued<"), "{html}");
}

#[tokio::test]
async fn the_awaiting_status_has_the_queued_tone_and_its_own_label() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    create(
        &app,
        &seed,
        "Waiting",
        system(),
        None,
        &[("DF-1", DependencyKind::Completion)],
    )
    .await;
    set_task_status(
        &app.pool,
        TaskId::from_uuid(
            sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-2'")
                .fetch_one(&app.pool)
                .await
                .unwrap(),
        ),
        TaskStatus::AwaitingDependencies,
    )
    .await;

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;

    assert!(
        html.contains("bg-status-queued-bg text-status-queued-fg"),
        "{html}"
    );
    assert!(html.contains(">awaiting dependencies<"), "{html}");
}
