//! Scenarios of the `forgejo-integration` spec "List issues", "List issues paging", "Create issue"
//! and "Pull request flag on issues".

use std::cell::RefCell;
use std::collections::VecDeque;

use serde_json::{Value, json};

use super::super::{Request, Response};
use super::*;
use forge_common::draft;
use forge_common::log::StderrLog;

/// Replays canned responses and records the requests.
struct Fake {
    responses: RefCell<VecDeque<Response>>,
    requests: RefCell<Vec<Request>>,
}

impl Transport for &Fake {
    fn send(&self, request: Request) -> Result<Response, Error> {
        self.requests.borrow_mut().push(request);
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .expect("a canned response for every request"))
    }
}

fn fake(responses: Vec<Response>) -> Fake {
    Fake {
        responses: RefCell::new(responses.into()),
        requests: RefCell::default(),
    }
}

fn reply(status: u16, body: &Value) -> Response {
    Response {
        status,
        retry_after: None,
        body: body.to_string().into_bytes(),
    }
}

fn repo() -> RepoRef {
    RepoRef {
        owner: "o".into(),
        name: "r".into(),
    }
}

fn plugin(fake: &Fake) -> Forgejo<&Fake, StderrLog> {
    Forgejo::new(
        fake,
        StderrLog,
        draft::parse_prefixes(draft::DEFAULT_PREFIXES),
    )
}

/// The page after the last one.
fn empty() -> Response {
    reply(200, &json!([]))
}

fn issue_json(number: u64, labels: &[&str]) -> Value {
    json!({
        "number": number, "html_url": format!("https://git/o/r/issues/{number}"),
        "title": format!("issue {number}"), "body": "b", "state": "open",
        "labels": labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>(),
    })
}

fn pull_json(number: u64) -> Value {
    let mut item = issue_json(number, &[]);
    item["pull_request"] = json!({"merged": false});
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

#[test]
fn list_asks_for_issues_with_state_labels_and_query() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, &json!([issue_json(5, &["bug", "ui"])])),
        empty(),
    ]);
    let filter = IssueFilter {
        state: StateFilter::Closed,
        labels: vec!["bug".into(), "ui".into()],
        query: Some("a b&c".into()),
    };

    let page = plugin(&fake).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(
        fake.requests.borrow()[0].path,
        "/api/v1/repos/o/r/issues?type=issues&state=closed&labels=bug%2Cui&q=a%20b%26c&page=1&limit=50"
    );
    assert_eq!(numbers(&page), vec![5]);
    assert!(!page.truncated && page.next_cursor.is_none());
    Ok(())
}

#[test]
fn a_qualifier_in_the_query_is_plain_text_on_the_projects_repository_only() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, &json!([]))]);
    let filter = IssueFilter {
        query: Some("repo:other/private".into()),
        ..filter()
    };

    plugin(&fake).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(
        fake.requests.borrow()[0].path,
        "/api/v1/repos/o/r/issues?type=issues&state=open&q=repo%3Aother%2Fprivate&page=1&limit=50"
    );
    Ok(())
}

#[test]
fn list_drops_items_that_carry_a_pull_request_field() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(
            200,
            &json!([issue_json(3, &[]), pull_json(2), issue_json(1, &[])]),
        ),
        empty(),
    ]);

    let page = plugin(&fake).list_issues(&repo(), &filter(), 30, None)?;

    assert_eq!(numbers(&page), vec![3, 1]);
    assert!(page.issues.iter().all(|i| !i.pull_request));
    Ok(())
}

#[test]
fn list_requires_every_label() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(
            200,
            &json!([issue_json(2, &["bug"]), issue_json(1, &["bug", "ui"])]),
        ),
        empty(),
    ]);
    let filter = IssueFilter {
        labels: vec!["bug".into(), "ui".into()],
        ..filter()
    };

    let page = plugin(&fake).list_issues(&repo(), &filter, 30, None)?;

    assert_eq!(numbers(&page), vec![1]);
    Ok(())
}

#[test]
fn list_with_limit_zero_is_invalid_input() {
    let fake = fake(Vec::new());

    let err = plugin(&fake)
        .list_issues(&repo(), &filter(), 0, None)
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(fake.requests.borrow().is_empty());
}

