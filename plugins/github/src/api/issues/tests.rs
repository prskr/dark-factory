//! Scenarios of the `github-integration` spec "List issues", "List issues paging", "Create issue"
//! and "Pull request flag on issues".

use serde_json::{Value, json};

use super::super::tests::{fake, github, reply};
use super::*;

const REPO_ISSUES: &str = "https://api.github.com/repos/o/r/issues";

fn repo() -> RepoRef {
    RepoRef {
        owner: "o".into(),
        name: "r".into(),
    }
}

fn issue_json(number: u64, labels: &[&str]) -> Value {
    json!({
        "number": number, "html_url": format!("https://github.com/o/r/issues/{number}"),
        "title": format!("issue {number}"), "body": null, "state": "open",
        "labels": labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>(),
    })
}

fn pull_json(number: u64) -> Value {
    let mut item = issue_json(number, &[]);
    item["pull_request"] = json!({"url": "https://api.github.com/repos/o/r/pulls/1"});
    item
}

fn filter() -> IssueFilter {
    IssueFilter {
        state: StateFilter::Open,
        labels: Vec::new(),
        query: None,
    }
}

fn numbers(page: &IssuePage) -> Vec<u64> {
    page.issues.iter().map(|i| i.id.number).collect()
}

fn search_reply(items: &[Value], total: u64, incomplete: bool) -> crate::api::Response {
    reply(
        200,
        json!({"total_count": total, "incomplete_results": incomplete, "items": items}),
    )
}

fn pulls(count: u64) -> Vec<Value> {
    (0..count).map(pull_json).collect()
}

#[test]
fn list_asks_for_the_state_and_all_labels_newest_first() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, json!([issue_json(5, &["bug", "ui"])]))]);
    let filter = IssueFilter {
        state: StateFilter::Closed,
        labels: vec!["bug".into(), "ui".into()],
        query: None,
    };

    let page = github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(
        fake.requests.borrow()[0].url,
        format!(
            "{REPO_ISSUES}?state=closed&labels=bug%2Cui&sort=created&direction=desc&per_page=100&page=1"
        )
    );
    assert_eq!(numbers(&page), vec![5]);
    assert!(!page.truncated && page.next_cursor.is_none());
    Ok(())
}

#[test]
fn list_drops_pull_requests_and_fills_the_page_from_the_next_forge_page() -> anyhow::Result<()> {
    let mut first = pulls(98);
    first.extend([issue_json(7, &[]), issue_json(6, &[])]);
    let fake = fake(vec![
        reply(200, json!(first)),
        reply(200, json!([pull_json(3), issue_json(2, &[])])),
    ]);

    let page = github(&fake, &[]).list_issues(&repo(), &filter(), 30, None)?;

    assert_eq!(numbers(&page), vec![7, 6, 2]);
    assert!(page.issues.iter().all(|i| !i.pull_request));
    assert!(!page.truncated && page.next_cursor.is_none());
    assert!(fake.requests.borrow()[1].url.ends_with("page=2"));
    Ok(())
}

#[test]
fn list_with_limit_zero_is_invalid_input() {
    let fake = fake(Vec::new());

    let err = github(&fake, &[])
        .list_issues(&repo(), &filter(), 0, None)
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(fake.requests.borrow().is_empty());
}

