use axum::http::StatusCode;
use df_core::{Principal, TITLE_MAX_CHARS};
use serde_json::json;

use super::{details_region, head_region, pending_region, runs_region};
use crate::testkit::{
    TestApp, add_event, chunk, form, header_str, insert_run, new_task, seed, tag_around, text,
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
        html.contains("issue: '7'") && html.contains("x-text=\"issue\""),
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
    // One card per run: number, status badge, a table with the tokens per model.
    let runs = &html[html.find("id=\"runs\"").unwrap()..];
    let card = &runs[..runs.find("</li>").unwrap()];
    assert!(card.contains(">#1<"), "{card}");
    assert!(
        card.contains("<time datetime=\"") && card.contains("class=\"ml-auto"),
        "{card}"
    );
    let table = &html[html.find("<th scope=\"col\"").unwrap()..];
    let table = &table[..table.find("</table>").unwrap()];
    for cell in [
        ">opus<", ">150<", ">14<", ">6 / 2<", ">haiku<", ">9<", ">7<", ">3 / 1<",
    ] {
        assert!(table.contains(cell), "{cell}: {table}");
    }
    assert!(card.contains("href=\"#run-1\""), "{card}");
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
    assert!(fragment.contains("also add a test for X") && fragment.contains("1 queued"));
    assert!(
        fragment.contains("aria-label=\"Edit queued message\""),
        "{fragment}"
    );

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
    // The transcript container follows new items while the reader is at the bottom.
    for needle in [
        "id=\"transcript-scroll\"",
        "x-data=\"{ pinned: true }\"",
        "x-init=",
        // Content growth without a DOM mutation (images) and a shrinking viewport (queued
        // messages) also keep the last item in view.
        "new ResizeObserver(down).observe($el)",
        "new ResizeObserver(down).observe($el.querySelector(",
        "x-on:scroll=",
        "&lt; 48",
    ] {
        assert!(html.contains(needle), "{needle}");
    }

    // The regions the stream replaces never contain the draft state.
    let detail = df_db::task_views::get_task_detail_by_key(&app.pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    for region in [
        head_region(&detail, None),
        details_region(&detail),
        runs_region(&[], None),
        pending_region("DF-1", "Linus", "queued", &[], false, None),
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
    assert!(list.contains("id=\"ws-detail\"") && list.contains("No task selected"));
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
    // Counts follow the status filter: WEB has no active task.
    assert!(
        default.contains("data-search=\"website web · 0\""),
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
            && page.contains("x-on:keydown.escape=\"editing = false; $refs.title.value = $refs.title.defaultValue\"")
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
        "/projects/DF/edit",
        "/personas",
        "/personas/new",
        "/credentials",
        "/credentials/new",
        "/tasks/nope",
    ] {
        let html = text(app.get_htmx(uri, &cookie).await).await;
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

#[tokio::test]
async fn hidden_blocks_are_labelled_on_the_task_page_only_with_the_option() {
    let hidden = json!({"userMessage": {"promptId": "p", "content": [
        {"text": {"text": "Fix login"}},
        {"hidden": true, "text": {"text": "call set_task_title"}},
    ]}});
    for (app, shown) in [
        (TestApp::with_hidden_blocks().await, true),
        (TestApp::new().await, false),
    ] {
        let cookie = app.login().await;
        let seed = seed(&app.pool).await;
        let task = new_task(&app.pool, &seed, "t").await;
        let run = insert_run(&app.pool, task.id, 1, "running").await;
        add_event(&app.pool, task.id, run, 1, "t1", hidden.clone()).await;
        let html = text(app.get("/tasks/DF-1", &cookie).await).await;
        assert!(html.contains("Fix login"), "{html}");
        assert_eq!(html.contains("Instruction for the agent"), shown);
        assert_eq!(html.contains("call set_task_title"), shown);
    }
}

fn human() -> Principal {
    Principal::human(df_core::UserId::new(), "Peter")
}

fn text_content(text: &str) -> Vec<df_proto::transcript::v1::ContentBlock> {
    use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
    vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: text.into() })),
        hidden: false,
    }]
}

/// A task whose assignment was delivered and that has one queued message of a board member.
async fn task_with_queued_message(
    app: &TestApp,
    seed: &crate::testkit::Seed,
    text: &str,
) -> (df_db::TaskRow, df_db::TaskMessageRow) {
    let task = new_task(&app.pool, seed, "t").await;
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
    let message = df_db::queue_message(&app.pool, task.id, human(), text_content(text))
        .await
        .unwrap();
    (task, message)
}

#[tokio::test]
async fn queued_stack_sits_above_the_input_and_not_in_the_transcript() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (_, message) = task_with_queued_message(&app, &seed, "also check logout").await;

    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let transcript = &page[page.find("id=\"transcript\"").unwrap()..];
    let transcript_end = transcript
        .find("id=\"pending\"")
        .expect("stack after transcript");
    assert!(!transcript[..transcript_end].contains("also check logout"));
    let stack = &transcript[transcript_end..];
    assert!(stack.find("also check logout").unwrap() < stack.find("id=\"message\"").unwrap());
    assert!(stack.contains(&format!(
        "hx-delete=\"/tasks/DF-1/messages/{}\"",
        message.id
    )));
    // The assignment prompt of the system is not a message of a board member.
    let task = df_db::task_views::get_task_detail_by_key(&app.pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.key, "DF-1");
    assert_eq!(stack.matches("data-queued-id").count(), 1);
}

#[tokio::test]
async fn message_input_can_switch_to_editing_with_cancel_and_save() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    for part in [
        "Editing a queued message",
        "aria-label=\"Send\"",
        "aria-label=\"Save\"",
        "hx-patch=\"/tasks/DF-1/messages/_\"",
        ":hx-patch=\"'/tasks/DF-1/messages/' + editing\"",
        "x-on:click=\"editing = null; draft = saved\"",
        "x-on:message-saved.window=\"editing = null; draft = ''\"",
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
    // Stack and input share one Alpine scope.
    let scope = html
        .find("x-data=\"{ draft: '', saved: '', editing: null }\"")
        .unwrap();
    assert!(
        scope < html.find("id=\"pending\"").unwrap()
            && html.find("id=\"pending\"").unwrap() < html.find("id=\"message\"").unwrap()
    );
}

