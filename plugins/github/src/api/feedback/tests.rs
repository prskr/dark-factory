//! Scenarios of the `github-integration` spec for `list-feedback` and `reply`.

use serde_json::{Value, json};

use super::super::tests::{
    Fake, FixedClock, GRAPHQL_URL, calls, fake, github, pr_ref, reply, sent_json,
};
use super::super::{Method, Response};
use super::*;
use crate::exports::darkfactory::integrations::pull_requests::{AuthorRole, ReviewVerdict};

fn connection(nodes: Value) -> Value {
    json!({ "pageInfo": { "hasPreviousPage": false }, "nodes": nodes })
}

fn comment(id: &str, login: &str, association: &str, created_at: &str) -> Value {
    json!({
        "id": id, "author": { "login": login }, "authorAssociation": association,
        "body": format!("comment {id}"), "url": format!("https://github.com/o/r/pull/7#{id}"),
        "createdAt": created_at, "updatedAt": created_at,
    })
}

fn review_comment(id: &str, login: &str, association: &str, created_at: &str) -> Value {
    let mut comment = comment(id, login, association, created_at);
    comment["state"] = json!("SUBMITTED");
    comment
}

fn review(id: &str, login: &str, association: &str, state: &str, submitted_at: &str) -> Value {
    json!({
        "id": id, "author": { "login": login }, "authorAssociation": association,
        "state": state, "body": format!("review {id}"),
        "url": format!("https://github.com/o/r/pull/7#{id}"),
        "submittedAt": submitted_at, "updatedAt": submitted_at,
    })
}

/// The bot `factory-bot`; bob (first-time contributor) commented, alice (member) requested changes
/// on line 12 and the bot replied in that thread, carol (owner) approved. Alice's comment on
/// `src/main.rs` (whole file) is resolved.
fn scenario() -> Value {
    json!({ "data": {
        "viewer": { "login": "factory-bot" },
        "repository": { "pullRequest": {
            "comments": connection(json!([
                comment("IC_50", "bob", "FIRST_TIME_CONTRIBUTOR", "2026-11-01T08:00:00Z"),
            ])),
            "reviews": connection(json!([
                review("PRR_10", "alice", "MEMBER", "CHANGES_REQUESTED", "2026-11-01T10:00:00Z"),
                review("PRR_11", "Factory-Bot", "COLLABORATOR", "COMMENTED", "2026-11-01T12:00:00Z"),
                review("PRR_14", "carol", "OWNER", "APPROVED", "2026-11-02T08:00:00Z"),
            ])),
            "reviewThreads": connection(json!([
                {
                    "id": "PRRT_1", "isResolved": false, "path": "src/lib.rs", "line": 12,
                    "comments": connection(json!([
                        review_comment("PRRC_100", "alice", "MEMBER", "2026-11-01T10:00:00Z"),
                        review_comment("PRRC_110", "factory-bot", "COLLABORATOR", "2026-11-01T12:00:00Z"),
                    ])),
                },
                {
                    "id": "PRRT_2", "isResolved": true, "path": "src/main.rs", "line": null,
                    "comments": connection(json!([
                        review_comment("PRRC_101", "alice", "MEMBER", "2026-11-01T10:00:00Z"),
                    ])),
                },
            ])),
        }},
    }})
}

fn list(response: Value, since: Option<&str>) -> Result<(Feedback, Fake), Error> {
    let fake = fake(vec![reply(200, response)]);
    let feedback = github(&fake, &[]).list_feedback(&pr_ref(), since)?;
    Ok((feedback, fake))
}

fn ids<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    items.into_iter().collect()
}

#[test]
fn list_feedback_sends_one_graphql_query_for_viewer_comments_reviews_and_threads()
-> anyhow::Result<()> {
    let (_, fake) = list(scenario(), None)?;

    assert_eq!(calls(&fake), [(Method::Post, GRAPHQL_URL.to_string())]);
    let sent = sent_json(&fake, 0);
    assert_eq!(
        sent["variables"],
        json!({ "owner": "o", "name": "r", "number": 7 })
    );
    let query = sent["query"].as_str().unwrap_or_default();
    for part in [
        "viewer { login }",
        "comments(last: 100)",
        "reviews(last: 100",
        "reviewThreads(last: 100)",
        "isResolved",
        "authorAssociation",
        "hasPreviousPage",
    ] {
        assert!(query.contains(part), "{part} missing in {query}");
    }
    assert!(!query.contains("first:"), "oldest-first paging in {query}");
    Ok(())
}