#[test]
fn list_with_a_malformed_cursor_is_invalid_input() {
    let fake = fake(Vec::new());

    let err = github(&fake, &[])
        .list_issues(&repo(), &filter(), 5, Some("x"))
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn list_cursor_resumes_inside_a_forge_page() -> anyhow::Result<()> {
    // 100, 99 (pull request), 98 .. 92, 91
    let mut items = vec![issue_json(100, &[]), pull_json(99)];
    items.extend((91..=98).rev().map(|n| issue_json(n, &[])));
    let fake = fake(vec![reply(200, json!(items)), reply(200, json!(items))]);

    let first = github(&fake, &[]).list_issues(&repo(), &filter(), 3, None)?;
    let second =
        github(&fake, &[]).list_issues(&repo(), &filter(), 3, first.next_cursor.as_deref())?;

    assert_eq!(numbers(&first), vec![100, 98, 97]);
    assert!(first.truncated);
    assert_eq!(numbers(&second), vec![96, 95, 94]);
    assert!(fake.requests.borrow()[1].url.ends_with("page=1"));
    Ok(())
}

#[test]
fn list_stops_after_five_forge_pages_with_a_cursor_after_the_last_read_item() -> anyhow::Result<()>
{
    let fake = fake(vec![reply(200, json!(pulls(100))); 5]);

    let page = github(&fake, &[]).list_issues(&repo(), &filter(), 5, None)?;

    assert!(page.issues.is_empty());
    assert!(page.truncated);
    assert_eq!(fake.requests.borrow().len(), 5);
    let cursor = page.next_cursor.expect("a cursor to the sixth page");
    let next = self::fake(vec![reply(200, json!([issue_json(1, &[])]))]);
    let more = github(&next, &[]).list_issues(&repo(), &filter(), 5, Some(&cursor))?;
    assert_eq!(numbers(&more), vec![1]);
    assert!(next.requests.borrow()[0].url.ends_with("page=6"));
    Ok(())
}

#[test]
fn list_with_a_query_searches_issues_of_the_repository_for_the_phrase() -> anyhow::Result<()> {
    let fake = fake(vec![search_reply(&[issue_json(4, &["bug"])], 1, false)]);
    let filter = IssueFilter {
        state: StateFilter::Open,
        labels: vec!["bug".into()],
        query: Some("timeout".into()),
    };

    let page = github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    let q = "is%3Aissue%20repo%3Ao%2Fr%20in%3Atitle%2Cbody%20is%3Aopen%20label%3A%22bug%22%20%22timeout%22";
    assert_eq!(
        fake.requests.borrow()[0].url,
        format!(
            "https://api.github.com/search/issues?q={q}&sort=created&order=desc&per_page=100&page=1"
        )
    );
    assert_eq!(numbers(&page), vec![4]);
    assert!(!page.truncated && page.next_cursor.is_none());
    Ok(())
}

#[test]
fn list_searches_double_quotes_in_the_query_as_spaces() -> anyhow::Result<()> {
    let fake = fake(vec![search_reply(&[], 0, false)]);
    let filter = IssueFilter {
        query: Some("foo \"bar\"".into()),
        ..filter()
    };

    github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    let url = fake.requests.borrow()[0].url.clone();
    assert!(url.contains("%22foo%20bar%22"), "{url}");
    Ok(())
}

#[test]
fn list_searches_qualifiers_in_the_query_as_literal_text() -> anyhow::Result<()> {
    let fake = fake(vec![search_reply(&[issue_json(4, &[])], 1, false)]);
    let filter = IssueFilter {
        query: Some("repo:other/private is:pr".into()),
        ..filter()
    };

    github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    let url = fake.requests.borrow()[0].url.clone();
    assert!(
        url.contains("%22repo%3Aother%2Fprivate%20is%3Apr%22"),
        "the qualifiers stay inside the quoted phrase: {url}"
    );
    assert!(url.contains("repo%3Ao%2Fr"), "{url}");
    Ok(())
}

#[test]
fn list_with_a_query_and_a_full_page_offers_the_next_page() -> anyhow::Result<()> {
    let items: Vec<Value> = (1..=100).map(|n| issue_json(n, &[])).collect();
    let fake = fake(vec![search_reply(&items, 250, false)]);
    let filter = IssueFilter {
        query: Some("x".into()),
        ..filter()
    };

    let page = github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(page.issues.len(), 30);
    assert!(page.truncated);
    assert_eq!(page.next_cursor.as_deref(), Some("1:30"));
    Ok(())
}

#[test]
fn list_with_incomplete_search_results_is_truncated_with_a_cursor() -> anyhow::Result<()> {
    let fake = fake(vec![search_reply(&[issue_json(4, &[])], 1, true)]);
    let filter = IssueFilter {
        query: Some("x".into()),
        ..filter()
    };

    let page = github(&fake, &[]).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(numbers(&page), vec![4]);
    assert!(page.truncated);
    assert_eq!(page.next_cursor.as_deref(), Some("2:0"));
    Ok(())
}

#[test]
fn list_stops_at_the_search_cap_of_1000_results_without_a_cursor() -> anyhow::Result<()> {
    let items: Vec<Value> = (1..=100).map(|n| issue_json(n, &[])).collect();
    let fake = fake(vec![search_reply(&items, 1500, false)]);
    let filter = IssueFilter {
        query: Some("x".into()),
        ..filter()
    };

    let page = github(&fake, &[]).list_issues(&repo(), &filter, 100, Some("10:0"))?;

    assert_eq!(page.issues.len(), 100);
    assert!(page.truncated);
    assert!(page.next_cursor.is_none());
    Ok(())
}

#[test]
fn create_sends_title_body_and_labels() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, json!([{"name": "bug"}, {"name": "ui"}, {"name": "x"}])),
        reply(201, issue_json(8, &["bug", "ui"])),
    ]);
    let issue = NewIssue {
        title: "Flaky".into(),
        body: "Details".into(),
        labels: vec!["bug".into(), "ui".into()],
    };

    let created = github(&fake, &[]).create_issue(&repo(), &issue)?;

    let requests = fake.requests.borrow();
    assert_eq!(
        requests[0].url,
        "https://api.github.com/repos/o/r/labels?per_page=100&page=1"
    );
    assert_eq!(requests[1].url, REPO_ISSUES);
    let sent: Value = serde_json::from_slice(requests[1].body.as_ref().expect("body"))?;
    assert_eq!(
        sent,
        json!({"title": "Flaky", "body": "Details", "labels": ["bug", "ui"]})
    );
    assert_eq!(created.id.number, 8);
    assert_eq!(created.url, "https://github.com/o/r/issues/8");
    assert!(!created.pull_request);
    Ok(())
}