#[tokio::test]
async fn editing_a_queued_message_replaces_its_text_and_answers_with_the_stack() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;

    let response = app
        .patch(
            &format!("/tasks/DF-1/messages/{}", message.id),
            &form(&[("message", "also check logout")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "message-saved");
    let stack = text(response).await;
    assert!(
        stack.contains("id=\"pending\"")
            && stack.contains("also check logout")
            && !stack.contains(">first<")
    );
    let stored = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    assert_eq!(stored[0].id, message.id);
}

#[tokio::test]
async fn editing_a_forwarded_message_is_a_conflict_that_keeps_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;
    df_db::claim_held_back_messages(&app.pool, task.id)
        .await
        .unwrap();

    let uri = format!("/tasks/DF-1/messages/{}", message.id);
    let edit = app
        .patch(&uri, &form(&[("message", "late")]), &cookie)
        .await;
    assert_eq!(edit.status(), StatusCode::CONFLICT);
    let stack = text(edit).await;
    assert!(
        stack.contains("The agent has already received this message."),
        "{stack}"
    );
    assert!(stack.contains("first") && stack.contains("waiting for delivery"));
    assert!(!stack.contains("sent to the agent"));
    assert!(!stack.contains("Edit queued message"));
    let remove = app.delete(&uri, &cookie).await;
    assert_eq!(remove.status(), StatusCode::CONFLICT);
    assert_eq!(
        df_db::list_undelivered_messages(&app.pool, task.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn empty_edit_is_unprocessable_and_removing_a_queued_message_empties_the_stack() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;
    let uri = format!("/tasks/DF-1/messages/{}", message.id);

    let empty = app.patch(&uri, &form(&[("message", "  ")]), &cookie).await;
    assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(empty).await.contains("Write a message first."));

    let removed = app.delete(&uri, &cookie).await;
    assert_eq!(removed.status(), StatusCode::OK);
    let stack = text(removed).await;
    assert!(stack.contains("id=\"pending\"") && !stack.contains("first"));
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "waiting");
    assert_eq!(
        app.delete("/tasks/DF-1/messages/not-an-id", &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&format!("/tasks/DF-9/messages/{}", message.id), &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn task_list_title_row_holds_the_project_menu_and_the_phone_profile_menu() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks", &cookie).await).await;

    let list = &html[html.find("class=\"ws-list\"").unwrap()..];
    let title_row_end = list
        .find("flex flex-wrap items-center gap-2")
        .expect("chips row");
    let title_row = &list[..title_row_end];
    // Project menu from 768 px, profile menu below; both in the row of the title.
    assert!(
        title_row.contains(">Tasks</h1>")
            && title_row.contains("hidden md:block")
            && title_row.contains("All projects"),
        "{title_row}"
    );
    assert!(
        title_row.contains("aria-label=\"Account menu of Peter\"") && title_row.contains(">P<")
    );
    assert!(title_row.contains("hx-post=\"/logout\"") && title_row.contains("board member"));
    // No active project filter, so no removable chip.
    assert!(!list.contains("Remove project filter"));
}

#[tokio::test]
async fn a_project_filter_shows_as_a_removable_chip_on_phones() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks?project=DF&status=waiting", &cookie).await).await;

    let at = html
        .find("aria-label=\"Remove project filter Dark Factory\"")
        .expect("chip");
    let chip = &html[html[..at].rfind("<a ").unwrap()..];
    let chip = &chip[..chip.find("</a>").unwrap()];
    assert!(
        chip.contains("md:hidden") && chip.contains("min-h-11") && chip.contains("Dark Factory"),
        "{chip}"
    );
    // Removing keeps the other filters.
    assert!(chip.contains("href=\"/tasks?status=waiting\""), "{chip}");
    // The status chips still work and keep the project.
    assert!(html.contains("href=\"/tasks?status=done&amp;project=DF\""));
}

#[tokio::test]
async fn empty_list_and_empty_workspace_have_their_own_states() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let web = other_project(&app, "WEB", "Website").await;
    task_in(&app, &seed, web, "done", "completed").await;

    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(html.contains("No active tasks"), "{html}");
    assert!(html.contains("href=\"/tasks?status=all\"") && html.contains("Show all tasks"));
    assert!(html.contains("No task selected"));
    let at = html.find("No task selected").unwrap();
    assert!(
        html[at..].contains(">New task</button>")
            && html[at..].contains("hx-get=\"/tasks/compose\"")
    );
    // The way out drops the project filter too.
    let filtered = text(app.get("/tasks?project=WEB", &cookie).await).await;
    let link_at = filtered.find("Show all tasks").unwrap();
    let link = &filtered[filtered[..link_at].rfind("<a ").unwrap()..link_at];
    assert!(link.contains("href=\"/tasks?status=all\""), "{link}");
    other_project(&app, "NONE", "No tasks here").await;
    let project_only = text(app.get("/tasks?status=all&project=NONE", &cookie).await).await;
    assert!(
        project_only.contains("Show all tasks") && project_only.contains("No tasks"),
        "{project_only}"
    );
    assert!(!project_only.contains("No tasks yet"));
    // With all tasks shown and none existing there is nothing to show instead.
    sqlx::query("DELETE FROM tasks")
        .execute(&app.pool)
        .await
        .unwrap();
    let none = text(app.get("/tasks?status=all", &cookie).await).await;
    assert!(none.contains("No tasks yet") && !none.contains("Show all tasks"));
    let narrow = text(app.get("/tasks?status=queued", &cookie).await).await;
    assert!(narrow.contains("No queued tasks"));
}