// Scenario "Replies kept in their thread".
#[test]
fn replies_stay_in_their_review_thread() -> anyhow::Result<()> {
    let (feedback, _) = list(scenario(), None)?;

    let threads: Vec<_> = feedback
        .threads
        .iter()
        .map(|t| (t.id.as_str(), t.path.as_str(), t.line))
        .collect();
    assert_eq!(
        threads,
        vec![
            ("PRRT_1", "src/lib.rs", Some(12)),
            ("PRRT_2", "src/main.rs", None),
        ]
    );
    let line_12 = &feedback.threads[0];
    assert_eq!(
        ids(line_12.comments.iter().map(|c| c.id.as_str())),
        vec!["PRRC_100", "PRRC_110"]
    );
    let reply = &line_12.comments[1];
    assert_eq!(reply.body, "comment PRRC_110");
    assert_eq!(reply.url, "https://github.com/o/r/pull/7#PRRC_110");
    assert_eq!(reply.created_at, "2026-11-01T12:00:00Z");
    Ok(())
}

// Scenario "Resolved thread".
#[test]
fn is_resolved_marks_the_thread_resolved() -> anyhow::Result<()> {
    let (feedback, _) = list(scenario(), None)?;
    let resolved: Vec<_> = feedback.threads.iter().map(|t| t.resolved).collect();
    assert_eq!(resolved, vec![false, true]);
    Ok(())
}

#[test]
fn pr_comments_and_reviews_are_mapped() -> anyhow::Result<()> {
    let (feedback, _) = list(scenario(), None)?;

    let comment = &feedback.comments[0];
    assert_eq!(
        (
            comment.id.as_str(),
            comment.body.as_str(),
            comment.url.as_str()
        ),
        (
            "IC_50",
            "comment IC_50",
            "https://github.com/o/r/pull/7#IC_50"
        )
    );
    assert_eq!(comment.updated_at, "2026-11-01T08:00:00Z");
    let reviews: Vec<_> = feedback
        .reviews
        .iter()
        .map(|r| (r.id.as_str(), r.verdict, r.submitted_at.as_str()))
        .collect();
    assert_eq!(
        reviews,
        vec![
            (
                "PRR_10",
                ReviewVerdict::ChangesRequested,
                "2026-11-01T10:00:00Z"
            ),
            ("PRR_11", ReviewVerdict::Commented, "2026-11-01T12:00:00Z"),
            ("PRR_14", ReviewVerdict::Approved, "2026-11-02T08:00:00Z"),
        ]
    );
    assert_eq!(feedback.reviews[0].body, "review PRR_10");
    Ok(())
}

#[test]
fn pending_reviews_and_pending_review_comments_are_hidden() -> anyhow::Result<()> {
    let mut response = scenario();
    let pr = &mut response["data"]["repository"]["pullRequest"];
    pr["reviews"]["nodes"]
        .as_array_mut()
        .expect("reviews")
        .push(review(
            "PRR_12",
            "factory-bot",
            "COLLABORATOR",
            "PENDING",
            "",
        ));
    let mut pending = review_comment("PRRC_120", "factory-bot", "COLLABORATOR", "");
    pending["state"] = json!("PENDING");
    pr["reviewThreads"]["nodes"][0]["comments"]["nodes"]
        .as_array_mut()
        .expect("comments")
        .push(pending.clone());
    pr["reviewThreads"]["nodes"]
        .as_array_mut()
        .expect("threads")
        .push(json!({
            "id": "PRRT_3", "isResolved": false, "path": "a.rs", "line": 1,
            "comments": connection(json!([pending])),
        }));
    let (feedback, fake) = list(response, None)?;

    let query = sent_json(&fake, 0)["query"].to_string();
    assert!(!query.contains("PENDING"), "{query}");
    assert_eq!(
        ids(feedback.reviews.iter().map(|r| r.id.as_str())),
        vec!["PRR_10", "PRR_11", "PRR_14"]
    );
    assert_eq!(
        ids(feedback.threads.iter().map(|t| t.id.as_str())),
        vec!["PRRT_1", "PRRT_2"]
    );
    assert_eq!(feedback.threads[0].comments.len(), 2);
    Ok(())
}

#[test]
fn a_dismissed_review_is_listed_as_commented() -> anyhow::Result<()> {
    let mut response = scenario();
    response["data"]["repository"]["pullRequest"]["reviews"]["nodes"][0]["state"] =
        json!("DISMISSED");
    let (feedback, _) = list(response, None)?;
    assert_eq!(feedback.reviews[0].verdict, ReviewVerdict::Commented);
    Ok(())
}

#[test]
fn owner_member_and_collaborator_are_members_everyone_else_is_outside() -> anyhow::Result<()> {
    for (association, role) in [
        ("OWNER", AuthorRole::Member),
        ("MEMBER", AuthorRole::Member),
        ("COLLABORATOR", AuthorRole::Member),
        ("CONTRIBUTOR", AuthorRole::Outside),
        ("FIRST_TIME_CONTRIBUTOR", AuthorRole::Outside),
        ("FIRST_TIMER", AuthorRole::Outside),
        ("MANNEQUIN", AuthorRole::Outside),
        ("NONE", AuthorRole::Outside),
    ] {
        let mut response = scenario();
        response["data"]["repository"]["pullRequest"]["reviews"]["nodes"][0]["authorAssociation"] =
            json!(association);
        let (feedback, _) = list(response, None)?;
        assert_eq!(feedback.reviews[0].author.role, role, "{association}");
    }
    Ok(())
}

