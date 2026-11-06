use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{Value, json};

use super::super::{Method, Request, Response};
use super::*;
use crate::darkfactory::integrations::types::RepoRef;
use crate::exports::darkfactory::integrations::pull_requests::{AuthorRole, ReviewVerdict};
use forge_common::draft;
use forge_common::log::Log;

const PR: &str = "/api/v1/repos/o/r/pulls/7";
const ISSUE_COMMENTS: &str = "/api/v1/repos/o/r/issues/7/comments";

/// Answers by method and path (404 for anything unknown) and records the requests.
#[derive(Default)]
struct Routes {
    responses: RefCell<HashMap<String, Response>>,
    /// Answers for a path with any query string, unless a response is registered for the exact one.
    any_query: RefCell<HashMap<String, Response>>,
    requests: RefCell<Vec<Request>>,
    warnings: RefCell<Vec<String>>,
}

impl Log for &Routes {
    fn warn(&self, message: &str) {
        self.warnings.borrow_mut().push(message.to_string());
    }
}

impl Transport for &Routes {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let key = key(request.method, &request.path);
        let path = request
            .path
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        self.requests.borrow_mut().push(request);
        let exact = self.responses.borrow().get(&key).cloned();
        let any_query = || self.any_query.borrow().get(&path).cloned();
        Ok(exact
            .or_else(any_query)
            .unwrap_or_else(|| response(404, &json!({ "message": format!("no route {key}") }))))
    }
}

impl Routes {
    fn on(&self, method: Method, path: &str, status: u16, body: &Value) {
        self.responses
            .borrow_mut()
            .insert(key(method, path), response(status, body));
    }

    fn get(&self, path: &str, body: &Value) {
        self.on(Method::Get, path, 200, body);
    }

    /// `items` for `path` with any query: the endpoint ignores `page` and `limit`, as Forgejo's
    /// issue comment listing does.
    fn get_any_query(&self, path: &str, items: &Value) {
        self.any_query
            .borrow_mut()
            .insert(path.to_string(), response(200, items));
    }

    /// `items` on page 1 and an empty page 2.
    fn pages(&self, path: &str, items: &Value) {
        let sep = if path.contains('?') { '&' } else { '?' };
        self.get(&format!("{path}{sep}page=1&limit=50"), items);
        self.get(&format!("{path}{sep}page=2&limit=50"), &json!([]));
    }

    fn count(&self, method: Method, path: &str) -> usize {
        self.requests
            .borrow()
            .iter()
            .filter(|r| r.method == method && r.path == path)
            .count()
    }

    fn sent_json(&self, method: Method, path: &str) -> Value {
        let requests = self.requests.borrow();
        let request = requests
            .iter()
            .find(|r| r.method == method && r.path == path)
            .expect("request sent");
        serde_json::from_slice(request.body.as_deref().expect("body")).expect("json")
    }
}

fn key(method: Method, path: &str) -> String {
    format!("{method:?} {path}")
}

fn response(status: u16, body: &Value) -> Response {
    Response {
        status,
        retry_after: None,
        body: body.to_string().into_bytes(),
    }
}

fn plugin(routes: &Routes) -> Forgejo<&Routes, &Routes> {
    Forgejo::new(
        routes,
        routes,
        draft::parse_prefixes(draft::DEFAULT_PREFIXES),
    )
}

fn pr() -> PullRequestRef {
    PullRequestRef {
        repo: RepoRef {
            owner: "o".into(),
            name: "r".into(),
        },
        number: 7,
    }
}

fn user(login: &str) -> Value {
    json!({ "id": 1, "login": login })
}

fn review(id: u64, login: &str, state: &str, submitted_at: &str) -> Value {
    json!({
        "id": id, "user": user(login), "state": state, "body": format!("review {id}"),
        "html_url": format!("http://forge/o/r/pulls/7#review-{id}"),
        "pull_request_url": "http://forge/o/r/pulls/7",
        "submitted_at": submitted_at, "updated_at": submitted_at,
    })
}

