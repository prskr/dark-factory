//! Resetting the workspace of a task (`iter2-task-volume-lifecycle`, design D2).

use std::time::Duration;

use axum::http::StatusCode;
use df_core::TaskVolumeSpec;
use df_core::testing::SandboxCall;

use super::set_status;
use crate::testkit::{TestApp, insert_run, new_task, seed, text};

async fn count(app: &TestApp, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.pool).await.unwrap()
}

async fn volume_deleted(app: &TestApp) -> bool {
    count(
        app,
        "SELECT count(*) FROM tasks WHERE volume_deleted_at IS NOT NULL",
    )
    .await
        == 1
}

async fn reset_audits(app: &TestApp) -> i64 {
    count(
        app,
        "SELECT count(*) FROM audit_log WHERE action = 'task.reset_workspace'",
    )
    .await
}

/// Returns once a session of this test's database waits for a row lock.
async fn wait_for_a_lock_wait(app: &TestApp) {
    let wait = async {
        loop {
            let waiting = count(
                app,
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .await;
            if waiting > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("no session waited for the task's row lock");
}

/// The status entry of the details list.
fn status_entry(html: &str) -> &str {
    let dl = &html[html.find("<dl ").unwrap()..];
    &dl[..dl.find("</dl>").unwrap()]
}

#[tokio::test]
async fn resetting_a_waiting_task_deletes_its_volume_audits_and_marks_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "succeeded").await;
    set_status(&app, "waiting").await;

    let response = app.post("/tasks/DF-1/reset-workspace", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("id=\"task-details\""));
    assert_eq!(
        app.provider.calls(),
        vec![SandboxCall::DeleteTaskVolume(TaskVolumeSpec::for_task(
            "DF-1".parse().unwrap()
        ))]
    );
    assert_eq!(reset_audits(&app).await, 1);
    assert!(volume_deleted(&app).await);
}

#[tokio::test]
async fn resetting_is_refused_while_a_run_is_preparing_and_keeps_the_volume() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "preparing").await;
    set_status(&app, "running").await;

    let response = app.post("/tasks/DF-1/reset-workspace", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(text(response).await.contains("id=\"task-details\""));
    assert!(app.provider.calls().is_empty());
    assert_eq!(reset_audits(&app).await, 0);
    assert!(!volume_deleted(&app).await);
}

#[tokio::test]
async fn resetting_an_unknown_task_is_not_found() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app.post("/tasks/DF-9/reset-workspace", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resetting_a_task_deleted_meanwhile_is_not_found() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    // The task is gone once the reset waits for its row lock.
    let mut delete = app.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM tasks WHERE key = 'DF-1'")
        .execute(&mut *delete)
        .await
        .unwrap();

    let (response, ()) = tokio::join!(
        app.post("/tasks/DF-1/reset-workspace", "", &cookie),
        async {
            wait_for_a_lock_wait(&app).await;
            delete.commit().await.unwrap();
        }
    );

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(app.provider.calls().is_empty());
    assert_eq!(reset_audits(&app).await, 0);
}

#[tokio::test]
async fn a_failed_volume_deletion_shows_an_inline_error_and_writes_neither_marker_nor_audit_entry()
{
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "waiting").await;
    app.provider
        .push_delete_volume_result(Err(df_core::ProviderError::Backend("down".into())));

    let response = app.post("/tasks/DF-1/reset-workspace", "", &cookie).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let html = text(response).await;
    assert!(html.starts_with("<div id=\"task-details\""), "{html}");
    assert!(
        html.contains("role=\"alert\">The workspace could not be reset. Try again later.<"),
        "{html}"
    );
    assert!(!volume_deleted(&app).await);
    assert_eq!(reset_audits(&app).await, 0);
}