#[tokio::test]
async fn narrower_status_filters_from_links_select_their_statuses_and_no_chip() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    task_in(&app, &seed, seed.project, "queued one", "queued").await;
    task_in(&app, &seed, seed.project, "running one", "running").await;
    task_in(&app, &seed, seed.project, "limited one", "rate_limited").await;
    task_in(&app, &seed, seed.project, "waiting one", "waiting").await;

    let queued = text(app.get("/tasks?status=queued", &cookie).await).await;
    assert!(
        queued.contains("queued one")
            && !queued.contains("running one")
            && !queued.contains("limited one")
    );
    for label in ["Active", "Waiting", "Done", "All"] {
        let at = queued.find(&format!(">{label}</a>")).unwrap();
        let chip = &queued[queued[..at].rfind("<a ").unwrap()..at];
        assert!(!chip.contains("aria-current"), "{label} is current: {chip}");
    }
    assert!(queued.contains("hx-sse:connect=\"/tasks/events?status=queued\""));
    let running = text(app.get("/tasks?status=running", &cookie).await).await;
    assert!(running.contains("running one") && running.contains("limited one"));
    assert!(!running.contains("queued one") && !running.contains("waiting one"));
}

#[tokio::test]
async fn details_can_be_collapsed_to_a_strip_that_is_remembered() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET pr_number = 9, pr_url = 'https://git.example.org/o/r/pulls/9', issue_number = 4 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    // The state is read and written in try/catch, defaults to expanded and applies to the workspace.
    assert!(
        html.contains("localStorage.getItem('df.details.collapsed') === '1'"),
        "{html}"
    );
    assert!(html.contains("try { localStorage.setItem('df.details.collapsed'"));
    assert!(html.contains(":data-collapsed=\"collapsed\""));
    // Hide in the header, show in the strip, both named.
    assert!(
        html.contains("aria-label=\"Hide details\"")
            && html.contains("x-on:click=\"setCollapsed(true)\"")
    );
    assert!(
        html.contains("aria-label=\"Show details\"")
            && html.contains("x-on:click=\"setCollapsed(false)\"")
    );
    // The strip has the status and the links with tooltips.
    let strip = &html[html.find("class=\"ws-strip").unwrap()..];
    let strip = &strip[..strip.find("ws-details-full").unwrap()];
    assert!(strip.contains("aria-label=\"Status: queued\""), "{strip}");
    assert!(
        strip.contains("title=\"Pull request #9\"")
            && strip.contains("href=\"https://git.example.org/o/r/pulls/9\"")
    );
    assert!(strip.contains("title=\"Issue #4\""));
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("data-collapsed=true"),
        "the stylesheet collapses the column"
    );
}

#[tokio::test]
async fn the_details_sheet_closes_with_escape_the_backdrop_and_the_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        html.contains("x-on:keydown.escape.window=\"details = false\""),
        "Escape"
    );
    assert!(
        html.contains("x-show=\"details\" x-cloak x-on:click=\"details = false\""),
        "backdrop"
    );
    assert!(html.contains("aria-label=\"Close details\""));
    // Opening starts at half height; the state and its handlers live in the workspace scope.
    assert!(
        html.contains("x-on:click=\"details = true; sheet = 'half'\""),
        "{html}"
    );
    // A cancelled pointer (no click follows) resets `dragged`; only pointerup sets it.
    let state = super::WORKSPACE_STATE;
    let method = |name: &str| {
        let start = state.find(&format!("{name}(e) {{")).expect(name);
        let rest = &state[start..];
        &rest[..rest.find("\n  },").unwrap_or(rest.len())]
    };
    assert!(method("sheetDragCancel").contains("this.dragged = false"));
    assert!(method("sheetDragEnd").contains("this.dragged = d.moved"));
    assert!(!method("sheetDragCancel").contains("d.moved"));
    assert!(html.contains("sheetToggle()") && html.contains("setPointerCapture"));
}

#[tokio::test]
async fn rename_has_save_and_cancel() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "keep me").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let form_at = html.find("hx-post=\"/tasks/DF-1/title\"").unwrap();
    let rename = &html[form_at..html[form_at..].find("</form>").unwrap() + form_at];
    assert!(
        rename.contains(">Save</button>") && rename.contains(">Cancel</button>"),
        "{rename}"
    );
    assert!(rename.contains("editing = false; $refs.title.value = $refs.title.defaultValue"));
    let title: String = sqlx::query_scalar("SELECT title FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(title, "keep me");
}

#[tokio::test]
async fn composer_is_a_frameless_card_with_a_named_round_submit_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;

    assert!(
        html.contains("bg-transparent"),
        "no solid modal frame: {html}"
    );
    let chips = html.find("name=\"project\"").unwrap();
    let card = html
        .find("rounded-2xl border border-line bg-panel")
        .unwrap();
    let description = html.find("name=\"description\"").unwrap();
    let assignee = html.find("name=\"persona\"").unwrap();
    assert!(chips < card && card < description, "chips above the card");
    assert!(card < assignee, "assignee inside the card");
    let submit = &html[html.find("aria-label=\"Create task\"").unwrap()..];
    assert!(submit.contains("rounded-full") && submit.contains("<svg"));
    assert!(html.find("type=\"submit\"").unwrap() > assignee);
}