/// A review comment; `line` < 0 is on the old side of the diff.
fn line_comment(id: u64, login: &str, path: &str, line: i64, created_at: &str) -> Value {
    json!({
        "id": id, "user": user(login), "body": format!("comment {id}"), "path": path,
        "position": if line > 0 { line } else { 0 },
        "original_position": if line < 0 { -line } else { 0 },
        "resolver": null,
        "html_url": format!("http://forge/o/r/pulls/7/files#issuecomment-{id}"),
        "created_at": created_at, "updated_at": created_at,
    })
}

/// The bot `factory`; alice (write) and bob (read) reviewed, mallory has a pending review and carol
/// was asked for a review. Alice's comment on line 12 got a reply from the bot, which Forgejo stores
/// in alice's review.
fn scenario() -> Routes {
    let routes = Routes::default();
    routes.get("/api/v1/user", &user("factory"));
    routes.get_any_query(
        ISSUE_COMMENTS,
        &json!([{
            "id": 50, "user": user("bob"), "body": "looks odd", "html_url": "http://forge/c/50",
            "created_at": "2026-11-01T08:00:00Z", "updated_at": "2026-11-01T08:00:00Z",
        }]),
    );
    let reviews = [
        review(10, "alice", "REQUEST_CHANGES", "2026-11-01T10:00:00Z"),
        review(11, "factory", "COMMENT", "2026-11-01T12:00:00Z"),
        review(12, "mallory", "PENDING", ""),
        json!({"id": 13, "user": null, "state": "REQUEST_REVIEW", "body": ""}),
        review(14, "bob", "APPROVED", "2026-11-02T09:00:00+01:00"),
    ];
    for review in &reviews {
        routes.get(&format!("{PR}/reviews/{}", review["id"]), review);
    }
    routes.pages(&format!("{PR}/reviews"), &json!(reviews));
    let mut resolved = line_comment(101, "alice", "src/lib.rs", -5, "2026-11-01T10:00:00Z");
    resolved["resolver"] = user("alice");
    routes.get(
        &format!("{PR}/reviews/10/comments"),
        &json!([
            line_comment(100, "alice", "src/lib.rs", 12, "2026-11-01T10:00:00Z"),
            resolved,
            line_comment(110, "factory", "src/lib.rs", 12, "2026-11-01T12:00:00Z"),
        ]),
    );
    routes.get(&format!("{PR}/reviews/11/comments"), &json!([]));
    routes.get(
        &format!("{PR}/reviews/12/comments"),
        &json!([line_comment(
            120,
            "mallory",
            "src/lib.rs",
            12,
            "2026-11-01T13:00:00Z"
        )]),
    );
    routes.get(&format!("{PR}/reviews/14/comments"), &json!([]));
    routes.get(&permission_path("alice"), &json!({ "permission": "write" }));
    routes.get(&permission_path("bob"), &json!({ "permission": "read" }));
    routes
}

fn permission_path(login: &str) -> String {
    format!("/api/v1/repos/o/r/collaborators/{login}/permission")
}

fn ids<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    items.into_iter().collect()
}

#[test]
fn review_comments_of_one_review_on_the_same_path_and_line_form_one_thread() -> anyhow::Result<()> {
    let routes = scenario();
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let threads: Vec<_> = feedback
        .threads
        .iter()
        .map(|t| (t.id.as_str(), t.path.as_str(), t.line))
        .collect();
    assert_eq!(
        threads,
        vec![
            ("10:src/lib.rs:12", "src/lib.rs", Some(12)),
            ("10:src/lib.rs:-5", "src/lib.rs", Some(5)),
        ]
    );
    let line_12 = &feedback.threads[0];
    assert_eq!(
        ids(line_12.comments.iter().map(|c| c.id.as_str())),
        vec!["100", "110"]
    );
    assert_eq!(line_12.comments[1].body, "comment 110");
    assert_eq!(line_12.comments[1].created_at, "2026-11-01T12:00:00Z");
    Ok(())
}

#[test]
fn two_reviews_on_the_same_line_form_two_threads() -> anyhow::Result<()> {
    let routes = scenario();
    routes.get(
        &format!("{PR}/reviews/14/comments"),
        &json!([line_comment(
            140,
            "bob",
            "src/lib.rs",
            12,
            "2026-11-02T08:00:00Z"
        )]),
    );
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let threads: Vec<_> = feedback
        .threads
        .iter()
        .map(|t| (t.id.as_str(), ids(t.comments.iter().map(|c| c.id.as_str()))))
        .collect();
    assert_eq!(
        threads,
        vec![
            ("10:src/lib.rs:12", vec!["100", "110"]),
            ("10:src/lib.rs:-5", vec!["101"]),
            ("14:src/lib.rs:12", vec!["140"]),
        ]
    );
    Ok(())
}

