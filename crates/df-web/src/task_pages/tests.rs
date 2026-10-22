use axum::http::StatusCode;
use df_core::Principal;
use serde_json::json;

use crate::testkit::{
    TestApp, add_event, chunk, form, header_str, insert_run, new_task, seed, text,
};

fn task_form(seed: &crate::testkit::Seed, title: &str, description: &str) -> String {
    form(&[
        ("title", title),
        ("description", description),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", ""),
    ])
}

#[tokio::test]
async fn creating_a_task_opens_its_detail_page_as_queued() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let response = app
        .post("/tasks", &task_form(&seed, "Fix login", "details"), &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/tasks/DF-1");

    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("DF-1") && page.contains("Fix login"));
    assert!(page.contains(">queued<"), "{page}");
    let created_by: Option<uuid::Uuid> = sqlx::query_scalar("SELECT created_by FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(created_by.is_some());
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'task.create' AND resource_id = 'DF-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn missing_title_shows_the_form_again_with_the_entered_values() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let response = app
        .post(
            "/tasks",
            &task_form(&seed, "  ", "keep this description"),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains("Title is required."));
    assert!(html.contains("keep this description"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn task_creation_needs_a_known_project_and_persona_and_a_numeric_issue() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("title", "t"),
        ("project", "nope"),
        ("persona", ""),
        ("issue_number", "x"),
    ]);
    let html = text(app.post("/tasks", &body, &cookie).await).await;
    assert!(html.contains("Choose a project.") && html.contains("Choose a persona."));
    let seed = seed(&app.pool).await;
    let body = form(&[
        ("title", "t"),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "0"),
    ]);
    assert!(
        text(app.post("/tasks", &body, &cookie).await)
            .await
            .contains("Must be a positive number.")
    );
}

#[tokio::test]
async fn task_list_shows_newest_first_with_status_and_status_line() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    let second = new_task(&app.pool, &seed, "second").await;
    sqlx::query("UPDATE tasks SET status = 'running', status_line = 'compiling' WHERE id = $1")
        .bind(second.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(
        html.find("DF-2").unwrap() < html.find("DF-1").unwrap(),
        "newest first"
    );
    assert!(
        html.contains("compiling")
            && html.contains("running")
            && html.contains("Linus")
            && html.contains("Dark Factory")
    );
    assert!(html.contains("hx-sse:connect=\"/tasks/events\""));
}

#[tokio::test]
async fn detail_lists_runs_with_outcome_retry_time_and_token_counts_per_model() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "failed").await;
    sqlx::query("UPDATE runs SET outcome = 'rate_limited', detail = 'window exhausted', retry_after = '2026-10-23 08:30:00+00', ended_at = now() WHERE id = $1")
        .bind(run)
        .execute(&app.pool)
        .await
        .unwrap();
    let ended = |model: &str, input: &str| json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN", "usage": [{"modelId": model, "inputTokens": input, "outputTokens": "7", "cacheReadTokens": "3", "cacheWriteTokens": "1"}]}});
    add_event(&app.pool, task.id, run, 1, "t1", ended("opus", "100")).await;
    add_event(&app.pool, task.id, run, 2, "t2", ended("opus", "50")).await;
    add_event(&app.pool, task.id, run, 3, "t2", ended("haiku", "9")).await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(html.contains("rate limited"), "{html}");
    assert!(html.contains("window exhausted") && html.contains("retry at 2026-10-23 08:30:00 UTC"));
    assert!(
        html.contains("opus: 150 in, 14 out, 6 cache read, 2 cache write"),
        "{html}"
    );
    assert!(html.contains("haiku: 9 in, 7 out, 3 cache read, 1 cache write"));
}

#[tokio::test]
async fn detail_links_pull_request_and_issue_when_they_exist() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let none = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!none.contains("PR #") && !none.contains("Issue #"));

    sqlx::query("UPDATE tasks SET pr_number = 9, pr_url = 'https://git.example.org/o/r/pulls/9', issue_number = 4 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        html.contains("href=\"https://git.example.org/o/r/pulls/9\"") && html.contains("PR #9")
    );
    assert!(
        html.contains("href=\"https://git.example.org/peter/dark-factory/issues/4\"")
            && html.contains("Issue #4")
    );

    // A non-http(s) URL from the forge is not turned into a link.
    sqlx::query("UPDATE tasks SET pr_url = 'javascript:alert(1)' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!html.contains("href=\"javascript:"));
}

#[tokio::test]
async fn detail_renders_the_transcript_escaped_and_connects_after_the_last_event() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "running").await;
    add_event(
        &app.pool,
        task.id,
        run,
        1,
        "t1",
        chunk("m1", "<script>alert(1)</script>"),
    )
    .await;
    let last = add_event(
        &app.pool,
        task.id,
        run,
        2,
        "t1",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    )
    .await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!html.contains("<script>alert(1)"), "{html}");
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        html.contains(&format!(
            "hx-sse:connect=\"/tasks/DF-1/events?after={last}\""
        )),
        "{html}"
    );
    assert!(app.get("/tasks/DF-99", &cookie).await.status() == StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn follow_up_message_is_pending_and_requeues_the_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    // The initial assignment was delivered; the agent is waiting.
    let initial = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    let run = insert_run(&app.pool, task.id, 1, "succeeded").await;
    df_db::mark_prompt_delivered(&app.pool, initial[0].id, df_core::RunId::from_uuid(run))
        .await
        .unwrap();
    df_db::update_task_status(&app.pool, task.id, df_core::TaskStatus::Waiting)
        .await
        .unwrap();

    let response = app
        .post(
            "/tasks/DF-1/messages",
            &form(&[("message", "also add a test for X")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "message-queued");
    let fragment = text(response).await;
    assert!(fragment.contains("also add a test for X") && fragment.contains("Pending"));

    let status: String = sqlx::query_scalar("SELECT status FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "queued");
    let queued = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    assert!(
        matches!(&queued[0].author, Principal::Human { display_name, .. } if display_name == "Peter")
    );

    // The page shows it as pending until the transcript delivers it.
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("also add a test for X"));
    df_db::mark_prompt_delivered(&app.pool, queued[0].id, df_core::RunId::from_uuid(run))
        .await
        .unwrap();
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!page.contains("also add a test for X"));
}

#[tokio::test]
async fn blank_messages_are_refused() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let response = app
        .post(
            "/tasks/DF-1/messages",
            &form(&[("message", "   ")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Write a message first."));
}

#[tokio::test]
async fn message_form_keeps_the_draft_outside_the_streamed_regions() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let form_at = html.find("x-data=").expect("alpine draft state");
    // The form is a sibling of, not inside, the regions the stream replaces.
    for region in [
        "id=\"task-head\"",
        "id=\"runs\"",
        "id=\"transcript\"",
        "id=\"pending\"",
    ] {
        assert!(html.find(region).is_none_or(|at| at < form_at), "{region}");
    }
    assert!(html.contains("x-model=\"draft\""));
}