#[tokio::test]
async fn a_failed_run_shows_one_line_of_the_error_and_the_full_text_in_a_disclosure() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let run = insert_run(&app.pool, task.id, 1, "failed").await;
    let error = format!(
        "sandbox failed to start: {}\nsecond line of the error",
        "x".repeat(120)
    );
    sqlx::query("UPDATE runs SET outcome = 'failed', detail = $2 WHERE id = $1")
        .bind(run)
        .bind(&error)
        .execute(&app.pool)
        .await
        .unwrap();
    insert_run(&app.pool, task.id, 2, "running").await;
    sqlx::query("UPDATE runs SET detail = 'short reason' WHERE number = 2")
        .execute(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    // Newest first: run 2 (short reason), then run 1 (failed).
    let runs = &html[html.find("id=\"runs\"").unwrap()..];
    let runs = &runs[runs.find("<ul").unwrap()..];
    let (second, first) = runs.split_once("</li>").unwrap();
    // Header: #n, status, start time on the right.
    assert!(
        first.contains(">#") && first.contains(">failed<") && first.contains("ml-auto"),
        "{first}"
    );
    // The summary is one truncated line; the full text is in the disclosure body.
    assert!(
        first.contains("<details")
            && first
                .contains("<summary class=\"cursor-pointer truncate\">sandbox failed to start:"),
        "{first}"
    );
    assert!(
        !first[first.find("<summary").unwrap()..first.find("</summary>").unwrap()]
            .contains("second line")
    );
    assert!(first[first.find("</summary>").unwrap()..].contains("second line of the error"));
    // A short reason needs no disclosure.
    assert!(
        second.contains("short reason") && !second.contains("<details"),
        "{second}"
    );
}

#[tokio::test]
async fn links_show_when_the_task_has_them_and_nothing_when_it_has_none() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    let none = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(none.contains("No pull request or issue yet."));
    sqlx::query("UPDATE tasks SET pr_number = 9, pr_url = 'https://git.example.org/o/r/pulls/9', pr_title = 'Fix login', issue_number = 4 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let links = &html[html.find(">Links</h2>").unwrap()..];
    assert!(
        links.contains("PR #9") && links.contains("Fix login") && links.contains("Issue #4"),
        "{links}"
    );
}

#[tokio::test]
async fn the_collapsed_strip_shows_status_and_links_without_any_heading() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET pr_number = 9, pr_url = 'https://git.example.org/o/r/pulls/9', issue_number = 4 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let strip = &html[html.find("class=\"ws-strip").unwrap()..];
    let strip = &strip[..strip.find("ws-details-full").unwrap()];
    assert!(
        !strip.contains("<h2") && !strip.contains("Details<"),
        "{strip}"
    );
    assert!(
        strip.contains("Status: queued")
            && strip.contains("Pull request #9")
            && strip.contains("Issue #4")
    );
    // The heading row (header with the title) is hidden in the collapsed state by a rule that no
    // display utility can override.
    let head = &html[html.find("class=\"ws-details-head").unwrap()..];
    assert!(head.starts_with("class=\"ws-details-head\""), "{head}");
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("[data-collapsed=true] .ws-details-head"),
        "{css}"
    );
}

#[tokio::test]
async fn split_screen_widths_keep_the_rail_and_the_list_next_to_the_transcript() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    // The switch is at 768 px: no 1024 px layout class is left in the shell.
    assert!(html.contains("md:flex") && html.contains("md:hidden"));
    assert!(
        !html.contains(" lg:flex") && !html.contains(" lg:hidden") && !html.contains("lg:block"),
        "{html}"
    );
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("@media (min-width:768px)"),
        "details panel from 768 px"
    );
    // List and transcript are both shown from 768 px, and the list stops being a flexible
    // column there (fixed width), so they sit side by side.
    assert!(
        css.contains("@media (min-width:48rem){.ws-list{")
            && css.contains("flex:none;display:flex}}"),
        "list column from 768 px"
    );
    assert!(
        css.contains("@media (min-width:48rem){.ws-main{display:flex}}"),
        "transcript column from 768 px"
    );
}

#[tokio::test]
async fn the_send_button_sits_inside_the_message_box() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let start = html.find("id=\"message-box\"").unwrap();
    let after = &html[start..];
    let mut depth = 0;
    let mut end = 0;
    for (i, _) in after.match_indices(['<']) {
        if after[i..].starts_with("<div") {
            depth += 1;
        }
        if after[i..].starts_with("</div") {
            depth -= 1;
            if depth == 0 {
                end = i;
                break;
            }
        }
    }
    let message_box = &after[..end];
    assert!(
        message_box.contains("<textarea") && message_box.contains("aria-label=\"Send\""),
        "{message_box}"
    );
    assert!(message_box.contains("rounded-full") && message_box.contains("aria-label=\"Save\""));
    assert!(
        message_box.find("<textarea").unwrap() < message_box.find("aria-label=\"Send\"").unwrap()
    );
}

#[tokio::test]
async fn the_composer_has_a_link_issue_plus_on_the_left_and_the_assignee_next_to_submit() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    assert!(!html.contains("Options"), "no Options text");
    let plus = html.find("aria-label=\"Link issue\"").unwrap();
    let assignee = html.find("name=\"persona\"").unwrap();
    let submit = html.find("aria-label=\"Create task\"").unwrap();
    let description = html.find("name=\"description\"").unwrap();
    assert!(
        description < plus && plus < assignee && assignee < submit,
        "{html}"
    );
    // Nothing but the end of the picker sits between it and the submit button.
    let between = &html[assignee..submit];
    assert!(
        between.matches("<button").count() == 1 && between.matches("<select").count() == 0,
        "{between}"
    );
    // The button itself: round, 44 px below 1024 px (36 px from there), with its icon inside.
    let start = html[..plus].rfind("<button").unwrap();
    let end = start + html[start..].find("</button>").unwrap();
    let plus_button = &html[start..end];
    assert!(
        plus_button.contains("rounded-full")
            && plus_button.contains("size-11")
            && plus_button.contains("lg:size-9")
            && !plus_button.contains("md:size-9")
            && plus_button.contains("<svg"),
        "{plus_button}"
    );
    assert!(
        html.contains("aria-label=\"Remove linked issue\"")
            && html.contains("aria-label=\"Linked issue\"")
    );
}

