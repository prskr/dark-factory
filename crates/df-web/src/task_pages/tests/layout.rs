//! Workspace layout, phone sheet, tokens, accessibility and CSS rules.

use crate::testkit::{TestApp, new_task, seed, text};

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
async fn the_workspace_state_is_a_component_script_with_named_pieces() {
    let script = crate::assets::script_source("workspace.js");
    assert!(script.contains("Alpine.data('workspace'"));
    for method in [
        "setCollapsed(value)",
        "sheetToggle()",
        "sheetDragStart(e)",
        "sheetDragMove(e)",
        "sheetDragEnd(e)",
        "sheetDragCancel(e)",
    ] {
        assert!(script.contains(method), "{method}");
    }
    // Thresholds are named values, not numbers buried in the handlers.
    for named in ["closeBelow: 0.25", "fullAbove: 0.7", "dragSlop: 4"] {
        assert!(script.contains(named), "{named}");
    }
    for used in ["this.closeBelow", "this.fullAbove", "this.dragSlop"] {
        assert!(script.contains(used), "{used}");
    }
    // The page uses the component.
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(html.contains("x-data=\"workspace\""), "{html}");
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

#[tokio::test]
async fn the_sheet_does_not_animate_while_it_is_dragged() {
    let html = crate::components::details_panel("Details", maud::html! { "x" }).into_string();
    assert!(html.contains(":data-dragging=\"dragging\""), "{html}");
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("[data-dragging=true]{transition:none"),
        "no lag behind the finger"
    );
    let script = crate::assets::script_source("workspace.js");
    assert!(script.contains("this.dragging = true") && script.contains("this.dragging = false"));
}