// Scenario "First-time contributor".
#[test]
fn a_first_time_contributor_is_outside() -> anyhow::Result<()> {
    let (feedback, _) = list(scenario(), None)?;
    let author = &feedback.comments[0].author;
    assert_eq!(
        (author.login.as_str(), author.role),
        ("bob", AuthorRole::Outside)
    );
    Ok(())
}

#[test]
fn the_viewer_is_the_bot_whatever_its_association_or_case() -> anyhow::Result<()> {
    let (feedback, _) = list(scenario(), None)?;
    let reviewers: Vec<_> = feedback
        .reviews
        .iter()
        .map(|r| (r.author.login.as_str(), r.author.role))
        .collect();
    assert_eq!(
        reviewers,
        vec![
            ("alice", AuthorRole::Member),
            ("Factory-Bot", AuthorRole::Bot),
            ("carol", AuthorRole::Member),
        ]
    );
    let thread_roles: Vec<_> = feedback.threads[0]
        .comments
        .iter()
        .map(|c| c.author.role)
        .collect();
    assert_eq!(thread_roles, vec![AuthorRole::Member, AuthorRole::Bot]);
    Ok(())
}

#[test]
fn a_deleted_author_has_an_empty_login_and_is_outside() -> anyhow::Result<()> {
    let mut response = scenario();
    response["data"]["repository"]["pullRequest"]["comments"]["nodes"][0]["author"] = Value::Null;
    let (feedback, _) = list(response, None)?;
    let author = &feedback.comments[0].author;
    assert_eq!(
        (author.login.as_str(), author.role),
        ("", AuthorRole::Outside)
    );
    Ok(())
}

#[test]
fn any_older_page_sets_truncated_and_keeps_the_newest_items_in_order() -> anyhow::Result<()> {
    for pointer in [
        "/data/repository/pullRequest/comments/pageInfo/hasPreviousPage",
        "/data/repository/pullRequest/reviews/pageInfo/hasPreviousPage",
        "/data/repository/pullRequest/reviewThreads/pageInfo/hasPreviousPage",
        "/data/repository/pullRequest/reviewThreads/nodes/1/comments/pageInfo/hasPreviousPage",
    ] {
        let mut response = scenario();
        *response.pointer_mut(pointer).expect(pointer) = json!(true);
        let (feedback, _) = list(response, None)?;
        assert!(feedback.truncated, "{pointer}");
        assert_eq!(
            ids(feedback.reviews.iter().map(|r| r.id.as_str())),
            vec!["PRR_10", "PRR_11", "PRR_14"],
            "{pointer}"
        );
        assert_eq!(
            ids(feedback.threads[0].comments.iter().map(|c| c.id.as_str())),
            vec!["PRRC_100", "PRRC_110"],
            "{pointer}"
        );
    }
    let (feedback, _) = list(scenario(), None)?;
    assert!(!feedback.truncated);
    Ok(())
}

#[test]
fn since_keeps_newer_items_and_whole_threads_with_a_newer_comment() -> anyhow::Result<()> {
    let mut response = scenario();
    let pr = &mut response["data"]["repository"]["pullRequest"];
    pr["comments"]["nodes"]
        .as_array_mut()
        .expect("comments")
        .push(comment("IC_51", "alice", "MEMBER", "2026-11-01T09:00:00Z"));
    pr["comments"]["nodes"][1]["updatedAt"] = json!("2026-11-01T11:30:00Z");
    let (feedback, _) = list(response, Some("2026-11-01T11:00:00Z"))?;

    // Bob's comment is older; alice's was edited after `since`.
    assert_eq!(
        ids(feedback.comments.iter().map(|c| c.id.as_str())),
        vec!["IC_51"]
    );
    assert_eq!(
        ids(feedback.reviews.iter().map(|r| r.id.as_str())),
        vec!["PRR_11", "PRR_14"]
    );
    // The line 12 thread got a reply at 12:00, so it is kept whole; the other thread is not.
    assert_eq!(feedback.threads.len(), 1);
    assert_eq!(
        ids(feedback.threads[0].comments.iter().map(|c| c.id.as_str())),
        vec!["PRRC_100", "PRRC_110"]
    );
    Ok(())
}

