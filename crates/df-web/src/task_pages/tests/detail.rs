//! The details column: runs, links, rename, status and mark done.

use axum::http::StatusCode;
use df_core::TITLE_MAX_CHARS;
use serde_json::json;

use super::set_status;
use crate::task_pages::runs_region;
use crate::testkit::{
    TestApp, add_event, chunk, form, header_str, insert_run, new_task, seed, tag_around, text,
};

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
    // One card per run: number, status badge, per model its name and a table with its tokens.
    let runs = &html[html.find("id=\"runs\"").unwrap()..];
    let card = &runs[..runs.find("</li>").unwrap()];
    assert!(card.contains(">#1<"), "{card}");
    assert!(
        card.contains("<time datetime=\"") && card.contains("class=\"ml-auto"),
        "{card}"
    );
    for cell in [
        ">opus<", ">150<", ">14<", ">6 / 2<", ">haiku<", ">9<", ">7<", ">3 / 1<",
    ] {
        assert!(card.contains(cell), "{cell}: {card}");
    }
    assert_eq!(card.matches("<table").count(), 2, "{card}");
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
    assert_eq!(
        app.get("/tasks/DF-99", &cookie).await.status(),
        StatusCode::NOT_FOUND
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
    let script = crate::assets::script_source("workspace.js");
    assert!(
        script.contains(
            "try { this.collapsed = localStorage.getItem('df.details.collapsed') === '1' }"
        )
    );
    assert!(script.contains("try { localStorage.setItem('df.details.collapsed'"));
    assert!(html.contains("x-data=\"workspace\""), "{html}");
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
    let script = crate::assets::script_source("workspace.js");
    let method = |name: &str| {
        let start = script.find(&format!("{name}(e) {{")).expect(name);
        let rest = &script[start..];
        &rest[..rest.find("\n    },").unwrap_or(rest.len())]
    };
    assert!(method("sheetDragCancel").contains("this.dragged = false"));
    assert!(method("sheetDragEnd").contains("this.dragged = d.moved"));
    assert!(!method("sheetDragCancel").contains("d.moved"));
    assert!(html.contains("sheetToggle()") && script.contains("setPointerCapture"));
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
