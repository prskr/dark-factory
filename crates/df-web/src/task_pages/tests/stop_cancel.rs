//! Stopping a run, cancelling a task, and marking done with a PR (`specs/task-ui/spec.md`).

use axum::http::StatusCode;

use super::set_status;
use crate::testkit::{TestApp, insert_run, insert_turn_event, new_task, seed, text};

async fn count(app: &TestApp, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.pool).await.unwrap()
}

#[tokio::test]
async fn an_active_run_card_offers_stop_and_an_ended_one_does_not() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "succeeded").await;
    let ended = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!ended.contains(">Stop run<"), "{ended}");

    insert_run(&app.pool, task.id, 2, "running").await;
    let running = text(app.get("/tasks/DF-1", &cookie).await).await;
    let runs = &running[running.find("id=\"runs\"").unwrap()..];
    assert!(runs.contains("hx-post=\"/tasks/DF-1/stop\""), "{runs}");
    assert_eq!(runs.matches(">Stop run<").count(), 1, "{runs}");
}

#[tokio::test]
async fn the_status_switch_offers_stop_run_for_an_active_run_that_is_not_ending() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "awaiting_input").await;
    set_status(&app, "waiting").await;
    let switch = |html: &str| {
        let dl = &html[html.find("<dl ").unwrap()..];
        dl[..dl.find("</dl>").unwrap()].to_string()
    };

    let idle = switch(&text(app.get("/tasks/DF-1", &cookie).await).await);
    let stop = crate::testkit::tag_around(&idle, ">Stop run<");
    assert!(stop.contains("hx-post=\"/tasks/DF-1/stop\""), "{idle}");

    sqlx::query("UPDATE runs SET state = 'finalizing' WHERE id = $1")
        .bind(run)
        .execute(&app.pool)
        .await
        .unwrap();
    let ending = switch(&text(app.get("/tasks/DF-1", &cookie).await).await);
    assert!(!ending.contains(">Stop run<"), "{ending}");
}

#[tokio::test]
async fn send_becomes_interrupt_while_a_turn_runs_with_an_empty_message_box() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "awaiting_input").await;

    set_status(&app, "waiting").await;
    let idle = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!idle.contains("aria-label=\"Interrupt\""));

    sqlx::query("UPDATE runs SET state = 'running' WHERE id = $1")
        .bind(run)
        .execute(&app.pool)
        .await
        .unwrap();
    set_status(&app, "running").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let area = &html[html.find("id=\"composer-action\"").unwrap()..];
    let area = &area[..area.find("</span>").unwrap()];
    // Interrupt is shown while the box is empty; Send stays for queueing a message during a turn.
    assert!(
        area.contains("aria-label=\"Interrupt\"")
            && area.contains("hx-post=\"/tasks/DF-1/interrupt\"")
    );
    assert!(area.contains("group-has-[textarea:placeholder-shown]:inline-flex"));
    assert!(
        area.contains("aria-label=\"Send\"") && area.contains("hx-post=\"/tasks/DF-1/messages\"")
    );
    assert!(area.contains("group-has-[textarea:placeholder-shown]:hidden"));
}

#[tokio::test]
async fn the_composer_offers_no_interrupt_while_no_turn_runs() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    for state in ["finalizing", "awaiting_input"] {
        sqlx::query("DELETE FROM runs")
            .execute(&app.pool)
            .await
            .unwrap();
        insert_run(&app.pool, task.id, 1, state).await;
        set_status(&app, "running").await;

        let html = text(app.get("/tasks/DF-1", &cookie).await).await;

        assert!(
            !html.contains("aria-label=\"Interrupt\""),
            "{state}: {html}"
        );
        assert!(html.contains("aria-label=\"Send\""));
    }
}

#[tokio::test]
async fn interrupting_a_running_turn_is_audited_and_answers_without_content() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "running").await;
    insert_turn_event(&app.pool, task.id, run, 1, "turn_started", "turn_1").await;

    let response = app.post("/tasks/DF-1/interrupt", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM audit_log WHERE action = 'run.interrupt'"
        )
        .await,
        1
    );
    // The run is not stopped.
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM runs WHERE stop_requested_at IS NOT NULL"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn interrupting_a_turn_that_has_ended_shows_the_notice_while_the_run_is_still_running() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "running").await;
    insert_turn_event(&app.pool, task.id, run, 1, "turn_started", "turn_1").await;
    insert_turn_event(&app.pool, task.id, run, 2, "turn_ended", "turn_1").await;

    let response = app.post("/tasks/DF-1/interrupt", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(text(response).await.contains("No turn is running"));
}