#[test]
fn since_with_an_offset_is_compared_as_an_instant() -> anyhow::Result<()> {
    // 12:30 UTC: only carol's review is newer.
    let (feedback, _) = list(scenario(), Some("2026-11-01T13:30:00+01:00"))?;
    assert!(feedback.comments.is_empty());
    assert_eq!(
        ids(feedback.reviews.iter().map(|r| r.id.as_str())),
        vec!["PRR_14"]
    );
    assert!(feedback.threads.is_empty());
    Ok(())
}

#[test]
fn since_that_is_not_rfc3339_is_invalid_input_without_a_request() {
    let fake = fake(vec![]);
    let err = github(&fake, &[])
        .list_feedback(&pr_ref(), Some("yesterday"))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert!(fake.requests.borrow().is_empty());
}

#[test]
fn a_missing_pull_request_is_not_found() {
    let response = json!({
        "data": { "viewer": { "login": "factory-bot" }, "repository": { "pullRequest": null } },
        "errors": [{ "type": "NOT_FOUND", "message": "Could not resolve to a PullRequest" }],
    });
    let fake = fake(vec![reply(200, response)]);
    let err = github(&fake, &[])
        .list_feedback(&pr_ref(), None)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

fn thread_node(number: u64, name_with_owner: &str) -> Response {
    reply(
        200,
        json!({ "data": { "node": {
            "pullRequest": { "number": number, "repository": { "nameWithOwner": name_with_owner } },
        }}}),
    )
}

fn reply_posted() -> Response {
    reply(
        200,
        json!({ "data": { "addPullRequestReviewThreadReply": { "comment": {
            "id": "PRRC_130", "url": "https://github.com/o/r/pull/7#discussion_r130",
        }}}}),
    )
}

fn plugin(fake: &Fake) -> GitHub<&Fake, FixedClock, &Fake> {
    github(fake, &[])
}

// Scenario "Reply joins the thread".
#[test]
fn reply_checks_the_thread_and_adds_the_reply_to_it() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, scenario()),
        thread_node(7, "o/r"),
        reply_posted(),
    ]);
    let thread = plugin(&fake).list_feedback(&pr_ref(), None)?.threads[0]
        .id
        .clone();
    let posted = plugin(&fake).reply(&pr_ref(), &thread, "fixed")?;

    assert_eq!(
        (posted.id.as_str(), posted.url.as_str()),
        ("PRRC_130", "https://github.com/o/r/pull/7#discussion_r130")
    );
    assert_eq!(calls(&fake).len(), 3);
    let lookup = sent_json(&fake, 1);
    let lookup_query = lookup["query"].as_str().unwrap_or_default();
    assert!(
        lookup_query.contains("node(id: $id)") && lookup_query.contains("PullRequestReviewThread"),
        "{lookup_query}"
    );
    assert_eq!(lookup["variables"], json!({ "id": "PRRT_1" }));
    let mutation = sent_json(&fake, 2);
    assert!(
        mutation["query"]
            .as_str()
            .is_some_and(|q| q.contains("addPullRequestReviewThreadReply")),
        "{mutation}"
    );
    assert_eq!(
        mutation["variables"],
        json!({ "thread": "PRRT_1", "body": "fixed" })
    );
    Ok(())
}

#[test]
fn a_thread_of_another_pull_request_is_invalid_input() {
    for node in [thread_node(8, "o/r"), thread_node(7, "o/other")] {
        let fake = fake(vec![node]);
        let err = plugin(&fake).reply(&pr_ref(), "PRRT_1", "x").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
        assert_eq!(calls(&fake).len(), 1);
    }
}

#[test]
fn the_repository_is_compared_case_insensitively() -> anyhow::Result<()> {
    let fake = fake(vec![thread_node(7, "O/R"), reply_posted()]);
    plugin(&fake).reply(&pr_ref(), "PRRT_1", "x")?;
    assert_eq!(calls(&fake).len(), 2);
    Ok(())
}

#[test]
fn a_node_that_is_not_a_review_thread_is_invalid_input() {
    let fake = fake(vec![reply(200, json!({ "data": { "node": {} } }))]);
    let err = plugin(&fake).reply(&pr_ref(), "PR_kwDO7", "x").unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert_eq!(calls(&fake).len(), 1);
}

#[test]
fn an_unknown_thread_id_is_not_found() {
    let fake = fake(vec![reply(
        200,
        json!({
            "data": { "node": null },
            "errors": [{ "type": "NOT_FOUND",
                "message": "Could not resolve to a node with the global id of 'nope'" }],
        }),
    )]);
    let err = plugin(&fake).reply(&pr_ref(), "nope", "x").unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
    assert_eq!(calls(&fake).len(), 1);
}

#[test]
fn an_empty_thread_id_is_invalid_input_without_a_request() {
    let fake = fake(vec![]);
    let err = plugin(&fake).reply(&pr_ref(), "", "x").unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert!(fake.requests.borrow().is_empty());
}
