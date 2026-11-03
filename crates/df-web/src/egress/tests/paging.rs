//! Search and pages of the rules table.

use axum::http::StatusCode;

use super::{add_host, add_task_host, app_with_egress};
use crate::testkit::{TestApp, has_disabled_attribute, new_task, seed, text};

/// 33 instance rules: `r01.example` to `r31.example`, `github.com` and
/// `objects.githubusercontent.com`.
async fn rules(app: &TestApp, cookie: &str) {
    for n in 1..=31 {
        add_host(app, cookie, "", &format!("r{n:02}.example"), "443").await;
    }
    add_host(app, cookie, "", "github.com", "443").await;
    add_host(app, cookie, "", "objects.githubusercontent.com", "443").await;
}

/// The `hx-delete` URL of the first row, as the browser sends it.
fn first_delete_url(page: &str) -> String {
    page.split("hx-delete=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .replace("&amp;", "&")
}

fn count_rows(html: &str) -> usize {
    html.matches("hx-delete=").count()
}

#[tokio::test]
async fn the_first_page_lists_25_rules_and_the_paging_controls() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    assert_eq!(count_rows(&page), 25);
    assert!(page.contains("1\u{2013}25 of 33 rules"), "{page}");
    let results = &page[page.find("id=\"rules-results\"").unwrap()..];
    assert!(
        results.contains("Page 1 of 2")
            && results.contains(">Previous<")
            && results.contains(">Next<"),
        "the paging controls are in the swapped results: {page}"
    );
    let previous = crate::testkit::tag_around(&page, ">Previous<");
    assert!(has_disabled_attribute(previous), "{previous}");
    assert!(
        page.contains("Egress rules <span class=\"font-normal text-faint\">33</span>"),
        "{page}"
    );
}

#[tokio::test]
async fn the_second_page_lists_the_rest_and_next_is_disabled() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?page=2", &cookie).await).await;

    assert_eq!(count_rows(&page), 8);
    assert!(
        page.contains("26\u{2013}33 of 33 rules") && page.contains("Page 2 of 2"),
        "{page}"
    );
    assert!(has_disabled_attribute(crate::testkit::tag_around(
        &page, ">Next<"
    )));
    assert!(!has_disabled_attribute(crate::testkit::tag_around(
        &page,
        ">Previous<"
    )));
}

#[tokio::test]
async fn a_page_beyond_the_end_shows_the_last_page() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?page=9", &cookie).await).await;

    assert!(
        page.contains("Page 2 of 2") && page.contains("26\u{2013}33 of 33 rules"),
        "{page}"
    );
}

#[tokio::test]
async fn a_single_page_has_no_paging_controls() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for n in 1..=8 {
        add_host(&app, &cookie, "", &format!("r{n}.example"), "443").await;
    }

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    assert_eq!(count_rows(&page), 8);
    assert!(page.contains("1\u{2013}8 of 8 rules"), "{page}");
    assert!(
        !page.contains(">Previous<") && !page.contains("Page 1 of"),
        "{page}"
    );
}

#[tokio::test]
async fn searching_keeps_matching_hosts_and_counts_them() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?q=GitHub", &cookie).await).await;

    assert_eq!(count_rows(&page), 2);
    assert!(page.contains("2 of 33 rules match"), "{page}");
    assert!(
        page.contains("Egress rules <span class=\"font-normal text-faint\">33</span>"),
        "the title ignores the search: {page}"
    );
    assert!(page.contains("value=\"GitHub\""), "{page}");
    assert!(!page.contains(">Previous<"), "{page}");
}

#[tokio::test]
async fn searching_matches_the_scope_label() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    add_host(&app, &cookie, "", "github.com", "443").await;

    let page = text(app.get("/egress/allowlist?q=project+df", &cookie).await).await;

    assert!(
        page.contains(">pypi.org<") && !page.contains(">github.com<"),
        "{page}"
    );
}

#[tokio::test]
async fn a_search_without_a_match_replaces_the_table() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?q=nothing-here", &cookie).await).await;

    let results = &page[page.find("id=\"rules-results\"").unwrap()..];
    assert!(
        results.contains("No rules match &quot;nothing-here&quot;"),
        "{page}"
    );
    assert!(!page.contains("<table"), "{page}");
    assert!(
        page.contains("id=\"rules-search\""),
        "the search stays: {page}"
    );
}

