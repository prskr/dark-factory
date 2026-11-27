//! The task list as a tree: top-level rows, opening a row, the scoped list and its breadcrumb
//! (`task-ui`, "Top-level tasks in the task list" to "Breadcrumb links").

use df_core::{PersonaId, Principal, ProjectId, TaskStatus};

use crate::testkit::{
    Seed, TestApp, new_subtask, new_task, row, seed, set_task_status, tag_around, text,
};

/// DF-1 > DF-2 (`completed`) and DF-3 (`running`) > DF-4 (`waiting`); returns the seed.
async fn tree(app: &TestApp) -> Seed {
    let seed = seed(&app.pool).await;
    let top = new_task(&app.pool, &seed, "Feature").await;
    let done = new_subtask(&app.pool, &seed, "First", top.id).await;
    let running = new_subtask(&app.pool, &seed, "Second", top.id).await;
    let leaf = new_subtask(&app.pool, &seed, "Leaf", running.id).await;
    set_task_status(&app.pool, done.id, TaskStatus::Completed).await;
    set_task_status(&app.pool, running.id, TaskStatus::Running).await;
    set_task_status(&app.pool, leaf.id, TaskStatus::Waiting).await;
    seed
}

#[tokio::test]
async fn the_default_list_shows_top_level_tasks_and_tasks_of_a_chat_not_subtasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = tree(&app).await;
    let chat = df_db::chats::create_chat(
        &app.pool,
        df_db::chats::NewChat {
            project_id: ProjectId::from_uuid(seed.project),
            persona_id: PersonaId::from_uuid(seed.persona),
            author: Principal::system("test"),
            text: "hello".into(),
        },
    )
    .await
    .unwrap()
    .0;
    let from_chat = new_subtask(&app.pool, &seed, "From chat", chat.id).await;

    let html = text(app.get("/tasks", &cookie).await).await;

    assert!(html.contains("row-DF-1\"") && html.contains(&format!("row-{}\"", from_chat.key)));
    for subtask in ["DF-2", "DF-3", "DF-4"] {
        assert!(!html.contains(&format!("row-{subtask}\"")), "{subtask}");
    }
}

#[tokio::test]
async fn a_done_parent_with_a_waiting_subtask_stays_in_active_with_the_dot_and_a_struck_title() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let top = new_task(&app.pool, &seed, "Shipped").await;
    let sub = new_subtask(&app.pool, &seed, "Open end", top.id).await;
    set_task_status(&app.pool, top.id, TaskStatus::Completed).await;
    set_task_status(&app.pool, sub.id, TaskStatus::Waiting).await;

    let html = text(app.get("/tasks", &cookie).await).await;

    let top_row = row(&html, "DF-1");
    assert!(
        top_row.contains("line-through") && top_row.contains(">1 subtask needs you<"),
        "{top_row}"
    );
    assert!(top_row.contains("role=\"img\""), "{top_row}");

    set_task_status(&app.pool, sub.id, TaskStatus::Running).await;
    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(!html.contains("row-DF-1\""), "{html}");
}

#[tokio::test]
async fn a_link_only_status_lists_subtasks_as_rows_without_disclosure() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    tree(&app).await;

    let html = text(app.get("/tasks?status=waiting", &cookie).await).await;

    assert!(html.contains("row-DF-4\""), "{html}");
    assert!(!html.contains("subtasks of"), "{html}");
}

#[tokio::test]
async fn opening_a_row_lists_its_subtasks_below_it_and_only_rows_with_subtasks_have_a_control() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = tree(&app).await;
    new_task(&app.pool, &seed, "Alone").await;

    let closed = text(app.get("/tasks", &cookie).await).await;
    let open = text(app.get("/tasks?open=DF-1", &cookie).await).await;

    let control = tag_around(&closed, "aria-label=\"Show subtasks of DF-1\"");
    assert!(
        control.contains("aria-expanded=\"false\"")
            && control.contains("href=\"?open=DF-1\"")
            && control.contains("hx-push-url=\"true\"")
            && control.contains("size-11"),
        "{control}"
    );
    assert!(!closed.contains("subtasks of DF-5"), "{closed}");
    assert!(!closed.contains("row-DF-2\""), "{closed}");
    let at = |html: &str, key: &str| html.find(&format!("row-{key}\"")).unwrap();
    // Below the parent, in the list's sort order (last update first).
    assert!(at(&open, "DF-1") < at(&open, "DF-3") && at(&open, "DF-3") < at(&open, "DF-2"));
    assert!(
        open.contains("aria-label=\"Hide subtasks of DF-1\""),
        "{open}"
    );
    assert!(
        !open.contains("row-DF-4\""),
        "subtasks two levels down stay closed: {open}"
    );
    let scope = tag_around(&open, "aria-label=\"Open subtasks of DF-3\"");
    assert!(scope.contains("href=\"?parent=DF-3\""), "{scope}");
    assert!(
        open.contains("hx-sse:connect=\"/tasks/events?open=DF-1\""),
        "{open}"
    );
}