#[tokio::test]
async fn forwarded_messages_wait_for_delivery_and_say_so_on_an_ended_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, _) = task_with_queued_message(&app, &seed, "first").await;
    df_db::claim_held_back_messages(&app.pool, task.id)
        .await
        .unwrap();

    let waiting = text(app.get("/tasks/DF-1", &cookie).await).await;
    let stack = &waiting[waiting.find("id=\"pending\"").unwrap()..];
    assert!(stack.contains("waiting for delivery"), "{stack}");
    assert!(!stack.contains("not delivered") && !stack.contains("sent to the agent"));

    for status in ["failed", "completed"] {
        sqlx::query("UPDATE tasks SET status = $1")
            .bind(status)
            .execute(&app.pool)
            .await
            .unwrap();
        let ended = text(app.get("/tasks/DF-1", &cookie).await).await;
        let stack = &ended[ended.find("id=\"pending\"").unwrap()..];
        assert!(
            stack.contains("not delivered, sent with the next run"),
            "{status}: {stack}"
        );
        assert!(!stack.contains("waiting for delivery"));
    }
}

#[tokio::test]
async fn a_list_row_has_key_status_and_time_on_the_top_line_before_the_title() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Hello World").await;
    sqlx::query("UPDATE tasks SET status = 'failed', status_line = 'tests red' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks?status=all", &cookie).await).await;
    let row = &html[html.find("<li id=\"row-DF-1\"").unwrap()..];
    let row = &row[..row.find("</li>").unwrap()];
    let at = |needle: &str| {
        row.find(needle)
            .unwrap_or_else(|| panic!("{needle}: {row}"))
    };
    let key = at(">DF-1<");
    let badge = at(">failed<");
    let time = at("<time ");
    let title = at(">Hello World<");
    assert!(key < badge && badge < time && time < title, "{row}");
    // The time is pushed to the right; the status line stays below the title.
    assert!(row.contains("ml-auto"), "{row}");
    assert!(title < at(">tests red<"));
}

#[tokio::test]
async fn the_empty_states_follow_the_mockup() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let web = other_project(&app, "WEB", "Website").await;
    task_in(&app, &seed, web, "done", "completed").await;
    let html = text(app.get("/tasks", &cookie).await).await;

    // List: title, one sentence about what shows up here, and a text link (not a button).
    let list = &html[html.find("No active tasks").unwrap()..];
    let list = &list[..list.find("</li>").unwrap()];
    assert!(
        list.contains("Queued, running and rate-limited tasks show up here."),
        "{list}"
    );
    let link = tag_around(list, "Show all tasks");
    assert!(
        link.contains("text-accent-text") && !link.contains("border"),
        "{link}"
    );
    for (status, sentence) in [
        (
            "waiting",
            "Tasks that wait for you or hit a limit show up here.",
        ),
        ("done", "Completed and failed tasks show up here."),
        ("queued", "Tasks waiting for a run show up here."),
        ("running", "Running and rate-limited tasks show up here."),
    ] {
        sqlx::query("DELETE FROM tasks")
            .execute(&app.pool)
            .await
            .unwrap();
        let page = text(app.get(&format!("/tasks?status={status}"), &cookie).await).await;
        assert!(page.contains(sentence), "{status}: {sentence}");
    }

    // Workspace: a round accent icon, title, sentence and a primary button with a plus.
    let workspace = &html[html.find("flex max-w-sm").unwrap()..];
    let workspace = &workspace[..workspace.find("</section>").unwrap()];
    assert!(
        workspace.contains("size-14")
            && workspace.contains("rounded-full")
            && workspace.contains("bg-accent-soft"),
        "{workspace}"
    );
    assert!(
        workspace.contains(
            "Pick a task from the list, or describe a new one and a persona starts on it."
        ),
        "{workspace}"
    );
    let button = &workspace[workspace.find("<button").unwrap()..];
    assert!(
        button.find("<svg").unwrap() < button.find("New task").unwrap(),
        "plus icon before the label: {button}"
    );
    assert!(button.contains("bg-accent"));
}

#[tokio::test]
async fn the_workspace_state_is_a_documented_constant_with_named_pieces() {
    use super::WORKSPACE_STATE;
    assert!(
        WORKSPACE_STATE.lines().count() > 15,
        "written over several lines"
    );
    for method in [
        "setCollapsed(value)",
        "sheetToggle()",
        "sheetDragStart(e)",
        "sheetDragMove(e)",
        "sheetDragEnd(e)",
        "sheetDragCancel(e)",
    ] {
        assert!(WORKSPACE_STATE.contains(method), "{method}");
    }
    // Thresholds are named values, not numbers buried in the handlers.
    for named in ["closeBelow: 0.25", "fullAbove: 0.7", "dragSlop: 4"] {
        assert!(WORKSPACE_STATE.contains(named), "{named}");
    }
    for used in ["this.closeBelow", "this.fullAbove", "this.dragSlop"] {
        assert!(WORKSPACE_STATE.contains(used), "{used}");
    }
    // The page uses the constant.
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(html.contains("closeBelow: 0.25") && html.contains("sheetDragCancel(e)"));
}

#[tokio::test]
async fn the_phone_sheet_keeps_handle_and_head_fixed_and_scrolls_only_its_body() {
    let html = crate::components::details_panel("Details", maud::html! { "x" }).into_string();
    let handle = html.find("aria-label=\"Resize details\"").unwrap();
    let head = html.find("class=\"ws-details-head\"").unwrap();
    let body = html.find("class=\"ws-details-body").unwrap();
    assert!(handle < head && head < body);
    let body_tag = &html[body..body + html[body..].find('>').unwrap()];
    assert!(
        body_tag.contains("min-h-0")
            && body_tag.contains("flex-1")
            && body_tag.contains("overflow-y-auto")
            && body_tag.contains("md:flex-col"),
        "{body_tag}"
    );
    let css = crate::app_css().unwrap();
    let phone = &css[css.find(".ws-details{").expect("sheet rule")..];
    let phone = &phone[..phone.find('}').unwrap()];
    assert!(
        phone.contains("flex-direction:column")
            && phone.contains("overflow:hidden")
            && phone.contains("display:flex"),
        "{phone}"
    );
}

