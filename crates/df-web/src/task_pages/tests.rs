use axum::http::StatusCode;
use df_core::{Principal, TITLE_MAX_CHARS};
use serde_json::json;

use super::{details_region, head_region, pending_region, runs_region};
use crate::testkit::{
    TestApp, add_event, chunk, form, header_str, insert_run, new_task, seed, text,
};

fn task_form(seed: &crate::testkit::Seed, description: &str) -> String {
    form(&[
        ("description", description),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", ""),
    ])
}

#[tokio::test]
async fn creating_a_task_opens_its_page_as_queued_with_a_placeholder_title() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let response = app
        .post(
            "/tasks",
            &task_form(
                &seed,
                "The nightly backup runs twice. It started last week.",
            ),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/tasks/DF-1");

    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("DF-1") && page.contains("The nightly backup runs twice."));
    assert!(page.contains(">queued<"), "{page}");
    let (created_by, source): (Option<uuid::Uuid>, String) =
        sqlx::query_as("SELECT created_by, title_source FROM tasks")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(created_by.is_some());
    assert_eq!(source, "placeholder");
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'task.create' AND resource_id = 'DF-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn missing_description_keeps_the_composer_open_with_project_and_assignee() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let response = app.post("/tasks", &task_form(&seed, "  "), &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("Describe what the agent should do."),
        "{html}"
    );
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.project)));
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.persona)));
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
        ("description", "d"),
        ("project", "nope"),
        ("persona", ""),
        ("issue_number", "x"),
    ]);
    let html = text(app.post("/tasks", &body, &cookie).await).await;
    assert!(html.contains("Choose a project.") && html.contains("Choose an assignee."));
    let seed = seed(&app.pool).await;
    let body = form(&[
        ("description", "d"),
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
async fn linked_issue_is_stored_and_shown_as_a_chip_when_the_composer_reopens() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let body = form(&[
        ("description", ""),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "7"),
    ]);
    let html = text(app.post("/tasks", &body, &cookie).await).await;
    assert!(
        html.contains("issue: '7'") && html.contains("Issue #"),
        "{html}"
    );

    let body = form(&[
        ("description", "Fix it"),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "7"),
    ]);
    app.post("/tasks", &body, &cookie).await;
    let issue: Option<i64> = sqlx::query_scalar("SELECT issue_number FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(issue, Some(7));
}

#[tokio::test]
async fn composer_defaults_to_the_filtered_project_and_offers_the_keyboard_shortcuts() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let html = text(app.get("/tasks/compose?project=DF", &cookie).await).await;
    assert!(html.contains("<dialog id=\"composer\""), "{html}");
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.project)));
    assert!(html.contains("data-explicit=\"true\""));
    assert!(
        html.contains("x-on:keydown.ctrl.enter.prevent")
            && html.contains("x-on:keydown.meta.enter.prevent")
    );
    assert!(html.contains("localStorage.getItem('df.composer')"));
    assert!(html.contains("aria-label=\"Close\""));
    // An unknown project key falls back to the first project without marking it explicit.
    let html = text(app.get("/tasks/compose?project=NOPE", &cookie).await).await;
    assert!(!html.contains("data-explicit"), "{html}");
    assert_eq!(
        app.get("/tasks/new", &cookie).await.status(),
        StatusCode::NOT_FOUND
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
    assert_eq!(html.matches("x-model=\"draft\"").count(), 1);

    // The regions the stream replaces never contain the draft state.
    let detail = df_db::task_views::get_task_detail_by_key(&app.pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    for region in [
        head_region(&detail, None),
        details_region(&detail),
        runs_region(&[]),
        pending_region(&[], None),
    ] {
        assert!(!region.into_string().contains("draft"));
    }
}

#[tokio::test]
async fn columns_are_ordered_list_transcript_details() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(at("class=\"ws-list\"") < at("class=\"ws-main\""));
    assert!(at("class=\"ws-main\"") < at("class=\"ws-details\""));
}

