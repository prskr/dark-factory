//! Scenarios of the `forgejo-integration` spec "Merge a pull request" and "Create a branch".

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

/// What Forgejo sends for a successful merge: 200 with an empty body.
fn empty(status: u16) -> Response {
    Response {
        status,
        retry_after: None,
        body: Vec::new(),
    }
}

fn pr(state: &str, merged: bool) -> Value {
    json!({
        "number": 12, "html_url": "https://git/pr/12", "title": "t", "body": "",
        "state": state, "merged": merged,
        "head": {"ref": "factory/task/DF-31", "sha": "abc", "repo": {"full_name": "o/r"}},
        "base": {"ref": "factory/task/DF-30", "sha": "def"}
    })
}

fn repo() -> RepoRef {
    RepoRef {
        owner: "o".into(),
        name: "r".into(),
    }
}

fn id() -> PullRequestRef {
    PullRequestRef {
        repo: repo(),
        number: 12,
    }
}

fn plugin(fake: &Fake) -> Forgejo<&Fake, StderrLog> {
    Forgejo::new(
        fake,
        StderrLog,
        draft::parse_prefixes(draft::DEFAULT_PREFIXES),
    )
}

fn paths(fake: &Fake) -> Vec<String> {
    fake.requests
        .borrow()
        .iter()
        .map(|r| format!("{:?} {}", r.method, r.path))
        .collect()
}

fn sent_json(fake: &Fake, index: usize) -> Value {
    let body = fake.requests.borrow()[index].body.clone().expect("body");
    serde_json::from_slice(&body).expect("json")
}

#[test]
fn merge_uses_the_repository_merge_style_and_pins_the_head() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, &pr("open", false)),
        reply(200, &json!({"default_merge_style": "squash"})),
        empty(200),
    ]);
    plugin(&fake).merge(&id(), "abc")?;
    assert_eq!(
        paths(&fake),
        [
            "Get /api/v1/repos/o/r/pulls/12",
            "Get /api/v1/repos/o/r",
            "Post /api/v1/repos/o/r/pulls/12/merge"
        ]
    );
    assert_eq!(
        sent_json(&fake, 2),
        json!({"Do": "squash", "head_commit_id": "abc"})
    );
    Ok(())
}

#[test]
fn merge_with_a_moved_head_fails_without_asking_the_forge_to_merge() {
    let fake = fake(vec![reply(200, &pr("open", false))]);
    let err = plugin(&fake).merge(&id(), "zzz").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m.contains("abc") && m.contains("zzz")));
    assert_eq!(fake.requests.borrow().len(), 1);
}

#[test]
fn merging_an_already_merged_pull_request_with_the_same_head_succeeds() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, &pr("closed", true))]);
    plugin(&fake).merge(&id(), "abc")?;
    assert_eq!(fake.requests.borrow().len(), 1);
    Ok(())
}

#[test]
fn merging_a_pull_request_merged_with_another_head_is_a_conflict() {
    let fake = fake(vec![reply(200, &pr("closed", true))]);
    let err = plugin(&fake).merge(&id(), "zzz").unwrap_err();
    assert!(matches!(err, Error::Conflict(_)));
}

#[test]
fn merging_a_closed_pull_request_is_invalid() {
    let fake = fake(vec![reply(200, &pr("closed", false))]);
    let err = plugin(&fake).merge(&id(), "abc").unwrap_err();
    assert!(matches!(err, Error::InvalidInput(m) if m.contains("closed")));
}

#[test]
fn a_refused_merge_is_a_conflict_with_the_forges_message() {
    let fake = fake(vec![
        reply(200, &pr("open", false)),
        reply(200, &json!({"default_merge_style": "merge"})),
        reply(405, &json!({"message": "Please try again later"})),
    ]);
    let err = plugin(&fake).merge(&id(), "abc").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m == "Please try again later"));
}

#[test]
fn a_refusal_without_a_message_still_says_why() {
    let fake = fake(vec![
        reply(200, &pr("open", false)),
        reply(200, &json!({"default_merge_style": "merge"})),
        reply(405, &json!({"message": ""})),
    ]);
    let err = plugin(&fake).merge(&id(), "abc").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m.contains("not mergeable")));
}

#[test]
fn a_head_that_moved_during_the_merge_is_a_conflict() {
    let fake = fake(vec![
        reply(200, &pr("open", false)),
        reply(200, &json!({"default_merge_style": "merge"})),
        reply(409, &json!({"message": "head out of date"})),
    ]);
    let err = plugin(&fake).merge(&id(), "abc").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m == "head out of date"));
}

#[test]
fn create_branch_posts_the_new_name_and_its_origin() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, &json!({"name": "factory/task/DF-30"}))]);
    plugin(&fake).create_branch(&repo(), "factory/task/DF-30", "main")?;
    assert_eq!(paths(&fake), ["Post /api/v1/repos/o/r/branches"]);
    assert_eq!(
        sent_json(&fake, 0),
        json!({"new_branch_name": "factory/task/DF-30", "old_ref_name": "main"})
    );
    Ok(())
}

#[test]
fn creating_an_existing_branch_is_a_conflict() {
    let fake = fake(vec![reply(
        409,
        &json!({"message": "The branch already exists."}),
    )]);
    let err = plugin(&fake)
        .create_branch(&repo(), "factory/task/DF-30", "main")
        .unwrap_err();
    assert!(matches!(err, Error::Conflict(_)));
}

#[test]
fn creating_a_branch_from_an_unknown_ref_is_not_found() {
    // Forgejo answers 500 with this message.
    let fake = fake(vec![reply(
        500,
        &json!({"message": "object does not exist [id: nope, rel_path: ]"}),
    )]);
    let err = plugin(&fake)
        .create_branch(&repo(), "factory/task/DF-30", "nope")
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(m) if m.contains("nope")));
}

#[test]
fn only_a_missing_origin_is_not_found() {
    let fake = fake(vec![reply(500, &json!({"message": "boom"}))]);
    let err = plugin(&fake)
        .create_branch(&repo(), "factory/task/DF-30", "main")
        .unwrap_err();
    assert!(matches!(err, Error::Upstream(_)));
}
