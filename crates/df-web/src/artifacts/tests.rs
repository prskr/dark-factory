//! Tests of the artifact cards, view, routes and their place on the task page.

use axum::http::StatusCode;
use serde_json::json;

use crate::testkit::{TestApp, add_event, header_str, insert_run, new_task, seed, text};

/// A PNG of 3 x 2 px.
const PNG_HEX: &str = "89504e470d0a1a0a0000000d49484452000000030000000208020000001216f14d0000001049444154789c63f8cfc000410c70160041d205fb87f0b9480000000049454e44ae426082";

fn png() -> Vec<u8> {
    hex::decode(PNG_HEX).unwrap()
}

async fn write(app: &TestApp, key: &str, name: &str, bytes: &[u8]) {
    let task: uuid::Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let limits = df_core::ArtifactsConfig::default();
    let sniffed = df_core::artifacts::check_artifact(name, bytes, &limits).unwrap();
    df_db::artifacts::write_artifact(
        &app.pool,
        df_db::artifacts::NewArtifact {
            task_id: df_core::TaskId::from_uuid(task),
            name,
            sniffed: &sniffed,
            content: bytes,
            max_per_task: 50,
        },
    )
    .await
    .unwrap();
}

async fn app_with_task() -> (TestApp, String) {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "Research").await;
    (app, cookie)
}

#[tokio::test]
async fn the_tab_lists_cards_newest_first_with_image_and_text_previews() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "findings.md", b"# Findings\nfirst line").await;
    write(&app, "DF-1", "login.png", &png()).await;

    let html = text(app.get_htmx("/tasks/DF-1/artifacts", &cookie).await).await;

    let image = html.find("login.png").unwrap();
    let doc = html.find("findings.md").unwrap();
    assert!(image < doc, "{html}");
    assert!(
        html.contains("src=\"/tasks/DF-1/artifacts/login.png/raw\""),
        "{html}"
    );
    assert!(html.contains("# Findings\nfirst line"), "{html}");
    assert!(html.contains("Markdown") && html.contains("PNG"), "{html}");
    assert!(
        html.contains("x-on:click.prevent=\"open('findings.md')\""),
        "{html}"
    );
    // The tab brings the tab bar in out of band.
    assert!(
        html.contains("hx-swap-oob=\"outerHTML\"") && html.contains("role=\"tablist\""),
        "{html}"
    );
}

#[tokio::test]
async fn card_previews_stop_after_eight_lines() {
    let (app, cookie) = app_with_task().await;
    let long = (1..=12).fold(String::new(), |text, i| text + &format!("line {i}\n"));
    write(&app, "DF-1", "long.txt", long.as_bytes()).await;

    let html = text(app.get_htmx("/tasks/DF-1/artifacts", &cookie).await).await;

    assert!(
        html.contains("line 8") && !html.contains("line 9"),
        "{html}"
    );
}

#[tokio::test]
async fn the_task_page_has_the_artifacts_tab_only_with_artifacts() {
    let (app, cookie) = app_with_task().await;

    let none = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        none.contains("<h2 class=\"text-base font-semibold\">Details</h2>"),
        "{none}"
    );
    assert!(!none.contains("role=\"tablist\""), "{none}");

    write(&app, "DF-1", "findings.md", b"x").await;
    write(&app, "DF-1", "sources.md", b"y").await;
    let two = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(two.contains("role=\"tablist\""), "{two}");
    assert!(
        two.contains(">Artifacts <span class=\"rounded-full bg-sunk px-1.5 text-xs\">2</span>"),
        "{two}"
    );
    assert!(
        two.contains("findings.md") && two.contains("sources.md"),
        "{two}"
    );
    assert!(
        two.contains("hx-trigger=\"artifacts-changed from:body\""),
        "{two}"
    );
    assert!(two.contains("x-data=\"artifactSplit("), "{two}");
}

#[tokio::test]
async fn the_first_artifact_turns_the_header_into_the_tab_bar_out_of_band() {
    let (app, cookie) = app_with_task().await;
    let empty = text(app.get_htmx("/tasks/DF-1/artifacts", &cookie).await).await;
    assert!(
        empty.contains("<h2 class=\"text-base font-semibold\">Details</h2>"),
        "{empty}"
    );

    write(&app, "DF-1", "findings.md", b"x").await;
    let first = text(app.get_htmx("/tasks/DF-1/artifacts", &cookie).await).await;
    assert!(
        first.contains("id=\"ws-details-heading\"") && first.contains("role=\"tablist\""),
        "{first}"
    );
}