#[tokio::test]
async fn the_status_menu_offers_reset_workspace_before_cancel_with_a_confirmation_that_names_the_loss()
 {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "succeeded").await;
    set_status(&app, "waiting").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let dl = status_entry(&html);

    let reset = dl.find(">Reset workspace…<").unwrap();
    assert!(dl.find(">Mark done<").unwrap() < reset);
    assert!(reset < dl.find(">Cancel task…<").unwrap());
    // Neutral, not red like "Cancel task…".
    let item = &dl[..reset];
    let item = &item[item.rfind("<button").unwrap()..];
    assert!(
        item.contains("text-ink") && !item.contains("text-danger") && !item.contains("disabled"),
        "{item}"
    );
    assert!(item.contains("$refs.resetDialog.showModal()"), "{item}");
    let dialog = &dl[dl.find("x-ref=\"resetDialog\"").unwrap()..];
    let dialog = &dialog[..dialog.find("</dialog>").unwrap()];
    assert!(dialog.contains("Reset workspace of DF-1?"), "{dialog}");
    assert!(dialog.contains(
        "This deletes the task's volume with its clone and build caches. The next run clones the repository again and restores the WIP snapshot if there is one."
    ));
    assert!(dialog.contains(
        "Lost for good: commits that were not pushed and uncommitted changes without a WIP snapshot."
    ));
    assert!(dialog.contains(">Keep workspace<") && dialog.contains(">Reset workspace<"));
    // Both paragraphs in the normal text colour and weight, 15 px on a phone.
    let paragraphs: Vec<&str> = dialog
        .match_indices("<p ")
        .map(|(i, _)| &dialog[i..])
        .collect();
    assert_eq!(paragraphs.len(), 2, "{dialog}");
    for p in paragraphs {
        let tag = &p[..p.find('>').unwrap()];
        assert!(
            tag.contains("font-normal")
                && tag.contains("max-md:text-[15px]")
                && !tag.contains("text-muted"),
            "{tag}"
        );
    }
    // Nothing is posted before the button inside the dialog is used.
    assert_eq!(
        dl.matches("hx-post=\"/tasks/DF-1/reset-workspace\"")
            .count(),
        1
    );
    assert_eq!(
        dialog
            .matches("hx-post=\"/tasks/DF-1/reset-workspace\"")
            .count(),
        1
    );
}

#[tokio::test]
#[cfg_attr(
    not(df_web_styles),
    ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
)]
async fn opening_the_status_menu_makes_the_phone_sheet_full_height() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let dl = status_entry(&html);

    let select = &dl[dl.find("aria-haspopup=\"true\"").unwrap()..];
    let select = &select[..select.find('>').unwrap()];
    assert!(
        select.contains("x-on:click=\"open = !open; if (open) sheet = 'full'\""),
        "{select}"
    );
    // The sheet the menu sits in: 90 % of the screen when full.
    assert!(
        crate::app_css()
            .unwrap()
            .contains(".ws-details[data-size=full]{height:90dvh}")
    );
}

#[tokio::test]
async fn reset_workspace_is_disabled_with_a_hint_while_a_run_is_active() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 2, "running").await;
    set_status(&app, "running").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let dl = status_entry(&html);

    let reset = dl.find(">Reset workspace…<").unwrap();
    let item = &dl[..reset];
    let item = &item[item.rfind("<button").unwrap()..];
    assert!(item.contains("disabled"), "{item}");
    let hint = &dl[reset..];
    let hint = &hint[..hint.find("</button>").unwrap()];
    assert!(hint.contains("text-xs text-muted") && hint.contains(">Stop the run first<"));
    assert!(!dl.contains("resetDialog"), "{dl}");
    assert!(!dl.contains("/reset-workspace"), "{dl}");
}

#[tokio::test]
async fn a_completed_task_offers_only_reset_workspace() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "succeeded").await;
    set_status(&app, "completed").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let dl = status_entry(&html);

    assert!(dl.contains(">Reset workspace…<"), "{dl}");
    assert!(!dl.contains(">Mark done<") && !dl.contains(">Cancel task…<"));
}