#[tokio::test]
async fn the_search_box_reloads_the_region_and_pages_keep_the_search() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?q=r&page=2", &cookie).await).await;

    let input = crate::testkit::tag_around(&page, "id=\"rules-search\"");
    assert!(
        input.contains("hx-get=\"/egress/allowlist\"")
            && input.contains("input changed delay:300ms")
            && input.contains("hx-push-url=\"true\""),
        "{input}"
    );
    let previous = crate::testkit::tag_around(&page, ">Previous<");
    assert!(
        previous.contains("hx-get=\"/egress/allowlist?q=r\""),
        "{previous}"
    );
}

#[tokio::test]
async fn highlight_without_a_page_opens_the_page_of_the_rule() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(
        app.get("/egress/allowlist?highlight=r30.example:443", &cookie)
            .await,
    )
    .await;

    assert!(page.contains("Page 2 of 2"), "{page}");
    assert_eq!(page.matches("aria-current=\"true\"").count(), 1, "{page}");
    let explicit = text(
        app.get(
            "/egress/allowlist?highlight=r30.example:443&page=1",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(explicit.contains("Page 1 of 2") && !explicit.contains("aria-current=\"true\""));
}

#[tokio::test]
async fn deleting_keeps_the_search_and_the_page() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;
    let page = text(app.get("/egress/allowlist?q=r&page=2", &cookie).await).await;
    let delete = first_delete_url(&page);
    assert!(delete.ends_with("?q=r&page=2"), "{delete}");

    let response = app.delete(&delete, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("Page 2 of 2") && html.contains("value=\"r\""),
        "{html}"
    );
    assert!(html.contains("Egress rules <span class=\"font-normal text-faint\">32</span>"));
}

#[tokio::test]
async fn deleting_the_last_row_of_a_later_page_shows_the_last_remaining_page() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for n in 1..=26 {
        add_host(&app, &cookie, "", &format!("r{n:02}.example"), "443").await;
    }
    let page = text(app.get("/egress/allowlist?page=2", &cookie).await).await;
    let delete = first_delete_url(&page);

    let html = text(app.delete(&delete, &cookie).await).await;

    assert_eq!(count_rows(&html), 25, "{html}");
    assert!(html.contains("1\u{2013}25 of 25 rules"), "{html}");
}

#[tokio::test]
async fn searching_by_scope_lists_only_the_rules_of_that_task() {
    let app = app_with_egress(vec![]).await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix CI").await;
    let cookie = app.login().await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "DF", "crates.io", "443").await;
    add_task_host(&app, task.id.as_uuid(), "pypi.org").await;

    let page = text(app.get("/egress/allowlist?q=df-1", &cookie).await).await;

    assert_eq!(count_rows(&page), 1, "{page}");
    assert!(
        page.contains(">pypi.org<") && page.contains(&format!(">Task {}<", task.key)),
        "{page}"
    );
    assert!(page.contains("1 of 3 rules match"), "{page}");
}

#[tokio::test]
async fn the_search_swaps_only_the_results_so_the_field_keeps_its_text_and_focus() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist?q=github", &cookie).await).await;

    let input = crate::testkit::tag_around(&page, "id=\"rules-search\"");
    assert!(
        input.contains("hx-target=\"#rules-results\"")
            && input.contains("hx-select=\"#rules-results\""),
        "{input}"
    );
    let results = &page[page.find("id=\"rules-results\"").unwrap()..];
    assert!(
        !results.contains("id=\"rules-search\""),
        "the field is outside the swap"
    );
    assert!(results.contains("<table") && results.contains("2 of 33 rules match"));
    let paged = text(app.get("/egress/allowlist", &cookie).await).await;
    for button in [">Previous<", ">Next<"] {
        let tag = crate::testkit::tag_around(&paged, button);
        assert!(tag.contains("hx-target=\"#rules-results\""), "{tag}");
    }
}

#[tokio::test]
async fn the_table_has_fixed_column_widths_so_columns_stay_put_between_pages() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    rules(&app, &cookie).await;

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    assert!(
        page.contains("[&amp;_table]:table-fixed") && page.contains("[&amp;_th:nth-child(2)]:w-28")
    );
}