#[tokio::test]
async fn markdown_is_rendered_with_inline_images_of_the_same_task_and_has_a_source_toggle() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "login.png", &png()).await;
    let report = "# Report\n\n![Login](login.png)\n\n![x](https://example.com/x.png)\n\n```rust\nfn main() {}\n```\n";
    write(&app, "DF-1", "report.md", report.as_bytes()).await;

    let html = text(
        app.get_htmx("/tasks/DF-1/artifacts/report.md", &cookie)
            .await,
    )
    .await;

    assert!(html.contains("<h1>Report</h1>"), "{html}");
    assert!(
        html.contains("<img loading=\"lazy\" src=\"/tasks/DF-1/artifacts/login.png/raw\""),
        "{html}"
    );
    assert!(
        html.contains("href=\"https://example.com/x.png\""),
        "{html}"
    );
    assert!(html.contains("hl-"), "{html}");
    assert!(
        html.contains(">Preview<") && html.contains(">Source<"),
        "{html}"
    );
    assert!(html.contains("x-show=\"source\""), "{html}");
}

#[tokio::test]
async fn other_text_is_highlighted_by_extension_and_images_have_no_toggle() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "sketch.rs", b"fn main() {}").await;
    write(&app, "DF-1", "login.png", &png()).await;

    let rust = text(
        app.get_htmx("/tasks/DF-1/artifacts/sketch.rs", &cookie)
            .await,
    )
    .await;
    assert!(rust.contains("hl-") && !rust.contains(">Source<"), "{rust}");

    let image = text(
        app.get_htmx("/tasks/DF-1/artifacts/login.png", &cookie)
            .await,
    )
    .await;
    assert!(
        image.contains("<img src=\"/tasks/DF-1/artifacts/login.png/raw\""),
        "{image}"
    );
    assert!(!image.contains(">Source<"), "{image}");
    assert!(image.contains("3 \u{d7} 2 px"), "{image}");
}

#[tokio::test]
async fn the_split_fragment_has_full_page_and_close_actions_the_page_has_a_way_back() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "findings.md", b"# F").await;

    let fragment = text(
        app.get_htmx("/tasks/DF-1/artifacts/findings.md", &cookie)
            .await,
    )
    .await;
    assert!(!fragment.contains("<html"), "{fragment}");
    assert!(
        fragment
            .contains("href=\"/tasks/DF-1/artifacts/findings.md\" aria-label=\"Open full page\""),
        "{fragment}"
    );
    assert!(
        fragment.contains("aria-label=\"Close artifact\""),
        "{fragment}"
    );

    let page = text(app.get("/tasks/DF-1/artifacts/findings.md", &cookie).await).await;
    assert!(
        page.contains("<html") && page.contains("href=\"/tasks/DF-1\""),
        "{page}"
    );
    assert!(
        page.contains("Research") && page.contains("findings.md"),
        "{page}"
    );
    assert!(!page.contains("aria-label=\"Close artifact\""), "{page}");
}

#[tokio::test]
async fn unknown_names_and_tasks_give_not_found() {
    let (app, cookie) = app_with_task().await;
    for uri in [
        "/tasks/DF-1/artifacts/missing.md",
        "/tasks/DF-99/artifacts/missing.md",
        "/tasks/DF-99/artifacts",
        "/tasks/DF-1/artifacts/missing.png/raw",
    ] {
        assert_eq!(
            app.get(uri, &cookie).await.status(),
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn the_image_endpoint_serves_bytes_with_safe_headers_and_revalidates_by_etag() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "login.png", &png()).await;

    let response = app
        .get("/tasks/DF-1/artifacts/login.png/raw", &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "content-type"), "image/png");
    assert_eq!(header_str(&response, "x-content-type-options"), "nosniff");
    assert_eq!(header_str(&response, "content-disposition"), "inline");
    assert_eq!(header_str(&response, "cache-control"), "no-cache");
    let etag = header_str(&response, "etag").to_string();
    assert_eq!(etag, format!("\"{}\"", df_db::sha256_hex(&png())));

    let revalidate = |tag: String| {
        let request = axum::http::Request::get("/tasks/DF-1/artifacts/login.png/raw")
            .header("cookie", &cookie)
            .header("if-none-match", tag)
            .body(axum::body::Body::empty())
            .unwrap();
        app.send(request)
    };
    assert_eq!(
        revalidate(etag.clone()).await.status(),
        StatusCode::NOT_MODIFIED
    );

    // Replaced: the old ETag gets the new bytes.
    let mut replaced = png();
    replaced.extend_from_slice(b"more");
    write(&app, "DF-1", "login.png", &replaced).await;
    let fresh = revalidate(etag.clone()).await;
    assert_eq!(fresh.status(), StatusCode::OK);
    assert_ne!(header_str(&fresh, "etag"), etag);
}