/// The declarations of the first rule that starts with `selector` (given with its `{`).
fn css_rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let start = css.find(selector).unwrap_or_else(|| panic!("{selector}"));
    let body = &css[start + selector.len()..];
    &body[..body.find('}').unwrap()]
}

#[test]
fn tool_rows_share_one_line_between_consecutive_calls() {
    let css = crate::app_css().unwrap();
    let rule = |selector: &str| css_rule(&css, selector);
    let row = rule("[data-tool],[data-system]{");
    assert!(
        row.contains("border-top") && row.contains("border-bottom"),
        "{row}"
    );
    let shared = rule(":is([data-tool],[data-system])+:is([data-tool],[data-system]){");
    assert!(shared.contains("border-top-width:0"), "{shared}");
}

#[test]
fn the_runs_fill_the_details_column_and_keep_room_for_two_cards() {
    let css = crate::app_css().unwrap();
    let rule = |selector: &str| css_rule(&css, selector);
    // The panel is a clipped column at every width; the body (or the runs list) scrolls.
    let panel = rule(".ws-details{");
    assert!(
        panel.contains("display:flex")
            && panel.contains("flex-direction:column")
            && panel.contains("overflow:hidden"),
        "{panel}"
    );
    let runs = rule(".ws-runs{");
    assert!(
        runs.contains("flex:1;") && runs.contains("min-height:0"),
        "{runs}"
    );
    assert!(rule(".ws-runs:has(details[open]){").contains("min-height:17rem"));
    assert!(rule(".ws-runs details::details-content{").contains("flex:1;"));
    let region = runs_region(&[], None).into_string();
    assert!(region.contains("ws-runs"), "{region}");
}

#[tokio::test]
async fn the_description_is_cut_after_three_lines_with_the_full_text_as_title() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET description = 'Do it <now>' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let desc = tag_around(&html, "Do it &lt;now&gt;");
    assert!(
        desc.contains("line-clamp-3") && desc.contains("title=\"Do it &lt;now&gt;\""),
        "{desc}"
    );
}

#[tokio::test]
async fn the_sheet_does_not_animate_while_it_is_dragged() {
    let html = crate::components::details_panel("Details", maud::html! { "x" }).into_string();
    assert!(html.contains(":data-dragging=\"dragging\""), "{html}");
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("[data-dragging=true]{transition:none"),
        "no lag behind the finger"
    );
    assert!(
        super::WORKSPACE_STATE.contains("this.dragging = true")
            && super::WORKSPACE_STATE.contains("this.dragging = false")
    );
}

#[tokio::test]
async fn the_phone_empty_list_has_the_mockup_sizes_and_the_new_task_hint() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/tasks", &cookie).await).await;
    let panel = &html[html[..html.find("No active tasks").unwrap()]
        .rfind("<div")
        .unwrap()..];
    let panel = &panel[..panel.find("</li>").unwrap()];
    assert!(
        !panel.contains("max-md:text-base"),
        "the sentence stays 14 px"
    );
    for class in ["max-md:py-16", "max-md:gap-2.5", "max-md:text-[17px]"] {
        assert!(panel.contains(class), "{class}: {panel}");
    }
    assert!(
        panel.contains("Tap New task to start one.") && panel.contains("md:hidden"),
        "{panel}"
    );
}

#[tokio::test]
async fn a_list_row_title_is_one_truncated_line_with_the_full_title_on_hover() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(
        &app.pool,
        &seed,
        "A very long title that would wrap in the narrow list column",
    )
    .await;
    let html = text(app.get("/tasks?status=all", &cookie).await).await;
    let row = &html[html.find("<li id=\"row-DF-1\"").unwrap()..];
    let title = tag_around(row, "A very long title");
    assert!(
        title.contains("truncate") && !title.contains("break-words"),
        "{title}"
    );
    assert!(
        title.contains("title=\"A very long title that would wrap in the narrow list column\""),
        "{title}"
    );
}

#[tokio::test]
async fn the_details_overview_has_the_sections_of_the_mockup_in_order() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET status_line = 'Writing the regression test', description = 'Do it' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    insert_run(&app.pool, task.id, 1, "failed").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    // The sections above Runs keep their height when the runs area fills the column.
    assert!(html.contains("id=\"task-details\" class=\"md:shrink-0\""));
    let full = &html[html.find("class=\"ws-details-full").unwrap()..];
    let at = |needle: &str| {
        full.find(needle)
            .unwrap_or_else(|| panic!("{needle}: {full}"))
    };
    let order = [
        at(">Now</h2>"),
        at(">Links</h2>"),
        at("<dl "),
        at(">Description<"),
        at(">Runs<"),
    ];
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    // Now: the status line, next to a status dot.
    let now = &full[at(">Now</h2>")..at(">Links</h2>")];
    assert!(
        now.contains("Writing the regression test") && now.contains("rounded-full"),
        "{now}"
    );
    // No pull request or issue yet: the section stays and says so.
    let links = &full[at(">Links</h2>")..at("<dl ")];
    assert!(links.contains("No pull request or issue yet."), "{links}");
    // The persona shows its harness, muted; the branch is mono.
    let dl = &full[at("<dl ")..at(">Description<")];
    assert!(
        dl.contains("Linus")
            && dl.contains("<span class=\"font-normal text-muted\">· Claude Code</span>"),
        "{dl}"
    );
    assert!(dl.contains("font-mono text-xs"), "{dl}");
}