#[tokio::test]
async fn task_rows_are_keyboard_reachable_links_in_list_order() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    for title in ["a", "b", "c"] {
        new_task(&app.pool, &seed, title).await;
    }
    let html = text(app.get("/tasks", &cookie).await).await;
    let rows = &html[html.find("<ul id=\"task-rows\"").unwrap()..];
    let rows = &rows[..rows.find("</ul>").unwrap()];
    let items: Vec<&str> = rows.split("<li ").skip(1).collect();
    assert_eq!(items.len(), 3);
    for (item, key) in items.iter().zip(["DF-3", "DF-2", "DF-1"]) {
        assert_eq!(item.matches("<a ").count(), 1, "{item}");
        assert!(
            item.contains(&format!("<a href=\"/tasks/{key}\"")),
            "{item}"
        );
        assert!(!item.contains("tabindex"), "{item}");
    }
    // The focus ring: rows never switch the outline off and the base stylesheet draws it.
    assert!(
        !html.contains("outline-none"),
        "a control hides its focus ring"
    );
    let css = crate::app_css().unwrap();
    assert!(
        css.contains(":focus-visible{outline:2px solid var(--accent-text)"),
        "{css}"
    );
}

#[tokio::test]
async fn rename_over_the_limit_is_refused_not_truncated() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "keep me").await;
    let long = "x".repeat(TITLE_MAX_CHARS + 1);
    let response = app
        .post("/tasks/DF-1/title", &form(&[("title", &long)]), &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains(&format!("At most {TITLE_MAX_CHARS} characters."))
    );
    let title: String = sqlx::query_scalar("SELECT title FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(title, "keep me");
    let exact = "y".repeat(TITLE_MAX_CHARS);
    let ok = app
        .post("/tasks/DF-1/title", &form(&[("title", &exact)]), &cookie)
        .await;
    assert_eq!(ok.status(), StatusCode::OK);
}

#[tokio::test]
async fn composer_remove_issue_button_is_touch_sized() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    let at = html.find("aria-label=\"Remove linked issue\"").unwrap();
    let tag = &html[at..at + 200];
    assert!(
        tag.contains("size-11") && tag.contains("lg:size-6"),
        "{tag}"
    );
}

