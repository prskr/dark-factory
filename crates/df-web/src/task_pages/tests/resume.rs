//! Resume time and count on the task detail and in the task list (`task-ui` spec, "Resume
//! information").

use crate::testkit::{TestApp, insert_run, new_task, seed, text};

/// A task in `status` with the resume time 04:30 UTC today and `count` automatic resumes.
async fn waiting_task(app: &TestApp, status: &str, count: i32) {
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    // Built from the date the page compares against, so "04:30" holds across UTC midnight.
    let resume_at = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(4, 30, 0)
        .unwrap()
        .and_utc();
    sqlx::query(
        "UPDATE tasks SET status = $2, resume_at = $4,
             auto_resumes = $3,
             status_line = 'Waiting for the window' WHERE id = $1",
    )
    .bind(task.id.as_uuid())
    .bind(status)
    .bind(count)
    .bind(resume_at)
    .execute(&app.pool)
    .await
    .unwrap();
}

async fn end_last_run(app: &TestApp, outcome: &str, detail: &str) {
    let task = sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let run = insert_run(&app.pool, df_core::TaskId::from_uuid(task), 1, "failed").await;
    sqlx::query("UPDATE runs SET outcome = $2, detail = $3 WHERE id = $1")
        .bind(run)
        .bind(outcome)
        .bind(detail)
        .execute(&app.pool)
        .await
        .unwrap();
}

/// The stored resume time as the page writes it in the `datetime` attribute.
async fn stored_resume_time(app: &TestApp) -> String {
    sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>("SELECT resume_at FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .to_rfc3339()
}

fn now_section(html: &str) -> &str {
    let now = &html[html.find(">Now</h2>").unwrap()..];
    &now[..now.find(">Links</h2>").unwrap()]
}

fn details_list(html: &str) -> &str {
    let dl = &html[html.find("<dl ").unwrap()..];
    &dl[..dl.find("</dl>").unwrap()]
}

#[tokio::test]
async fn a_rate_limited_task_shows_when_it_resumes_and_how_often_it_did() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    waiting_task(&app, "rate_limited", 2).await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    let now = now_section(&html);
    assert!(now.contains("bg-status-limited-fg"), "{now}");
    let datetime = stored_resume_time(&app).await;
    assert!(
        now.contains(&format!("Resumes at <time datetime=\"{datetime}\""))
            && now.contains(">04:30</time>"),
        "{now}"
    );
    assert!(now.contains("· rate limit"), "{now}");
    assert!(!now.contains("Waiting for the window"), "{now}");
    let dl = details_list(&html);
    assert!(
        dl.contains("<dt class=\"text-faint\">Resumes</dt>")
            && dl.contains(">2 of 3 automatic</dd>"),
        "{dl}"
    );
}

#[tokio::test]
async fn the_resume_count_row_is_left_out_while_the_count_is_zero() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    waiting_task(&app, "rate_limited", 0).await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(now_section(&html).contains("Resumes at "));
    assert!(!details_list(&html).contains("Resumes"));
}

#[tokio::test]
async fn a_queued_task_names_the_failure_of_its_last_run() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    waiting_task(&app, "queued", 1).await;
    end_last_run(&app, "sandbox_lost", "sandbox is gone: pod deleted").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        now_section(&html).contains("Resumes at ") && now_section(&html).contains("· sandbox lost"),
        "{}",
        now_section(&html)
    );
    assert!(details_list(&html).contains("1 of 3 automatic"));

    sqlx::query("UPDATE runs SET outcome = 'provisioning_failed', detail = 'capacity'")
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(now_section(&html).contains("· no capacity"));
}

#[tokio::test]
async fn a_task_without_a_resume_time_shows_its_status_line_and_no_resume_row() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    waiting_task(&app, "rate_limited", 0).await;
    sqlx::query("UPDATE tasks SET resume_at = NULL")
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(now_section(&html).contains("Waiting for the window"));
    assert!(!html.contains("Resumes at "));
    assert!(!details_list(&html).contains("Resumes"));
}

#[tokio::test]
async fn the_task_row_says_when_the_task_resumes() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    waiting_task(&app, "queued", 0).await;
    let html = text(app.get("/tasks", &cookie).await).await;
    let row = &html[html.find("<li id=\"row-DF-1\"").unwrap()..];
    let row = &row[..row.find("</li>").unwrap()];
    assert!(row.contains("Resumes at 04:30"), "{row}");
    assert!(!row.contains("Waiting for the window"), "{row}");
}