#[test]
fn pending_reviews_and_review_requests_are_hidden() -> anyhow::Result<()> {
    let routes = scenario();
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let reviews: Vec<_> = feedback
        .reviews
        .iter()
        .map(|r| (r.id.as_str(), r.verdict))
        .collect();
    assert_eq!(
        reviews,
        vec![
            ("10", ReviewVerdict::ChangesRequested),
            ("11", ReviewVerdict::Commented),
            ("14", ReviewVerdict::Approved),
        ]
    );
    assert_eq!(
        routes.count(Method::Get, &format!("{PR}/reviews/12/comments")),
        0
    );
    assert_eq!(
        routes.count(Method::Get, &format!("{PR}/reviews/13/comments")),
        0
    );
    assert_eq!(routes.count(Method::Get, &permission_path("mallory")), 0);
    assert!(
        feedback
            .threads
            .iter()
            .all(|t| t.comments.iter().all(|c| c.id != "120"))
    );
    Ok(())
}

#[test]
fn a_resolver_marks_the_thread_resolved() -> anyhow::Result<()> {
    let routes = scenario();
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;
    let resolved: Vec<_> = feedback.threads.iter().map(|t| t.resolved).collect();
    assert_eq!(resolved, vec![false, true]);
    Ok(())
}

#[test]
fn roles_come_from_the_token_owner_and_one_permission_lookup_per_author() -> anyhow::Result<()> {
    let routes = scenario();
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    assert_eq!(feedback.comments[0].author.login, "bob");
    assert_eq!(feedback.comments[0].author.role, AuthorRole::Outside);
    let reviewers: Vec<_> = feedback
        .reviews
        .iter()
        .map(|r| (r.author.login.as_str(), r.author.role))
        .collect();
    assert_eq!(
        reviewers,
        vec![
            ("alice", AuthorRole::Member),
            ("factory", AuthorRole::Bot),
            ("bob", AuthorRole::Outside),
        ]
    );
    let thread_authors: Vec<_> = feedback.threads[0]
        .comments
        .iter()
        .map(|c| c.author.role)
        .collect();
    assert_eq!(thread_authors, vec![AuthorRole::Member, AuthorRole::Bot]);
    assert_eq!(routes.count(Method::Get, "/api/v1/user"), 1);
    assert_eq!(routes.count(Method::Get, &permission_path("alice")), 1);
    assert_eq!(routes.count(Method::Get, &permission_path("bob")), 1);
    assert_eq!(routes.count(Method::Get, &permission_path("factory")), 0);
    Ok(())
}

#[test]
fn write_admin_and_owner_permissions_are_members() -> anyhow::Result<()> {
    for (permission, role) in [
        ("owner", AuthorRole::Member),
        ("admin", AuthorRole::Member),
        ("write", AuthorRole::Member),
        ("read", AuthorRole::Outside),
        ("none", AuthorRole::Outside),
    ] {
        let routes = scenario();
        routes.get(
            &permission_path("alice"),
            &json!({ "permission": permission }),
        );
        let feedback = plugin(&routes).list_feedback(&pr(), None)?;
        assert_eq!(feedback.reviews[0].author.role, role, "{permission}");
    }
    Ok(())
}

#[test]
fn whoami_is_the_token_owner() -> anyhow::Result<()> {
    let routes = scenario();
    assert_eq!(plugin(&routes).whoami()?, "factory");
    Ok(())
}

#[test]
fn author_role_maps_permissions_and_the_token_owner() -> anyhow::Result<()> {
    let routes = scenario();
    routes.get(&permission_path("dave"), &json!({ "permission": "admin" }));
    let repo = pr().repo;
    for (login, role) in [
        ("Factory", AuthorRole::Bot),
        ("alice", AuthorRole::Member),
        ("dave", AuthorRole::Member),
        ("bob", AuthorRole::Outside),
        ("nobody", AuthorRole::Outside),
    ] {
        assert_eq!(plugin(&routes).author_role(&repo, login)?, role, "{login}");
    }
    assert_eq!(routes.count(Method::Get, &permission_path("Factory")), 0);
    Ok(())
}