#[test]
fn create_accepts_a_label_beyond_the_fifth_label_page() -> anyhow::Result<()> {
    let filler = |page: usize| -> Vec<Value> {
        (0..100)
            .map(|n| json!({"name": format!("filler-{page}-{n}")}))
            .collect()
    };
    let mut responses: Vec<_> = (1..=5).map(|p| reply(200, json!(filler(p)))).collect();
    responses.push(reply(200, json!([{"name": "Late"}])));
    responses.push(reply(201, issue_json(8, &["Late"])));
    let fake = fake(responses);
    let issue = NewIssue {
        title: "T".into(),
        body: String::new(),
        labels: vec!["late".into()],
    };

    github(&fake, &[]).create_issue(&repo(), &issue)?;

    let requests = fake.requests.borrow();
    assert!(requests[5].url.ends_with("labels?per_page=100&page=6"));
    let sent: Value = serde_json::from_slice(requests[6].body.as_ref().expect("body"))?;
    assert_eq!(sent["labels"], json!(["Late"]));
    Ok(())
}

#[test]
fn create_stops_reading_label_pages_once_every_name_is_found() -> anyhow::Result<()> {
    let full: Vec<Value> = (0..100)
        .map(|n| json!({"name": if n == 0 { "bug".to_string() } else { format!("f{n}") }}))
        .collect();
    let fake = fake(vec![
        reply(200, json!(full)),
        reply(201, issue_json(8, &["bug"])),
    ]);
    let issue = NewIssue {
        title: "T".into(),
        body: String::new(),
        labels: vec!["bug".into()],
    };

    github(&fake, &[]).create_issue(&repo(), &issue)?;

    assert_eq!(fake.requests.borrow().len(), 2);
    Ok(())
}

#[test]
fn create_with_an_unknown_label_is_invalid_input_and_posts_nothing() {
    let fake = fake(vec![reply(200, json!([{"name": "bug"}]))]);
    let issue = NewIssue {
        title: "Flaky".into(),
        body: String::new(),
        labels: vec!["bug".into(), "nope".into()],
    };

    let err = github(&fake, &[])
        .create_issue(&repo(), &issue)
        .unwrap_err();

    assert!(
        matches!(&err, Error::InvalidInput(m) if m.contains("nope")),
        "{err:?}"
    );
    assert_eq!(fake.requests.borrow().len(), 1, "only the label lookup");
}

#[test]
fn create_without_labels_does_not_look_labels_up() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, issue_json(9, &[]))]);
    let issue = NewIssue {
        title: "T".into(),
        body: "B".into(),
        labels: Vec::new(),
    };

    github(&fake, &[]).create_issue(&repo(), &issue)?;

    assert_eq!(fake.requests.borrow().len(), 1);
    Ok(())
}

#[test]
fn get_flags_pull_request_numbers() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pull_json(4)),
        reply(200, issue_json(5, &[])),
    ]);
    let id = |number| IssueRef {
        repo: repo(),
        number,
    };

    let pull = github(&fake, &[]).get_issue(&id(4))?;
    let issue = github(&fake, &[]).get_issue(&id(5))?;

    assert!(pull.pull_request);
    assert!(!issue.pull_request);
    Ok(())
}
