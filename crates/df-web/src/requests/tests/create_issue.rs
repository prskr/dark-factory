//! The card of a `create-issue` request (spec `agent-requests`, "create-issue card shows the exact
//! text" and "create-issue body has a Markdown tab").

use df_core::RequestId;
use df_db::create_issue::{IssueOutcome, record_issue_outcome};
use df_db::requests::{Raised, raise_approval};
use serde_json::json;

use super::{Fixture, fixture};
use crate::testkit::{TestApp, form};

const BODY: &str =
    "See [docs](https://evil.example/x) and <b>raw</b> html.\n\n```rust\nlet x = 1;\n```";

impl Fixture {
    async fn create_issue(&self, title: &str, body: &str, labels: &[&str]) -> RequestId {
        let agent = self.agent();
        let raised = raise_approval(
            &self.app.pool,
            &TestApp::approval_types(),
            self.from(&agent),
            "create-issue",
            json!({"title": title, "body": body, "labels": labels}),
            "",
        )
        .await
        .unwrap();
        let Raised::Request { request, .. } = raised else {
            panic!("not stored")
        };
        request.id
    }
}

/// The part of `html` that starts at the first `needle` and ends before the first `end` after it.
fn between<'a>(html: &'a str, needle: &str, end: &str) -> &'a str {
    let start = html
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` in {html}"));
    let len = html[start..]
        .find(end)
        .unwrap_or_else(|| panic!("`{end}` in {html}"));
    &html[start..start + len]
}

/// The Preview and the Markdown panel of the page's first card.
fn panels(page: &str) -> (&str, &str) {
    let mut parts = page.split("role=\"tabpanel\"").skip(1);
    let preview = parts.next().expect("the Preview panel");
    let raw = parts.next().expect("the Markdown panel");
    (preview, raw)
}

#[tokio::test]
async fn the_card_shows_title_labels_and_the_body_in_both_tabs() {
    let f = fixture().await;
    f.create_issue("Flaky <login> test", BODY, &["bug", "ui"])
        .await;

    let page = f.page("/requests").await;

    assert!(page.contains("Flaky &lt;login&gt; test"), "{page}");
    assert!(!page.contains("<login>"), "{page}");
    for label in ["bug", "ui"] {
        assert!(
            page.contains(&format!(">{label}</span>")),
            "{label}: {page}"
        );
    }
    let (preview, raw) = panels(&page);
    assert!(
        preview.contains("<pre") && preview.contains("hl-rust\">let</span>"),
        "{preview}"
    );
    assert!(raw.contains("let x = 1;"), "{raw}");
}

#[tokio::test]
async fn raw_html_of_the_body_is_shown_as_text_in_both_tabs() {
    let f = fixture().await;
    f.create_issue("T", BODY, &[]).await;

    let page = f.page("/requests").await;

    assert!(!page.contains("<b>raw</b>"), "{page}");
    assert_eq!(page.matches("&lt;b&gt;raw&lt;/b&gt;").count(), 2, "{page}");
}

#[tokio::test]
async fn a_link_target_is_visible_only_in_the_markdown_tab() {
    let f = fixture().await;
    f.create_issue("T", BODY, &[]).await;

    let page = f.page("/requests").await;

    let (preview, raw) = panels(&page);
    assert!(preview.contains(">docs</a>"), "{preview}");
    assert!(!preview.contains("[docs]("), "{preview}");
    assert!(raw.contains("[docs](https://evil.example/x)"), "{raw}");
}

#[tokio::test]
async fn the_preview_tab_is_selected_first_and_the_tabs_are_accessible() {
    let f = fixture().await;
    f.create_issue("T", "b", &[]).await;

    let page = f.page("/requests").await;

    assert!(page.contains("role=\"tablist\""), "{page}");
    let preview_tab = between(&page, "data-tab=\"preview\"", ">");
    let markdown_tab = between(&page, "data-tab=\"markdown\"", ">");
    assert!(
        preview_tab.contains("aria-selected=\"true\""),
        "{preview_tab}"
    );
    assert!(
        markdown_tab.contains("aria-selected=\"false\""),
        "{markdown_tab}"
    );
    assert!(preview_tab.contains("aria-controls="), "{preview_tab}");
    assert_eq!(page.matches("role=\"tab\"").count(), 2, "{page}");
    assert_eq!(page.matches("role=\"tabpanel\"").count(), 2, "{page}");
    assert!(page.contains("tabs('preview', false, false)"), "{page}");
}

#[tokio::test]
async fn the_actions_sit_below_the_tabs_without_a_scope_picker() {
    let f = fixture().await;
    f.create_issue("T", "b", &[]).await;

    let page = f.page("/requests").await;

    let panels_end = page.rfind("role=\"tabpanel\"").unwrap();
    let approve = page.find("/approve\"").expect("approve action");
    assert!(panels_end < approve, "{page}");
    assert!(!page.contains("type=\"radio\""), "{page}");
    assert!(
        page.contains("placeholder=\"Note to the agent on denial (optional)\""),
        "{page}"
    );
    assert!(page.contains("Approve for this task"), "{page}");
}

#[tokio::test]
async fn an_approved_card_names_the_created_issue_and_keeps_the_body_tabs() {
    let f = fixture().await;
    let id = f.create_issue("Flaky", "Details", &["bug"]).await;
    let (_, card) = f
        .post(
            &format!("/requests/{id}/approve"),
            &form(&[("scope", "task"), ("comment", "")]),
        )
        .await;
    assert!(card.contains("Approved by"), "{card}");
    assert!(!card.contains("created issue"), "{card}");
    record_issue_outcome(
        &f.app.pool,
        id,
        &IssueOutcome::Created {
            number: 42,
            url: "https://forge.example.test/o/r/issues/42".into(),
        },
    )
    .await
    .unwrap();

    let page = f.page("/tasks/DF-1").await;

    let card = between(&page, &format!("id=\"request-{id}\""), "</article>");
    assert!(card.contains("data-state=\"approved\""), "{card}");
    assert!(
        card.contains("created issue <a href=\"https://forge.example.test/o/r/issues/42\""),
        "{card}"
    );
    assert!(card.contains(">#42</a>"), "{card}");
    assert!(
        card.contains("role=\"tablist\"") && card.contains("Details"),
        "{card}"
    );
    assert!(
        !card.contains("/approve\""),
        "no actions after the decision: {card}"
    );
}

#[tokio::test]
async fn an_approved_card_shows_the_error_when_the_issue_could_not_be_created() {
    let f = fixture().await;
    let id = f.create_issue("Flaky", "Details", &["nope"]).await;
    f.post(
        &format!("/requests/{id}/approve"),
        &form(&[("scope", "task"), ("comment", "")]),
    )
    .await;
    record_issue_outcome(
        &f.app.pool,
        id,
        &IssueOutcome::Failed {
            error: "the repository has no label `<nope>`".into(),
        },
    )
    .await
    .unwrap();

    let page = f.page("/tasks/DF-1").await;

    let card = between(&page, &format!("id=\"request-{id}\""), "</article>");
    assert!(
        card.contains("the issue could not be created: the repository has no label `&lt;nope&gt;`"),
        "{card}"
    );
    assert!(!card.contains("created issue"), "{card}");
}

#[tokio::test]
async fn a_denied_card_shows_the_status_and_the_comment() {
    let f = fixture().await;
    let id = f.create_issue("Flaky", "Details", &[]).await;

    let (_, card) = f
        .post(
            &format!("/requests/{id}/deny"),
            &form(&[("comment", "not now")]),
        )
        .await;

    assert!(card.contains("data-state=\"denied\""), "{card}");
    assert!(
        card.contains("Denied by") && card.contains("not now"),
        "{card}"
    );
    assert!(card.contains("role=\"tablist\""), "{card}");
}