#[test]
fn an_unknown_author_is_outside() -> anyhow::Result<()> {
    let routes = scenario();
    routes.on(
        Method::Get,
        &permission_path("bob"),
        404,
        &json!({ "message": "user does not exist" }),
    );
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;
    assert_eq!(feedback.comments[0].author.role, AuthorRole::Outside);
    Ok(())
}

#[test]
fn a_failed_permission_lookup_fails_the_call() {
    let routes = scenario();
    routes.on(
        Method::Get,
        &permission_path("alice"),
        500,
        &json!({ "message": "boom" }),
    );
    let err = plugin(&routes).list_feedback(&pr(), None).unwrap_err();
    assert!(matches!(err, Error::Upstream(_)), "{err:?}");
}

/// Exactly one warning was logged; it names `login` and the missing repository admin access.
fn assert_one_admin_warning(routes: &Routes, login: &str) {
    let warnings = routes.warnings.borrow();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains(login) && warnings[0].contains("repository admin"));
}

#[test]
fn an_invalid_credential_in_the_permission_lookup_fails_the_call_without_a_warning() {
    let routes = scenario();
    routes.on(
        Method::Get,
        &permission_path("alice"),
        401,
        &json!({ "message": "token is invalid" }),
    );
    let err = plugin(&routes).list_feedback(&pr(), None).unwrap_err();
    assert!(
        matches!(&err, Error::Unauthorized(m) if m.contains("token is invalid")),
        "{err:?}"
    );
    assert!(routes.warnings.borrow().is_empty());

    let err = plugin(&routes)
        .author_role(&pr().repo, "alice")
        .unwrap_err();
    assert!(matches!(err, Error::Unauthorized(_)), "{err:?}");
}

/// Forgejo answers the permission lookup with 403 unless the bot account is repository admin.
fn refuse_permission_lookup(routes: &Routes, login: &str) {
    routes.on(
        Method::Get,
        &permission_path(login),
        403,
        &json!({ "message": "Must have admin permission" }),
    );
}

#[test]
fn a_refused_permission_lookup_makes_the_author_outside_and_warns_once() -> anyhow::Result<()> {
    let routes = scenario();
    refuse_permission_lookup(&routes, "alice");
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let alice = [
        &feedback.reviews[0].author,
        &feedback.threads[0].comments[0].author,
        &feedback.threads[1].comments[0].author,
    ];
    assert!(
        alice
            .iter()
            .all(|a| a.login == "alice" && a.role == AuthorRole::Outside)
    );
    assert_eq!(routes.count(Method::Get, &permission_path("alice")), 1);
    assert_one_admin_warning(&routes, "alice");
    Ok(())
}

#[test]
fn a_refused_permission_lookup_makes_author_role_outside_and_warns() -> anyhow::Result<()> {
    let routes = scenario();
    refuse_permission_lookup(&routes, "alice");
    assert_eq!(
        plugin(&routes).author_role(&pr().repo, "alice")?,
        AuthorRole::Outside
    );
    assert_one_admin_warning(&routes, "alice");
    Ok(())
}

#[test]
fn an_unauthorized_error_names_the_request_and_the_forge_message() {
    let routes = scenario();
    routes.on(
        Method::Get,
        "/api/v1/user",
        403,
        &json!({ "message": "token does not have required scope" }),
    );
    let err = plugin(&routes).whoami().unwrap_err();
    assert!(
        matches!(&err, Error::Unauthorized(m)
            if m == "GET /api/v1/user: token does not have required scope"),
        "{err:?}"
    );
}