#[tokio::test]
async fn interrupting_without_a_running_turn_shows_the_notice_and_audits_nothing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "awaiting_input").await;

    let response = app.post("/tasks/DF-1/interrupt", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = text(response).await;
    assert!(
        body.contains("id=\"pending\"") && body.contains("No turn is running"),
        "{body}"
    );
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM audit_log WHERE action = 'run.interrupt'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn interrupting_needs_the_same_permission_as_stopping() {
    let app = TestApp::new().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "running").await;
    let (no_roles, _) =
        crate::auth::testing::seed_session(&app.pool, &[], 3600, Some("refresh-0")).await;

    let stop = app.post("/tasks/DF-1/stop", "", &no_roles).await.status();
    let interrupt = app
        .post("/tasks/DF-1/interrupt", "", &no_roles)
        .await
        .status();

    assert!(!stop.is_success(), "{stop}");
    assert_eq!(interrupt, stop);
    assert_eq!(
        app.post("/tasks/DF-9/interrupt", "", &app.login().await)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn stopping_marks_the_run_and_answers_without_content() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "running").await;

    let response = app.post("/tasks/DF-1/stop", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM runs WHERE stop_requested_at IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM audit_log WHERE action = 'run.stop'"
        )
        .await,
        1
    );
    // The page shows the run as finalizing, so there is nothing to stop twice.
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!html.contains(">Stop run<"), "{html}");
}

#[tokio::test]
async fn stopping_without_an_active_run_or_for_an_unknown_task_is_refused() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    assert_eq!(
        app.post("/tasks/DF-1/stop", "", &cookie).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.post("/tasks/DF-9/stop", "", &cookie).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_status_menu_ends_with_cancel_task_and_a_confirmation_that_names_the_effects() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 2, "running").await;
    set_status(&app, "running").await;
    sqlx::query("UPDATE tasks SET pr_number = 41, issue_number = 7")
        .execute(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let dl = &html[html.find("<dl ").unwrap()..];
    let dl = &dl[..dl.find("</dl>").unwrap()];
    // Last item, danger colored; "Mark done" is disabled while the task works.
    assert!(dl.find(">Mark done<").unwrap() < dl.find(">Cancel task…<").unwrap());
    assert!(dl.contains("text-danger"));
    // The native dialog confirms; nothing is posted before the button inside it is used.
    assert!(dl.contains("<dialog") && dl.contains("showModal()"));
    assert!(dl.contains("Cancel DF-1?"), "{dl}");
    assert!(
        dl.contains("Stops run #2, discards 1 queued message and closes the pending requests of this task. PR #41 and issue #7 stay as they are."),
        "{dl}"
    );
    assert!(dl.contains("A new message reopens the task."));
    let dialog = &dl[dl.find("<dialog").unwrap()..];
    assert!(dialog.contains("Keep task"));
    assert_eq!(dialog.matches("hx-post=\"/tasks/DF-1/cancel\"").count(), 1);
    assert_eq!(dl.matches("hx-post=\"/tasks/DF-1/cancel\"").count(), 1);
}

#[tokio::test]
async fn cancelling_cancels_the_task_and_answers_the_details() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "running").await;
    set_status(&app, "running").await;

    let response = app.post("/tasks/DF-1/cancel", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("id=\"task-details\"") && html.contains("cancelled"),
        "{html}"
    );
    assert!(!html.contains("Cancel task…"), "{html}");
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM tasks WHERE status = 'cancelled'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM audit_log WHERE action = 'task.cancel'"
        )
        .await,
        1
    );
    // The header shows the outlined badge.
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let head = &page[page.find("id=\"task-head\"").unwrap()..];
    let head = &head[..head.find("</header>").unwrap()];
    assert!(head.contains("border-line-strong"), "{head}");
}

#[tokio::test]
async fn cancelling_a_completed_task_is_refused_and_changes_nothing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "completed").await;

    let response = app.post("/tasks/DF-1/cancel", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(text(response).await.contains("id=\"task-details\""));
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM tasks WHERE status = 'completed'"
        )
        .await,
        1
    );
    assert_eq!(
        app.post("/tasks/DF-9/cancel", "", &cookie).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn marking_a_task_with_a_pr_done_enqueues_a_ready_job_without_calling_the_forge() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "waiting").await;
    sqlx::query("UPDATE tasks SET pr_number = 41, pr_url = 'https://git.example.org/o/r/pulls/41'")
        .execute(&app.pool)
        .await
        .unwrap();

    let response = app.post("/tasks/DF-1/done", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(app.forge.calls().is_empty());
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM outbox WHERE kind = 'pr.sync' AND payload ->> 'open' = 'false'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn marking_done_without_a_pr_enqueues_no_job() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "waiting").await;

    assert_eq!(
        app.post("/tasks/DF-1/done", "", &cookie).await.status(),
        StatusCode::OK
    );

    assert!(app.forge.calls().is_empty());
    assert_eq!(
        count(&app, "SELECT count(*) FROM outbox WHERE kind = 'pr.sync'").await,
        0
    );
}