#[tokio::test]
async fn the_image_endpoint_needs_a_session_and_refuses_text() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "login.png", &png()).await;
    write(&app, "DF-1", "findings.md", b"x").await;

    let anonymous = app.get("/tasks/DF-1/artifacts/login.png/raw", "").await;
    assert_ne!(anonymous.status(), StatusCode::OK);
    assert!(!anonymous.headers().contains_key("etag"));
    assert_eq!(
        app.get("/tasks/DF-1/artifacts/findings.md/raw", &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

async fn tool_row(app: &TestApp, status: &str, name: &str) -> String {
    let seed_task: uuid::Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE key = 'DF-1'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let task = df_core::TaskId::from_uuid(seed_task);
    let run = insert_run(&app.pool, task, 1, "running").await;
    add_event(
        &app.pool,
        task,
        run,
        1,
        "t1",
        json!({"toolCall": {"toolCallId": "c1", "title": "mcp__factory__write_artifact",
            "kind": "TOOL_KIND_OTHER", "status": status,
            "rawInputJson": json!({"name": name, "content": "x"}).to_string()}}),
    )
    .await;
    let cookie = app.login().await;
    text(app.get("/tasks/DF-1", &cookie).await).await
}

#[tokio::test]
async fn a_written_artifact_is_a_transcript_row_that_opens_it() {
    let (app, _cookie) = app_with_task().await;

    let html = tool_row(&app, "TOOL_CALL_STATUS_COMPLETED", "findings.md").await;

    assert!(
        html.contains("data-artifact-written=\"findings.md\""),
        "{html}"
    );
    assert!(
        html.contains("x-on:click=\"open('findings.md')\""),
        "{html}"
    );
    assert!(html.contains("\u{b7} Markdown"), "{html}");
}

#[tokio::test]
async fn a_failed_write_stays_an_ordinary_tool_row() {
    let (app, _cookie) = app_with_task().await;

    let html = tool_row(&app, "TOOL_CALL_STATUS_FAILED", "findings.md").await;

    assert!(!html.contains("data-artifact-written"), "{html}");
    assert!(html.contains("mcp__factory__write_artifact"), "{html}");
}

#[test]
fn the_split_script_guards_storage_and_never_stores_the_temporary_collapse() {
    let js = crate::assets::script_source("artifactSplit.ts");
    assert!(
        js.contains("try {") && js.contains("localStorage.getItem(cfg.splitKey)"),
        "{js}"
    );
    assert!(
        js.contains("try { localStorage.setItem(cfg.splitKey"),
        "{js}"
    );
    // The details collapse is set on the workspace state directly, not through `setCollapsed`.
    assert!(
        !js.contains("setCollapsed") && !js.contains("df.details.collapsed"),
        "{js}"
    );
    assert!(
        js.contains("artifacts-changed") && js.contains("data-artifact-written"),
        "{js}"
    );
    assert!(js.contains("window.location.assign"), "{js}");
}

#[test]
fn the_workspace_script_remembers_the_task_list_in_guarded_storage() {
    let js = crate::assets::script_source("workspace.ts");
    assert!(
        js.contains(
            "try { this.listCollapsed = localStorage.getItem('df.list.collapsed') === '1' } catch"
        ),
        "{js}"
    );
    assert!(
        js.contains("try { localStorage.setItem('df.list.collapsed'"),
        "{js}"
    );
}

#[tokio::test]
async fn the_task_list_has_a_collapse_control_and_a_strip_to_expand_it() {
    let (app, cookie) = app_with_task().await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(
        html.contains("aria-label=\"Collapse task list\"")
            && html.contains("setListCollapsed(true)"),
        "{html}"
    );
    assert!(
        html.contains("aria-label=\"Expand task list\"")
            && html.contains("setListCollapsed(false)"),
        "{html}"
    );
    assert!(
        html.contains(":data-list-collapsed=\"listCollapsed\""),
        "{html}"
    );
}

#[test]
fn opening_an_artifact_hides_the_slide_over_and_closing_restores_both_details_states() {
    let js = crate::assets::script_source("artifactSplit.ts");
    // Below 1280 px the details are a slide-over (`details`), from 1280 px a column (`collapsed`).
    assert!(
        js.contains("this.restore = { collapsed: ws.collapsed, details: ws.details }"),
        "{js}"
    );
    assert!(js.contains("ws.details = false"), "{js}");
    assert!(js.contains("ws.details = this.restore.details"), "{js}");
    assert!(js.contains("ws.collapsed = this.restore.collapsed"), "{js}");
}

#[test]
fn the_pane_keeps_a_share_of_the_split_so_it_grows_with_the_center() {
    let js = crate::assets::script_source("artifactSplit.ts");
    assert!(
        js.contains("width:${(this.frac * 100).toFixed(2)}%"),
        "{js}"
    );
    assert!(!js.contains("this.w "), "{js}");
    let pane = super::view::split_pane().into_string();
    assert!(pane.contains(":style=\"paneStyle()\""), "{pane}");
    assert!(!pane.contains("px'"), "{pane}");
    assert!(super::view::split_args("DF-1").contains("defaultFraction"));
}

#[tokio::test]
async fn the_details_tabs_are_a_keyboard_operable_tablist_of_the_tabs_component() {
    let (app, cookie) = app_with_task().await;
    write(&app, "DF-1", "findings.md", b"x").await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(
        html.contains("x-data=\"tabs(&#39;overview&#39;)\"")
            || html.contains("x-data=\"tabs('overview')\""),
        "{html}"
    );
    let list = &html[html.find("role=\"tablist\"").unwrap()..];
    let list = &list[..list.find("</div>").unwrap()];
    assert!(list.contains("x-on:keydown=\"move($event)\""), "{list}");
    for tab in ["overview", "artifacts"] {
        assert!(list.contains(&format!("data-tab=\"{tab}\"")), "{list}");
    }
    assert!(
        list.contains("aria-selected=\"true\"") && list.contains("tabindex=\"-1\""),
        "{list}"
    );
    assert!(
        list.contains("aria-controls=\"panel-overview\"")
            && list.contains("aria-controls=\"panel-artifacts\""),
        "{list}"
    );
    // Each panel is a tabpanel that points back at its tab.
    for (panel, tab) in [
        ("panel-overview", "tab-overview"),
        ("panel-artifacts", "tab-artifacts"),
    ] {
        let at = html.find(&format!("id=\"{panel}\"")).unwrap();
        let tag = &html[html[..at].rfind("<div").unwrap()..];
        let tag = &tag[..tag.find('>').unwrap()];
        assert!(
            tag.contains("role=\"tabpanel\"")
                && tag.contains(&format!("aria-labelledby=\"{tab}\"")),
            "{tag}"
        );
    }
}

#[tokio::test]
async fn a_markdown_artifact_with_a_mermaid_fence_renders_the_diagram_markup() {
    let (app, cookie) = app_with_task().await;
    write(
        &app,
        "DF-1",
        "plan.md",
        b"# Plan\n\n```mermaid\ngraph TD; A-->B;\n```\n",
    )
    .await;

    let html = text(app.get_htmx("/tasks/DF-1/artifacts/plan.md", &cookie).await).await;

    assert!(html.contains("x-data=\"mermaidDiagram\""), "{html}");
}

#[test]
fn image_rows_get_the_image_icon_and_text_rows_the_file_icon() {
    let image = super::artifact_row("ev-1", "login.png").into_string();
    let doc = super::artifact_row("ev-2", "findings.md").into_string();
    // The image icon has a circle; the file icon does not.
    assert!(
        image.contains("<circle") && !doc.contains("<circle"),
        "{image} {doc}"
    );
}

#[tokio::test]
async fn panels_are_tabpanels_only_while_the_tab_bar_is_shown() {
    let (app, cookie) = app_with_task().await;

    let none = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!none.contains("role=\"tabpanel\""), "{none}");
    assert!(!none.contains("aria-labelledby=\"tab-"), "{none}");

    write(&app, "DF-1", "findings.md", b"x").await;
    let some = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert_eq!(some.matches("role=\"tabpanel\"").count(), 2, "{some}");
    // A tab bar that arrives later makes the panels tabpanels itself.
    assert!(some.contains("x-init=\"adopt()\""), "{some}");
    let script = crate::assets::script_source("tabs.ts");
    assert!(
        script.contains("setAttribute('role', 'tabpanel')"),
        "{script}"
    );
}