#[tokio::test]
async fn the_now_section_falls_back_to_the_status_and_links_replace_the_empty_text() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET status = 'rate_limited', pr_number = 9, pr_url = 'https://git.example.org/o/r/pulls/9' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let now = &html[html.find(">Now</h2>").unwrap()..];
    let now = &now[..now.find(">Links</h2>").unwrap()];
    assert!(now.contains("rate limited"), "{now}");
    assert!(!html.contains("No pull request or issue yet."));
}

#[tokio::test]
async fn a_run_without_usage_says_which_model_was_meant_and_that_nothing_was_recorded() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "failed").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let runs = &html[html.find("id=\"runs\"").unwrap()..];
    assert!(
        runs.contains("default model · no token usage recorded"),
        "{runs}"
    );
    assert!(!runs.contains("<table"));

    sqlx::query("UPDATE personas SET model = 'opus'")
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(html.contains("opus · no token usage recorded"));
}

#[tokio::test]
async fn the_selected_row_has_an_accent_edge_and_the_others_a_transparent_one() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "first").await;
    new_task(&app.pool, &seed, "second").await;
    let html = text(app.get("/tasks/DF-2", &cookie).await).await;
    let row = &html[html.find("<li id=\"row-DF-1\"").unwrap()..];
    let a_tag = tag_around(row, "hx-push-url");
    for class in [
        "border-l-[3px]",
        "border-l-transparent",
        "pl-[17px]",
        "pr-5",
        "aria-[current=page]:border-l-accent",
        "aria-[current=page]:bg-accent-soft",
    ] {
        assert!(a_tag.contains(class), "{class}: {a_tag}");
    }
    // Title 15 px, key and time mono.
    assert!(row.contains("text-[15px] font-medium"), "{row}");
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("aria-current=page"),
        "the selected-row rule is in the stylesheet"
    );
    assert!(
        css.contains("border-left-color:var(--accent)")
            || css.contains("border-left-color:var(--color-accent)"),
        "accent edge"
    );
}

#[tokio::test]
async fn the_composer_chips_are_rounded_rectangles_the_persona_has_no_border_and_create_is_an_arrow()
 {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    let project = tag_around(&html, "name=\"project\"");
    assert!(
        project.contains("rounded-[10px]") && !project.contains("rounded-full"),
        "{project}"
    );
    let persona = tag_around(&html, "name=\"persona\"");
    assert!(
        !persona.contains(" border ")
            && !persona.contains("border-line")
            && persona.contains("bg-transparent"),
        "{persona}"
    );
    // The arrow-up of the mockup, not the paper plane.
    let submit = &html[html.find("aria-label=\"Create task\"").unwrap()..];
    let submit = &submit[..submit.find("</button>").unwrap()];
    assert!(
        submit.contains("M12 19V5") && submit.contains("m5 12 7-7 7 7"),
        "{submit}"
    );
    // No accent outline on the text; the card shows focus with a stronger border instead.
    let textarea = tag_around(&html, "name=\"description\"");
    assert!(
        textarea.contains("focus-visible:outline-none"),
        "{textarea}"
    );
    let card = tag_around(&html, "rounded-2xl border border-line bg-panel");
    assert!(card.contains("focus-within:border-line-strong"), "{card}");
    // Neutral: no accent anywhere in the focus treatment (the user asked to drop it).
    assert!(
        !card.contains("accent") && !textarea.contains("accent"),
        "{card} {textarea}"
    );
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("focus-within"),
        "focus-within rule in the stylesheet"
    );
    // The close button has no background, not even on hover.
    let close = tag_around(&html, "aria-label=\"Close\"");
    assert!(!close.contains("hover:bg-"), "{close}");
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("focus-within"),
        "focus-within rule in the stylesheet"
    );
}

#[tokio::test]
async fn the_description_header_matches_the_other_sections_and_runs_has_no_bottom_border() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    sqlx::query("UPDATE tasks SET description = 'Do it' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&app.pool)
        .await
        .unwrap();
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let summary = tag_around(&html, ">Description<");
    for class in ["uppercase", "tracking-wide", "text-xs", "text-faint"] {
        assert!(summary.contains(class), "{class}: {summary}");
    }
    // Nothing adds space between the details and the runs, and Runs closes the list.
    assert!(!html.contains("ws-details-pad"), "dead wrapper class");
    let runs = tag_around(&html, "id=\"runs\"");
    assert!(!runs.contains("border-b"), "{runs}");
    let css = crate::app_css().unwrap();
    assert!(!css.contains("ws-details-pad"));
    assert!(
        css.contains("group-open\\:hidden"),
        "the utility is generated"
    );
}

#[tokio::test]
async fn a_running_run_without_usage_also_says_nothing_was_recorded() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    insert_run(&app.pool, task.id, 1, "running").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        html.contains("default model · no token usage recorded"),
        "{html}"
    );
}