#[test]
fn list_with_a_malformed_cursor_is_invalid_input() {
    let fake = fake(Vec::new());

    let err = plugin(&fake)
        .list_issues(&repo(), &filter(), 10, Some("nonsense"))
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn list_stops_at_the_limit_and_the_cursor_resumes_after_the_last_item() -> anyhow::Result<()> {
    // 10 raw items on the page: 100, 99 (a pull request), 98, ..., 92.
    let mut items = vec![issue_json(100, &[]), pull_json(99)];
    items.extend((92..=98).rev().map(|n| issue_json(n, &[])));
    items.push(issue_json(91, &[]));
    let fake = fake(vec![reply(200, &json!(items)), reply(200, &json!(items))]);

    let first = plugin(&fake).list_issues(&repo(), &filter(), 3, None)?;
    let second = plugin(&fake).list_issues(&repo(), &filter(), 3, first.next_cursor.as_deref())?;

    assert_eq!(numbers(&first), vec![100, 98, 97]);
    assert!(first.truncated);
    assert_eq!(numbers(&second), vec![96, 95, 94]);
    // The cursor re-reads the same forge page and resumes inside it.
    assert!(fake.requests.borrow()[1].path.contains("page=1"));
    Ok(())
}

#[test]
fn list_with_everything_fitting_is_not_truncated() -> anyhow::Result<()> {
    let items: Vec<Value> = (1..=20).map(|n| issue_json(n, &[])).collect();
    let fake = fake(vec![reply(200, &json!(items)), empty()]);

    let page = plugin(&fake).list_issues(&repo(), &filter(), 30, None)?;

    assert_eq!(page.issues.len(), 20);
    assert!(!page.truncated && page.next_cursor.is_none());
    Ok(())
}

#[test]
fn list_continues_on_the_next_forge_page_to_fill_the_limit() -> anyhow::Result<()> {
    let full: Vec<Value> = (0..PAGE_SIZE as u64).map(pull_json).collect();
    let fake = fake(vec![
        reply(200, &json!(full)),
        reply(200, &json!([issue_json(7, &[])])),
        empty(),
    ]);

    let page = plugin(&fake).list_issues(&repo(), &filter(), 5, None)?;

    assert_eq!(numbers(&page), vec![7]);
    assert!(!page.truncated && page.next_cursor.is_none());
    assert!(fake.requests.borrow()[1].path.contains("page=2"));
    Ok(())
}

#[test]
fn list_reads_on_past_pages_the_server_capped_below_the_requested_size() -> anyhow::Result<()> {
    // `MAX_RESPONSE_ITEMS` below the requested `limit`: pages of 2, none of them the last.
    let fake = fake(vec![
        reply(200, &json!([issue_json(4, &[]), issue_json(3, &[])])),
        reply(200, &json!([issue_json(2, &[]), issue_json(1, &[])])),
        empty(),
    ]);

    let page = plugin(&fake).list_issues(&repo(), &filter(), 30, None)?;

    assert_eq!(numbers(&page), vec![4, 3, 2, 1]);
    assert!(!page.truncated && page.next_cursor.is_none());
    assert_eq!(fake.requests.borrow().len(), 3);
    Ok(())
}

#[test]
fn create_finds_labels_on_later_capped_label_pages() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, &json!([{"id": 11, "name": "bug"}])),
        reply(200, &json!([{"id": 12, "name": "ui"}])),
        reply(201, &issue_json(8, &["bug", "ui"])),
    ]);
    let issue = NewIssue {
        title: "T".into(),
        body: String::new(),
        labels: vec!["ui".into(), "bug".into()],
    };

    plugin(&fake).create_issue(&repo(), &issue)?;

    let requests = fake.requests.borrow();
    assert_eq!(requests.len(), 3, "stops once every name is found");
    let sent: Value = serde_json::from_slice(requests[2].body.as_ref().expect("body"))?;
    assert_eq!(sent["labels"], json!([12, 11]));
    Ok(())
}

