//! The task list: rows, filters, project menu and empty states.

use super::set_status;
use crate::testkit::{TestApp, new_task, seed, tag_around, text};

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