#[test]
fn since_keeps_newer_items_and_whole_threads_with_a_newer_comment() -> anyhow::Result<()> {
    let routes = scenario();
    let since_comments = format!("{ISSUE_COMMENTS}?since=2026-11-01T11%3A00%3A00Z");
    routes.get(&since_comments, &json!([]));
    let feedback = plugin(&routes).list_feedback(&pr(), Some("2026-11-01T11:00:00Z"))?;

    // Forgejo filters PR comments itself.
    assert_eq!(routes.count(Method::Get, &since_comments), 1);
    assert!(feedback.comments.is_empty());
    // At or after `since`, in any time zone: review 14 is 08:00 UTC on Nov 2.
    assert_eq!(
        ids(feedback.reviews.iter().map(|r| r.id.as_str())),
        vec!["11", "14"]
    );
    // The line 12 thread got a reply at 12:00, so it is kept whole; the old-side thread is not.
    assert_eq!(feedback.threads.len(), 1);
    assert_eq!(
        ids(feedback.threads[0].comments.iter().map(|c| c.id.as_str())),
        vec!["100", "110"]
    );
    // No permission lookup for alice's review, which was filtered out; she is in the thread.
    assert_eq!(routes.count(Method::Get, &permission_path("alice")), 1);
    Ok(())
}

#[test]
fn since_with_an_offset_is_compared_as_an_instant() -> anyhow::Result<()> {
    let routes = scenario();
    routes.get(
        &format!("{ISSUE_COMMENTS}?since=2026-11-01T13%3A30%3A00%2B01%3A00"),
        &json!([]),
    );
    // 12:30 UTC: only bob's review is newer.
    let feedback = plugin(&routes).list_feedback(&pr(), Some("2026-11-01T13:30:00+01:00"))?;
    assert_eq!(
        ids(feedback.reviews.iter().map(|r| r.id.as_str())),
        vec!["14"]
    );
    assert!(feedback.threads.is_empty());
    Ok(())
}

#[test]
fn since_that_is_not_rfc3339_is_invalid_input() {
    let routes = scenario();
    let err = plugin(&routes)
        .list_feedback(&pr(), Some("yesterday"))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert!(routes.requests.borrow().is_empty());
}