#[test]
fn create_accepts_a_label_beyond_the_fifth_label_page() -> anyhow::Result<()> {
    let mut responses: Vec<Response> = (1..=5)
        .map(|n| reply(200, &json!([{"id": n, "name": format!("filler-{n}")}])))
        .collect();
    responses.push(reply(200, &json!([{"id": 99, "name": "late"}])));
    responses.push(reply(201, &issue_json(8, &["late"])));
    let fake = fake(responses);
    let issue = NewIssue {
        title: "T".into(),
        body: String::new(),
        labels: vec!["late".into()],
    };

    plugin(&fake).create_issue(&repo(), &issue)?;

    let requests = fake.requests.borrow();
    assert!(
        requests[5].path.contains("labels?page=6"),
        "{}",
        requests[5].path
    );
    let sent: Value = serde_json::from_slice(requests[6].body.as_ref().expect("body"))?;
    assert_eq!(sent["labels"], json!([99]));
    Ok(())
}

#[test]
fn list_stops_after_five_forge_pages_with_a_cursor() -> anyhow::Result<()> {
    let full: Vec<Value> = (0..PAGE_SIZE as u64).map(pull_json).collect();
    let fake = fake(vec![reply(200, &json!(full)); 5]);

    let page = plugin(&fake).list_issues(&repo(), &filter(), 5, None)?;

    assert!(page.issues.is_empty());
    assert!(page.truncated);
    assert_eq!(fake.requests.borrow().len(), 5);
    let cursor = page.next_cursor.expect("a cursor to the sixth page");
    let next = self::fake(vec![reply(200, &json!([issue_json(1, &[])])), empty()]);
    let more = plugin(&next).list_issues(&repo(), &filter(), 5, Some(&cursor))?;
    assert_eq!(numbers(&more), vec![1]);
    Ok(())
}

#[test]
fn create_resolves_label_names_to_ids() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(
            200,
            &json!([{"id": 11, "name": "bug"}, {"id": 12, "name": "ui"}, {"id": 13, "name": "x"}]),
        ),
        reply(201, &issue_json(8, &["bug", "ui"])),
    ]);
    let issue = NewIssue {
        title: "Flaky".into(),
        body: "Details".into(),
        labels: vec!["ui".into(), "bug".into()],
    };

    let created = plugin(&fake).create_issue(&repo(), &issue)?;

    let requests = fake.requests.borrow();
    assert_eq!(requests[0].path, "/api/v1/repos/o/r/labels?page=1&limit=50");
    assert_eq!(requests[1].path, "/api/v1/repos/o/r/issues");
    let sent: Value = serde_json::from_slice(requests[1].body.as_ref().expect("body"))?;
    assert_eq!(
        sent,
        json!({"title": "Flaky", "body": "Details", "labels": [12, 11]})
    );
    assert_eq!(created.id.number, 8);
    assert_eq!(created.url, "https://git/o/r/issues/8");
    assert!(!created.pull_request);
    Ok(())
}

#[test]
fn create_with_an_unknown_label_is_invalid_input_and_posts_nothing() {
    let fake = fake(vec![
        reply(200, &json!([{"id": 11, "name": "bug"}])),
        empty(),
    ]);
    let issue = NewIssue {
        title: "Flaky".into(),
        body: String::new(),
        labels: vec!["bug".into(), "nope".into()],
    };

    let err = plugin(&fake).create_issue(&repo(), &issue).unwrap_err();

    assert!(
        matches!(&err, Error::InvalidInput(m) if m.contains("nope")),
        "{err:?}"
    );
    assert!(
        fake.requests
            .borrow()
            .iter()
            .all(|r| r.path.contains("/labels?")),
        "only the label lookup"
    );
}

#[test]
fn create_without_labels_does_not_look_labels_up() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, &issue_json(9, &[]))]);
    let issue = NewIssue {
        title: "T".into(),
        body: "B".into(),
        labels: Vec::new(),
    };

    plugin(&fake).create_issue(&repo(), &issue)?;

    assert_eq!(fake.requests.borrow().len(), 1);
    Ok(())
}

#[test]
fn get_flags_pull_request_numbers() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, &pull_json(4)),
        reply(200, &issue_json(5, &[])),
    ]);
    let id = |number| IssueRef {
        repo: repo(),
        number,
    };

    let pull = plugin(&fake).get_issue(&id(4))?;
    let issue = plugin(&fake).get_issue(&id(5))?;

    assert!(pull.pull_request);
    assert!(!issue.pull_request);
    Ok(())
}