/// Tailwind palette utilities (`bg-slate-900`, `text-sky-400`, ...) in rendered markup.
fn palette_classes(html: &str) -> Vec<String> {
    const PALETTES: [&str; 17] = [
        "slate", "gray", "zinc", "neutral", "stone", "red", "orange", "amber", "yellow", "lime",
        "green", "emerald", "teal", "cyan", "sky", "blue", "indigo",
    ];
    html.split(|c: char| c.is_whitespace() || c == '"')
        .filter(|token| {
            let parts: Vec<&str> = token.split('-').collect();
            parts.len() >= 3
                && PALETTES.contains(&parts[parts.len() - 2])
                && parts[parts.len() - 1].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

async fn other_project(app: &TestApp, key: &str, name: &str) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, $2, $3, 'https://g/o/r.git', 'img')")
        .bind(id)
        .bind(key)
        .bind(name)
        .execute(&app.pool)
        .await
        .unwrap();
    id
}

async fn task_in(
    app: &TestApp,
    seed: &crate::testkit::Seed,
    project: uuid::Uuid,
    title: &str,
    status: &str,
) {
    let (task, _) = df_db::create_task(
        &app.pool,
        df_db::NewTask {
            project_id: df_core::ProjectId::from_uuid(project),
            persona_id: df_core::PersonaId::from_uuid(seed.persona),
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(task.id.as_uuid())
        .bind(status)
        .execute(&app.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_list_and_task_page_share_one_workspace_with_the_view_for_phones() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;

    let list = text(app.get("/tasks", &cookie).await).await;
    assert!(
        list.contains("data-view=\"list\"") && list.contains("class=\"ws-list\""),
        "{list}"
    );
    assert!(list.contains("id=\"ws-detail\"") && list.contains("Select a task."));
    assert!(!list.contains("id=\"task-head\""));
    // Rail on desktop, bottom bar on phones.
    assert!(list.contains("aria-label=\"New task\"") && list.contains("id=\"mobile-menu\""));

    let task = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(task.contains("data-view=\"task\""), "{task}");
    for part in [
        "class=\"ws-list\"",
        "class=\"ws-main\"",
        "class=\"ws-details\"",
        "id=\"task-head\"",
        "id=\"task-details\"",
        "aria-label=\"Back to tasks\"",
        "aria-label=\"Details\"",
        "aria-label=\"Close details\"",
    ] {
        assert!(task.contains(part), "{part}");
    }
}

#[tokio::test]
async fn selecting_a_task_swaps_the_detail_regions_and_pushes_the_url() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    new_task(&app.pool, &seed, "second").await;

    let html = text(app.get("/tasks/DF-2", &cookie).await).await;
    assert!(
        html.contains("hx-get=\"/tasks/DF-1\" hx-target=\"#ws-detail\" hx-select=\"#ws-detail\" hx-swap=\"outerHTML\" hx-push-url=\"true\""),
        "{html}"
    );
    assert!(
        html.contains("href=\"/tasks/DF-1\""),
        "plain link for reloads and phones"
    );
    // The open task is marked; the list stream is outside the swapped region.
    assert!(
        html.contains("href=\"/tasks/DF-2\" hx-get=\"/tasks/DF-2\"")
            && html.contains("aria-current=\"page\"")
    );
    let list_stream = html.find("hx-sse:connect=\"/tasks/events\"").unwrap();
    assert!(list_stream < html.find("id=\"ws-detail\"").unwrap());
    assert!(
        html.find("hx-sse:connect=\"/tasks/DF-2/events").unwrap()
            > html.find("id=\"ws-detail\"").unwrap()
    );
}

#[tokio::test]
async fn list_filters_by_status_group_and_project_and_keeps_them_in_the_urls() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let web = other_project(&app, "WEB", "Website").await;
    new_task(&app.pool, &seed, "queued df").await;
    task_in(&app, &seed, web, "waiting web", "waiting").await;
    task_in(&app, &seed, web, "done web", "completed").await;

    let default = text(app.get("/tasks", &cookie).await).await;
    assert!(
        default.contains("queued df")
            && !default.contains("waiting web")
            && !default.contains("done web")
    );
    // Chips for every group, the current one marked; the project menu lists projects with counts.
    for label in ["Active", "Waiting", "Done", "All"] {
        assert!(default.contains(&format!(">{label}</a>")), "{label}");
    }
    assert!(default.contains("href=\"/tasks?status=done\""));
    assert!(
        default.contains("data-search=\"website web · 2\"")
            || default.contains("data-search=\"website web"),
        "{default}"
    );

    let waiting = text(app.get("/tasks?status=waiting", &cookie).await).await;
    assert!(waiting.contains("waiting web") && !waiting.contains("queued df"));
    let all = text(app.get("/tasks?status=all", &cookie).await).await;
    assert!(all.contains("queued df") && all.contains("waiting web") && all.contains("done web"));

    let web_only = text(app.get("/tasks?status=all&project=WEB", &cookie).await).await;
    assert!(web_only.contains("waiting web") && !web_only.contains("queued df"));
    assert!(
        web_only.contains("hx-sse:connect=\"/tasks/events?status=all&amp;project=WEB\""),
        "{web_only}"
    );
    assert!(web_only.contains("href=\"/tasks?status=done&amp;project=WEB\""));
    assert!(
        web_only.contains("hx-get=\"/tasks/WEB-1?status=all&amp;project=WEB\""),
        "{web_only}"
    );
    // The composer opened from here defaults to the filtered project.
    assert!(web_only.contains("hx-get=\"/tasks/compose?project=WEB\""));
}

#[tokio::test]
async fn project_menu_lists_every_project_with_search_data() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    other_project(&app, "WEB", "Website").await;
    new_task(&app.pool, &seed, "t").await;

    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(
        html.contains("data-search=\"dark factory df · 1\""),
        "{html}"
    );
    assert!(html.contains("data-search=\"website web · 0\""));
    assert!(html.contains("x-model=\"q\"") && html.contains("data-search=\"all projects \""));
}

#[tokio::test]
async fn placeholder_titles_are_italic_until_the_task_is_renamed() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, _) = df_db::create_task(
        &app.pool,
        df_db::NewTask {
            project_id: df_core::ProjectId::from_uuid(seed.project),
            persona_id: df_core::PersonaId::from_uuid(seed.persona),
            title: None,
            description: "Backups run twice. Since last week.".into(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap();

    let list = text(app.get("/tasks", &cookie).await).await;
    assert!(
        list.contains("italic text-muted\">Backups run twice."),
        "{list}"
    );
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("title=\"Provisional title"), "{page}");

    let response = app
        .post(
            "/tasks/DF-1/title",
            &form(&[("title", "Stop duplicate\nnightly backups")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "title-saved");
    let head = text(response).await;
    assert!(head.contains("id=\"task-head\"") && head.contains("Stop duplicate nightly backups"));
    assert!(!head.contains("Provisional title"));

    let list = text(app.get("/tasks", &cookie).await).await;
    assert!(
        list.contains(">Stop duplicate nightly backups<")
            && !list.contains("italic text-muted\">Stop")
    );
    let (title, source): (String, String) = sqlx::query_as("SELECT title, title_source FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        (title.as_str(), source.as_str()),
        ("Stop duplicate nightly backups", "user")
    );
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'task.rename' AND resource_id = $1",
    )
    .bind(task.id.to_string())
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn rename_needs_a_title_and_an_existing_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "keep me").await;

    let response = app
        .post("/tasks/DF-1/title", &form(&[("title", "  \n ")]), &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Enter a title."));
    let title: String = sqlx::query_scalar("SELECT title FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(title, "keep me");
    let missing = app
        .post("/tasks/DF-9/title", &form(&[("title", "x")]), &cookie)
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    // The rename control of the header has an accessible name and a keyboard way out.
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        page.contains("aria-label=\"Rename task\"")
            && page.contains("x-on:keydown.escape=\"editing = false\"")
    );
    assert!(page.contains("maxlength=\"80\""));
}

#[tokio::test]
async fn details_show_status_line_project_persona_and_the_task_branch() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET status_line = 'reproduced the bug' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    for part in [
        "reproduced the bug",
        "factory/task/DF-1",
        "Dark Factory",
        "Linus",
    ] {
        assert!(html.contains(part), "{part}");
    }
}

#[tokio::test]
async fn pages_use_tokens_only_and_load_nothing_from_third_parties() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    for uri in [
        "/tasks",
        "/tasks/DF-1",
        "/tasks/compose",
        "/projects",
        "/projects/new",
        "/personas",
        "/personas/new",
        "/credentials",
        "/credentials/new",
        "/tasks/nope",
    ] {
        let html = text(app.get(uri, &cookie).await).await;
        assert!(
            palette_classes(&html).is_empty(),
            "{uri}: {:?}",
            palette_classes(&html)
        );
        for attr in ["src=\"http", "href=\"http", "url(http", "@import"] {
            assert!(
                !html.contains(attr),
                "{uri} references a third party: {attr}"
            );
        }
    }
}

#[tokio::test]
async fn icon_only_controls_have_accessible_names() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    // Every button that holds only an icon carries an aria-label.
    for button in html.split("<button").skip(1) {
        let (attrs, rest) = button.split_once('>').unwrap();
        let content = rest.split("</button>").next().unwrap();
        let mut visible_text = String::new();
        let mut in_tag = false;
        for c in content.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => visible_text.push(c),
                _ => {}
            }
        }
        if content.contains("<svg") && visible_text.trim().is_empty() {
            assert!(
                attrs.contains("aria-label="),
                "unnamed icon button: {attrs}"
            );
        }
    }
}