#[test]
fn each_pr_comment_is_returned_once_and_not_truncated() -> anyhow::Result<()> {
    let routes = scenario();
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;
    assert_eq!(ids(feedback.comments.iter().map(|c| c.id.as_str())), ["50"]);
    assert!(!feedback.truncated);
    assert_eq!(
        routes
            .requests
            .borrow()
            .iter()
            .filter(|r| r.path.starts_with(ISSUE_COMMENTS))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn review_listing_stops_at_the_page_limit_and_reports_truncation() -> anyhow::Result<()> {
    let routes = scenario();
    let reviews = json!([review(14, "bob", "APPROVED", "2026-11-02T09:00:00+01:00")]);
    for page in 1..=MAX_PAGES + 1 {
        routes.get(&format!("{PR}/reviews?page={page}&limit=50"), &reviews);
    }
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;
    assert!(feedback.truncated);
    assert_eq!(feedback.reviews.len(), MAX_PAGES);
    Ok(())
}

/// A pull request with `count` approved reviews by bob, review `n` submitted on day `n` of
/// October with one line comment on line `n`.
fn routes_with_reviews(count: u64) -> Routes {
    routes_with_reviews_listed(&(1..=count).collect::<Vec<_>>(), |n| {
        format!("2026-10-{n:02}T09:00:00Z")
    })
}

/// Like [`routes_with_reviews`] for the review ids `listed`, in that listing order, with the
/// submission time of review `n` given by `submitted`.
fn routes_with_reviews_listed(listed: &[u64], submitted: impl Fn(u64) -> String) -> Routes {
    let routes = Routes::default();
    routes.get("/api/v1/user", &user("factory"));
    routes.get_any_query(ISSUE_COMMENTS, &json!([]));
    routes.get(&permission_path("bob"), &json!({ "permission": "read" }));
    let reviews: Vec<_> = listed
        .iter()
        .map(|n| review(*n, "bob", "APPROVED", &submitted(*n)))
        .collect();
    routes.pages(&format!("{PR}/reviews"), &json!(reviews));
    for n in listed {
        let comment = line_comment(
            100 + n,
            "bob",
            "src/lib.rs",
            i64::try_from(*n).unwrap(),
            &submitted(*n),
        );
        routes.get(&format!("{PR}/reviews/{n}/comments"), &json!([comment]));
    }
    routes
}

/// The ids of the reviews the threads belong to (`<review-id>:<path>:<line>`).
fn thread_reviews(feedback: &Feedback) -> Vec<u64> {
    feedback
        .threads
        .iter()
        .map(|t| t.id.split(':').next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn review_threads_are_read_for_the_20_newest_reviews_only() -> anyhow::Result<()> {
    let routes = routes_with_reviews(25);
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    assert_eq!(feedback.reviews.len(), 25);
    assert_eq!(thread_reviews(&feedback), (6..=25).collect::<Vec<_>>());
    assert!(feedback.truncated);
    let requested: Vec<_> = (1..=25)
        .filter(|n| routes.count(Method::Get, &format!("{PR}/reviews/{n}/comments")) == 1)
        .collect();
    assert_eq!(requested, (6..=25).collect::<Vec<_>>());
    Ok(())
}

#[test]
fn the_20_reviews_submitted_last_are_selected_whatever_their_ids_and_listing_order()
-> anyhow::Result<()> {
    // Review 1 was submitted last; the listing is not in submission order.
    let listed: Vec<u64> = (1..=25).rev().collect();
    let routes = routes_with_reviews_listed(&listed, |n| {
        if n == 1 {
            "2026-11-01T09:00:00Z".to_string()
        } else {
            format!("2026-10-{n:02}T09:00:00Z")
        }
    });
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let mut expected: Vec<u64> = std::iter::once(1).chain(7..=25).collect();
    let mut threads = thread_reviews(&feedback);
    threads.sort_unstable();
    expected.sort_unstable();
    assert_eq!(threads, expected);
    let requested: Vec<u64> = (1..=25)
        .filter(|n| routes.count(Method::Get, &format!("{PR}/reviews/{n}/comments")) == 1)
        .collect();
    assert_eq!(requested, expected);
    assert!(feedback.truncated);
    Ok(())
}

#[test]
fn review_threads_of_all_reviews_are_returned_and_not_truncated_up_to_20_reviews()
-> anyhow::Result<()> {
    let routes = routes_with_reviews(3);
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    assert_eq!(thread_reviews(&feedback), [1, 2, 3]);
    assert!(!feedback.truncated);
    Ok(())
}

#[test]
fn a_reply_in_a_thread_of_a_review_beyond_the_newest_20_is_not_returned() -> anyhow::Result<()> {
    let routes = routes_with_reviews(25);
    routes.get(
        &format!("{PR}/reviews/1/comments"),
        &json!([
            line_comment(101, "bob", "src/lib.rs", 1, "2026-10-01T09:00:00Z"),
            line_comment(900, "bob", "src/lib.rs", 1, "2026-11-05T08:00:00Z"),
        ]),
    );
    let feedback = plugin(&routes).list_feedback(&pr(), Some("2026-11-05T00:00:00Z"))?;

    assert!(feedback.threads.is_empty());
    assert!(feedback.truncated);
    Ok(())
}

#[test]
fn complete_listing_is_not_truncated() -> anyhow::Result<()> {
    let routes = scenario();
    assert!(!plugin(&routes).list_feedback(&pr(), None)?.truncated);
    Ok(())
}

fn posted(id: u64) -> Value {
    json!({ "id": id, "html_url": format!("http://forge/c/{id}"), "path": "x", "body": "b",
        "created_at": "2026-11-03T10:00:00Z", "updated_at": "2026-11-03T10:00:00Z" })
}

fn reply_routes(review_id: u64) -> Routes {
    let routes = scenario();
    routes.on(
        Method::Post,
        &format!("{PR}/reviews/{review_id}/comments"),
        200,
        &posted(130),
    );
    routes
}

#[test]
fn reply_posts_to_the_review_that_started_the_thread() -> anyhow::Result<()> {
    let routes = reply_routes(10);
    let thread = plugin(&routes).list_feedback(&pr(), None)?.threads[0]
        .id
        .clone();
    let comment = plugin(&routes).reply(&pr(), &thread, "fixed")?;

    assert_eq!(
        (comment.id.as_str(), comment.url.as_str()),
        ("130", "http://forge/c/130")
    );
    assert_eq!(
        routes.sent_json(Method::Post, &format!("{PR}/reviews/10/comments")),
        json!({ "path": "src/lib.rs", "body": "fixed", "new_position": 12 })
    );
    Ok(())
}

#[test]
fn reply_on_an_old_side_line_uses_old_position() -> anyhow::Result<()> {
    let routes = reply_routes(10);
    plugin(&routes).reply(&pr(), "10:src/lib.rs:-5", "fixed")?;
    assert_eq!(
        routes.sent_json(Method::Post, &format!("{PR}/reviews/10/comments")),
        json!({ "path": "src/lib.rs", "body": "fixed", "old_position": 5 })
    );
    Ok(())
}

#[test]
fn reply_keeps_colons_in_the_path() -> anyhow::Result<()> {
    let routes = reply_routes(10);
    routes.get(
        &format!("{PR}/reviews/10/comments"),
        &json!([line_comment(
            100,
            "alice",
            "docs/a:b:c.md",
            3,
            "2026-11-01T10:00:00Z"
        )]),
    );
    plugin(&routes).reply(&pr(), "10:docs/a:b:c.md:3", "fixed")?;
    assert_eq!(
        routes.sent_json(Method::Post, &format!("{PR}/reviews/10/comments")),
        json!({ "path": "docs/a:b:c.md", "body": "fixed", "new_position": 3 })
    );
    Ok(())
}

#[test]
fn malformed_thread_ids_are_invalid_input() {
    for thread in [
        "",
        "10",
        "10:src/lib.rs",
        "x:src/lib.rs:12",
        "-10:src/lib.rs:12",
        "10::12",
        "10:src/lib.rs:",
        "10:src/lib.rs:twelve",
        "PVT_kwDOAbc",
    ] {
        let routes = reply_routes(10);
        let err = plugin(&routes).reply(&pr(), thread, "x").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{thread:?}: {err:?}");
        assert!(routes.requests.borrow().is_empty(), "{thread:?}");
    }
}

#[test]
fn a_review_of_another_pull_request_is_invalid_input() {
    // Forgejo answers 404 for a review id of another pull request.
    let routes = reply_routes(99);
    let err = plugin(&routes)
        .reply(&pr(), "99:src/lib.rs:12", "x")
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert_eq!(routes.count(Method::Get, &format!("{PR}/reviews/99")), 1);
    assert_eq!(
        routes.count(Method::Post, &format!("{PR}/reviews/99/comments")),
        0
    );

    // A review whose pull request URL names another pull request is rejected as well.
    let routes = reply_routes(10);
    let mut other = review(10, "alice", "COMMENT", "2026-11-01T10:00:00Z");
    other["pull_request_url"] = "http://forge/o/r/pulls/70".into();
    routes.get(&format!("{PR}/reviews/10"), &other);
    let err = plugin(&routes)
        .reply(&pr(), "10:src/lib.rs:12", "x")
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert_eq!(
        routes.count(Method::Post, &format!("{PR}/reviews/10/comments")),
        0
    );
}

#[test]
fn a_path_or_line_without_a_comment_in_the_review_is_invalid_input() {
    // Review 10 has comments on src/lib.rs:12 and src/lib.rs:-5 only.
    for thread in ["10:other.rs:12", "10:src/lib.rs:99", "10:src/lib.rs:5"] {
        let routes = reply_routes(10);
        let err = plugin(&routes).reply(&pr(), thread, "x").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{thread:?}: {err:?}");
        assert_eq!(
            routes.count(Method::Post, &format!("{PR}/reviews/10/comments")),
            0,
            "{thread:?}"
        );
    }
}

#[test]
fn a_pending_review_is_invalid_input() {
    let routes = reply_routes(12);
    routes.get(
        &format!("{PR}/reviews/12"),
        &review(12, "mallory", "PENDING", ""),
    );
    let err = plugin(&routes)
        .reply(&pr(), "12:src/lib.rs:12", "x")
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert_eq!(
        routes.count(Method::Post, &format!("{PR}/reviews/12/comments")),
        0
    );
}

#[test]
fn review_carries_its_updated_at() -> anyhow::Result<()> {
    let routes = scenario();
    let mut edited = review(10, "alice", "REQUEST_CHANGES", "2026-11-01T10:00:00Z");
    edited["updated_at"] = json!("2026-11-03T09:00:00Z");
    routes.pages(&format!("{PR}/reviews"), &json!([edited]));
    let feedback = plugin(&routes).list_feedback(&pr(), None)?;

    let updated: Vec<_> = feedback
        .reviews
        .iter()
        .map(|r| r.updated_at.as_str())
        .collect();
    assert_eq!(updated, ["2026-11-03T09:00:00Z"]);
    Ok(())
}