#[tokio::test]
async fn a_task_whose_subtasks_are_all_cancelled_can_be_opened_and_lists_them() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let top = new_task(&app.pool, &seed, "Dropped plan").await;
    let cancelled = new_subtask(&app.pool, &seed, "Dropped", top.id).await;
    set_task_status(&app.pool, cancelled.id, TaskStatus::Cancelled).await;
    let nested = new_subtask(&app.pool, &seed, "Nested", cancelled.id).await;
    set_task_status(&app.pool, nested.id, TaskStatus::Cancelled).await;

    let closed = text(app.get("/tasks", &cookie).await).await;
    let open = text(app.get("/tasks?open=DF-1", &cookie).await).await;

    assert!(
        closed.contains("aria-label=\"Show subtasks of DF-1\""),
        "{closed}"
    );
    assert!(!closed.contains("data-subtasks"), "no chip: {closed}");
    assert!(open.contains("row-DF-2\""), "{open}");
    assert!(
        row(&open, "DF-2").contains("aria-label=\"Open subtasks of DF-2\""),
        "{open}"
    );
}

#[tokio::test]
async fn the_scoped_list_shows_the_task_first_with_a_breadcrumb_and_no_chips() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    tree(&app).await;

    let html = text(app.get("/tasks?parent=DF-3", &cookie).await).await;

    let at = |key: &str| html.find(&format!("row-{key}\"")).unwrap();
    assert!(at("DF-3") < at("DF-4"));
    assert!(
        !html.contains("row-DF-1\"") && !html.contains("row-DF-2\""),
        "{html}"
    );
    assert!(html.contains("aria-label=\"Breadcrumb\""), "{html}");
    assert!(
        !html.contains(">Active</a>") && !html.contains("All projects"),
        "{html}"
    );
    assert!(
        !html.contains("aria-label=\"Show subtasks of DF-3\""),
        "the scoped task has no disclosure: {html}"
    );
    assert!(
        html.contains("hx-sse:connect=\"/tasks/events?parent=DF-3\""),
        "{html}"
    );
}

#[tokio::test]
async fn an_unknown_scope_key_falls_back_to_the_top_level_list() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    tree(&app).await;

    let html = text(app.get("/tasks?parent=DF-99", &cookie).await).await;

    assert!(
        html.contains("row-DF-1\"") && html.contains(">Active</a>"),
        "{html}"
    );
}

#[tokio::test]
async fn the_breadcrumb_of_a_third_level_task_links_up_the_tree() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    tree(&app).await;

    let html = text(
        app.get(
            "/tasks?status=done&project=DF&sort=created&parent=DF-4",
            &cookie,
        )
        .await,
    )
    .await;

    let nav = &html[html.find("aria-label=\"Breadcrumb\"").unwrap()..];
    let nav = &nav[..nav.find("</nav>").unwrap()];
    assert!(
        nav.contains("href=\"/tasks?status=done&amp;project=DF&amp;sort=created\""),
        "{nav}"
    );
    assert!(
        nav.contains("href=\"/tasks?status=done&amp;project=DF&amp;sort=created&amp;open=DF-1\""),
        "{nav}"
    );
    assert!(
        nav.contains("href=\"/tasks?status=done&amp;project=DF&amp;sort=created&amp;parent=DF-3\""),
        "{nav}"
    );
    assert!(
        nav.contains("<span aria-current=\"page\" class=\"font-semibold text-ink\">DF-4</span>"),
        "{nav}"
    );
}