#[tokio::test]
async fn the_link_issue_plus_opens_a_popover_with_the_number_field_and_a_chip_shows_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;

    // The plus: named, expanded state bound, active look while open (accent border + soft fill).
    let plus = tag_around(&html, "aria-label=\"Link issue\"");
    assert!(
        plus.contains(":aria-expanded=\"linking\"")
            && plus.contains("x-on:click=\"linking = !linking\""),
        "{plus}"
    );
    assert!(plus.contains("border-accent-text bg-accent-soft"), "{plus}");
    // The popover: anchored below the card on the left, 280 px, panel surface, named.
    let popover = tag_around(&html, "aria-label=\"Linked issue\"");
    for class in [
        "absolute",
        "left-3",
        "top-[calc(100%-8px)]",
        "w-[280px]",
        "rounded-xl",
        "border",
        "bg-panel",
        "p-3",
        "shadow-lg",
    ] {
        assert!(popover.contains(class), "{class}: {popover}");
    }
    // On phones (bottom sheet) it sits inline below the card instead.
    assert!(
        popover.contains("max-md:static") && popover.contains("max-md:w-full"),
        "{popover}"
    );
    assert!(popover.contains("x-show=\"linking\""));
    // Label, mono # prefix, numeric field under the unchanged form field name, help sentence.
    let at = html.find("aria-label=\"Linked issue\"").unwrap();
    let inside = &html[at..];
    assert!(inside.contains(">Linked issue</label>"), "{inside}");
    assert!(inside.contains(">#</span>") && inside.contains("font-mono"));
    let field = tag_around(&html, "id=\"issue_number\"");
    assert!(
        field.contains("name=\"issue_number\"")
            && field.contains("inputmode=\"numeric\"")
            && field.contains("x-model=\"issue\""),
        "{field}"
    );
    assert!(
        inside.contains("An existing issue in ")
            && inside.contains("tracker that this task works on."),
        "{inside}"
    );
    // The chip above the card: "Issue #n" with a remove button that clears the number.
    let chip = &html[html.find("x-show=\"issue\"").unwrap()..];
    let chip = &chip[..chip.find("</span></span>").unwrap_or(400.min(chip.len()))];
    assert!(
        chip.contains("Issue") && chip.contains("x-text=\"issue\"") && chip.contains("font-mono"),
        "{chip}"
    );
    let remove = tag_around(&html, "aria-label=\"Remove linked issue\"");
    assert!(remove.contains("x-on:click=\"issue = ''\""), "{remove}");
    // Escape and a click outside close the popover only.
    let card = tag_around(&html, "rounded-2xl border border-line bg-panel");
    assert!(
        card.contains("relative") && card.contains("x-on:click.outside=\"linking = false\""),
        "{card}"
    );
    assert!(card.contains("x-on:keydown.escape=\"if (linking) { $event.preventDefault(); $event.stopPropagation(); linking = false }\""), "{card}");
}

#[tokio::test]
async fn both_message_boxes_signal_focus_neutrally() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let compose = text(app.get("/tasks/compose", &cookie).await).await;
    for (what, html, box_marker, textarea_marker) in [
        ("message box", &page, "id=\"message-box\"", "id=\"message\""),
        (
            "composer",
            &compose,
            "rounded-2xl border border-line bg-panel",
            "id=\"description\"",
        ),
    ] {
        let frame = tag_around(html, box_marker);
        let textarea = tag_around(html, textarea_marker);
        // Rest: line; focus inside: the stronger neutral line; never an accent.
        assert!(
            frame.contains("border-line ") && frame.contains("focus-within:border-line-strong"),
            "{what}: {frame}"
        );
        assert!(
            !frame.contains("border-line-strong bg-panel"),
            "{what} is not strong at rest: {frame}"
        );
        assert!(
            !frame.contains("accent") && !textarea.contains("accent"),
            "{what}: {frame} {textarea}"
        );
        assert!(
            textarea.contains("focus-visible:outline-none"),
            "{what}: {textarea}"
        );
    }
}

async fn set_status(app: &TestApp, status: &str) {
    sqlx::query("UPDATE tasks SET status = $1")
        .bind(status)
        .execute(&app.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn mark_done_is_offered_in_the_status_select_only_for_allowed_statuses() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;

    for (status, offered) in [
        ("waiting", true),
        ("in_review", true),
        ("rate_limited", true),
        ("limit_reached", true),
        ("failed", true),
        ("queued", false),
        ("running", false),
        ("completed", false),
    ] {
        set_status(&app, status).await;
        let html = text(app.get("/tasks/DF-1", &cookie).await).await;
        let dl = &html[html.find("<dl ").unwrap()..];
        let dl = &dl[..dl.find("</dl>").unwrap()];
        assert_eq!(
            dl.contains("hx-post=\"/tasks/DF-1/done\""),
            offered,
            "{status}"
        );
        assert_eq!(dl.contains(">Mark done<"), offered, "{status}");
        // The Status row comes first, above Project.
        assert!(
            dl.find(">Status<").unwrap() < dl.find(">Project<").unwrap(),
            "{status}"
        );
        assert!(dl.contains(&status.replace('_', " ")), "{status}: {dl}");
    }
}

#[tokio::test]
async fn posting_mark_done_completes_the_task_and_answers_the_details() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "in_review").await;

    let before = text(app.get("/tasks?status=done", &cookie).await).await;
    assert!(!before.contains("DF-1"), "{before}");
    let response = app.post("/tasks/DF-1/done", "", &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("id=\"task-details\""), "{html}");
    assert!(
        html.contains("completed") && !html.contains("Mark done"),
        "{html}"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "completed");
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'task.done'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(audited, 1);

    // The task moved to the "Done" filter and its header shows the completed badge.
    let done = text(app.get("/tasks?status=done", &cookie).await).await;
    assert!(done.contains("DF-1"), "{done}");
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let head = &page[page.find("id=\"task-head\"").unwrap()..];
    let head = &head[..head.find("</header>").unwrap()];
    assert!(
        head.contains("bg-status-completed-bg text-status-completed-fg"),
        "{head}"
    );
}

#[tokio::test]
async fn mark_done_is_refused_for_working_tasks_and_unknown_keys() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    set_status(&app, "running").await;

    let response = app.post("/tasks/DF-1/done", "", &cookie).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    // The answer shows the task as it is, so the page catches up.
    assert!(text(response).await.contains("id=\"task-details\""));
    let status: String = sqlx::query_scalar("SELECT status FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "running");
    assert_eq!(
        app.post("/tasks/DF-9/done", "", &cookie).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn in_review_has_its_own_badge_and_belongs_to_the_waiting_filter() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "review me").await;
    set_status(&app, "in_review").await;

    let waiting = text(app.get("/tasks?status=waiting", &cookie).await).await;
    assert!(waiting.contains("review me"), "{waiting}");
    assert!(
        waiting.contains("bg-status-review-bg text-status-review-fg")
            && waiting.contains("in review"),
        "{waiting}"
    );
    let active = text(app.get("/tasks", &cookie).await).await;
    assert!(!active.contains("review me"));
}
