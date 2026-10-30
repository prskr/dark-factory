//! Scenarios of the `github-integration` spec against a fake transport.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use serde_json::json;

use super::*;
use crate::darkfactory::integrations::types::{RateLimit, UpstreamError};
use crate::settings::DraftMode;

const PR_URL: &str = "https://api.github.com/repos/o/r/pulls/7";
const GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// Replays canned responses and records the requests and warnings.
struct Fake {
    responses: RefCell<VecDeque<Response>>,
    requests: RefCell<Vec<Request>>,
    warnings: RefCell<Vec<String>>,
}

impl Log for &Fake {
    fn warn(&self, message: &str) {
        self.warnings.borrow_mut().push(message.to_string());
    }
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

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> u64 {
        1_800_000_000
    }
}

fn fake(responses: Vec<Response>) -> Fake {
    Fake {
        responses: RefCell::new(responses.into()),
        requests: RefCell::default(),
        warnings: RefCell::default(),
    }
}

fn reply(status: u16, body: serde_json::Value) -> Response {
    Response {
        status,
        body: body.to_string().into_bytes(),
        ..Response::default()
    }
}

/// Settings as `wasi:config/store` holds them; `base-url` is github.com's API unless given.
fn settings(values: &[(&str, &str)]) -> Result<Settings, Error> {
    let mut values: HashMap<&str, &str> = values.iter().copied().collect();
    values.entry("base-url").or_insert("https://api.github.com");
    Settings::read(|key| Ok(values.get(key).map(|v| (*v).to_string())))
}

fn github<'a>(fake: &'a Fake, values: &[(&str, &str)]) -> GitHub<&'a Fake, FixedClock, &'a Fake> {
    GitHub::new(
        fake,
        FixedClock,
        fake,
        settings(values).expect("valid settings"),
    )
}

fn pr_json(title: &str, draft: bool) -> serde_json::Value {
    json!({
        "number": 7, "node_id": "PR_kwDO7", "html_url": "https://github.com/o/r/pull/7",
        "title": title, "body": "b", "state": "open", "draft": draft, "merged_at": null,
        "head": {"ref": "factory/task/DF-1", "sha": "abc"},
        "base": {"ref": "main", "sha": "def"},
    })
}

fn pr_ref() -> PullRequestRef {
    PullRequestRef {
        repo: RepoRef {
            owner: "o".into(),
            name: "r".into(),
        },
        number: 7,
    }
}

/// URLs of every request sent.
fn urls(fake: &Fake) -> Vec<String> {
    fake.requests
        .borrow()
        .iter()
        .map(|r| r.url.clone())
        .collect()
}

fn header(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

// Scenario "github.com".
#[test]
fn github_com_reads_from_the_api_host_and_sends_graphql_there() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    let github = github(&fake, &[("base-url", "https://api.github.com")]);
    github.get(&pr_ref())?;
    assert_eq!(urls(&fake), [PR_URL]);
    assert_eq!(github.settings.graphql_url, GRAPHQL_URL);
    Ok(())
}

// Scenario "GitHub Enterprise Server".
#[test]
fn enterprise_server_keeps_the_api_path_and_sends_graphql_to_api_graphql() -> anyhow::Result<()> {
    for base_url in [
        "https://ghe.example.org/api/v3",
        "https://ghe.example.org/api/v3/",
    ] {
        let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
        let github = github(&fake, &[("base-url", base_url)]);
        github.get(&pr_ref())?;
        assert_eq!(
            urls(&fake),
            ["https://ghe.example.org/api/v3/repos/o/r/pulls/7"]
        );
        assert_eq!(
            github.settings.graphql_url,
            "https://ghe.example.org/api/graphql"
        );
    }
    Ok(())
}

// Scenario "Web host as base URL".
#[test]
fn web_host_as_base_url_is_a_config_error_naming_the_api_url() {
    for base_url in ["https://github.com", "https://GitHub.com/"] {
        let err = settings(&[("base-url", base_url)]).unwrap_err();
        assert!(
            matches!(&err, Error::Config(m) if m.contains("apiUrl: https://api.github.com")),
            "{base_url}: {err:?}"
        );
    }
}

#[test]
fn missing_or_schemeless_base_url_is_a_config_error() {
    let err = Settings::read(|_| Ok(None)).unwrap_err();
    assert!(
        matches!(&err, Error::Config(m) if m.contains("base-url")),
        "{err:?}"
    );
    let err = settings(&[("base-url", "api.github.com")]).unwrap_err();
    assert!(
        matches!(&err, Error::Config(m) if m.contains("base-url")),
        "{err:?}"
    );
}

// Scenario "Unknown draft mode".
#[test]
fn unknown_draft_mode_is_a_config_error_naming_the_value() {
    let err = settings(&[("draft-mode", "detect")]).unwrap_err();
    assert!(
        matches!(&err, Error::Config(m) if m.contains("`detect`")),
        "{err:?}"
    );
}

#[test]
fn draft_mode_defaults_to_title_prefix() -> anyhow::Result<()> {
    assert_eq!(settings(&[])?.draft_mode, DraftMode::TitlePrefix);
    Ok(())
}

#[test]
fn draft_mode_accepts_native_title_prefix_and_auto() -> anyhow::Result<()> {
    let native = settings(&[("draft-mode", "native")])?;
    assert_eq!(native.draft_mode, DraftMode::Native);
    let prefix = settings(&[("draft-mode", "title-prefix")])?;
    assert_eq!(prefix.draft_mode, DraftMode::TitlePrefix);
    assert_eq!(
        settings(&[("draft-mode", "auto")])?.draft_mode,
        DraftMode::Auto
    );
    Ok(())
}

#[test]
fn wip_prefixes_default_to_wip_and_bracketed_wip() -> anyhow::Result<()> {
    assert_eq!(settings(&[])?.prefixes, ["WIP:", "[WIP]"]);
    assert_eq!(
        settings(&[("wip-prefixes", " Draft: , ")])?.prefixes,
        ["Draft:"]
    );
    Ok(())
}

#[test]
fn empty_wip_prefixes_in_title_prefix_mode_is_a_config_error() {
    for value in ["", ",", " , "] {
        let err = settings(&[("wip-prefixes", value)]).unwrap_err();
        assert!(
            matches!(&err, Error::Config(m) if m.contains("wip-prefixes")),
            "{value:?}: {err:?}"
        );
    }
}

#[test]
fn empty_wip_prefixes_in_auto_mode_is_a_config_error() {
    let err = settings(&[("draft-mode", "auto"), ("wip-prefixes", ",")]).unwrap_err();
    assert!(
        matches!(&err, Error::Config(m) if m.contains("wip-prefixes")),
        "{err:?}"
    );
}

#[test]
fn empty_wip_prefixes_in_native_mode_is_allowed() -> anyhow::Result<()> {
    let native = settings(&[("draft-mode", "native"), ("wip-prefixes", ",")])?;
    assert!(native.prefixes.is_empty());
    Ok(())
}

// Scenario "Headers on every request".
#[test]
fn every_request_carries_the_github_headers_and_no_authorization() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", false)),
        reply(200, json!({"data": {}})),
    ]);
    let github = github(&fake, &[]);
    github.get(&pr_ref())?;
    github.send(
        Method::Post,
        "https://api.github.com/graphql".into(),
        Some(json!({})),
    )?;
    let requests = fake.requests.borrow();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert_eq!(
            header(request, "user-agent").as_deref(),
            Some(concat!(
                "dark-factory-github-plugin/",
                env!("CARGO_PKG_VERSION")
            ))
        );
        assert_eq!(
            header(request, "accept").as_deref(),
            Some("application/vnd.github+json")
        );
        assert_eq!(
            header(request, "x-github-api-version").as_deref(),
            Some("2026-03-10")
        );
        assert_eq!(header(request, "authorization"), None);
        assert_eq!(
            header(request, "content-type").as_deref(),
            request.body.as_ref().map(|_| "application/json")
        );
    }
    assert!(requests[1].body.is_some());
    Ok(())
}

// Scenario "API version override".
#[test]
fn api_version_setting_overrides_the_version_header() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    github(&fake, &[("api-version", "2022-11-28")]).get(&pr_ref())?;
    assert_eq!(
        header(&fake.requests.borrow()[0], "x-github-api-version").as_deref(),
        Some("2022-11-28")
    );
    Ok(())
}

#[test]
fn get_maps_a_pull_request() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    let pr = github(&fake, &[]).get(&pr_ref())?;
    assert_eq!(pr.id.number, 7);
    assert_eq!(pr.url, "https://github.com/o/r/pull/7");
    assert_eq!(
        (pr.head.as_str(), pr.base.as_str()),
        ("factory/task/DF-1", "main")
    );
    assert_eq!(
        (pr.title.as_str(), pr.body.as_str()),
        ("DF-1: Fix login", "b")
    );
    assert_eq!(pr.head_sha, "abc");
    assert!(!pr.draft);
    assert!(matches!(pr.state, PullRequestState::Open));
    Ok(())
}

#[test]
fn native_draft_flag_and_title_prefix_both_read_as_draft() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", true)),
        reply(200, pr_json("[WIP] DF-1: Fix login", false)),
    ]);
    let github = github(&fake, &[]);
    for _ in 0..2 {
        let pr = github.get(&pr_ref())?;
        assert!(pr.draft);
        assert_eq!(pr.title, "DF-1: Fix login");
    }
    Ok(())
}

// Scenario "Merged pull request".
#[test]
fn merge_time_makes_a_closed_pull_request_merged() -> anyhow::Result<()> {
    let mut merged = pr_json("Fix", false);
    merged["state"] = json!("closed");
    merged["merged_at"] = json!("2026-10-30T10:00:00Z");
    let mut closed = pr_json("Fix", false);
    closed["state"] = json!("closed");
    let fake = fake(vec![reply(200, merged), reply(200, closed)]);
    let github = github(&fake, &[]);
    assert!(matches!(
        github.get(&pr_ref())?.state,
        PullRequestState::Merged
    ));
    assert!(matches!(
        github.get(&pr_ref())?.state,
        PullRequestState::Closed
    ));
    Ok(())
}

// Scenario "Empty body".
#[test]
fn null_body_is_returned_as_empty() -> anyhow::Result<()> {
    let mut pr = pr_json("Fix", false);
    pr["body"] = serde_json::Value::Null;
    let fake = fake(vec![reply(200, pr)]);
    assert_eq!(github(&fake, &[]).get(&pr_ref())?.body, "");
    Ok(())
}

#[test]
fn non_success_status_is_upstream_with_githubs_message() {
    let fake = fake(vec![reply(502, json!({"message": "Server Error"}))]);
    let err = github(&fake, &[]).get(&pr_ref()).unwrap_err();
    assert!(
        matches!(&err, Error::Upstream(UpstreamError { status: 502, message }) if message == "Server Error"),
        "{err:?}"
    );
}

#[test]
fn bad_token_is_unauthorized() {
    let fake = fake(vec![reply(401, json!({"message": "Bad credentials"}))]);
    let err = github(&fake, &[]).get(&pr_ref()).unwrap_err();
    assert!(
        matches!(&err, Error::Unauthorized(m) if m == "Bad credentials"),
        "{err:?}"
    );
}

fn new_pr(draft: bool) -> NewPullRequest {
    NewPullRequest {
        repo: pr_ref().repo,
        head: "factory/task/DF-1".into(),
        base: "main".into(),
        title: "DF-1: Fix login".into(),
        body: "b".into(),
        draft,
    }
}

/// JSON body of the `index`th request.
fn sent_json(fake: &Fake, index: usize) -> serde_json::Value {
    let requests = fake.requests.borrow();
    serde_json::from_slice(requests[index].body.as_deref().expect("a request body"))
        .expect("JSON body")
}

// Scenario "Draft with title prefix".
#[test]
fn draft_in_title_prefix_mode_prefixes_the_title_without_draft_flag() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, pr_json("WIP: DF-1: Fix login", false))]);
    let pr = github(&fake, &[]).create(&new_pr(true))?;
    assert_eq!(urls(&fake), ["https://api.github.com/repos/o/r/pulls"]);
    assert_eq!(fake.requests.borrow()[0].method, Method::Post);
    assert_eq!(
        sent_json(&fake, 0),
        json!({"title": "WIP: DF-1: Fix login", "head": "factory/task/DF-1",
               "base": "main", "body": "b"})
    );
    assert!(pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login");
    Ok(())
}

// Scenario "Custom prefix".
#[test]
fn custom_prefix_marks_new_drafts() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, pr_json("Draft: DF-1: Fix login", false))]);
    let pr = github(&fake, &[("wip-prefixes", "Draft:")]).create(&new_pr(true))?;
    assert_eq!(sent_json(&fake, 0)["title"], "Draft: DF-1: Fix login");
    assert!(pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login");
    Ok(())
}

#[test]
fn ready_pull_request_keeps_its_title() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, pr_json("DF-1: Fix login", false))]);
    let pr = github(&fake, &[]).create(&new_pr(false))?;
    let body = sent_json(&fake, 0);
    assert_eq!(body["title"], "DF-1: Fix login");
    assert_eq!(body.get("draft"), None);
    assert!(!pr.draft);
    Ok(())
}

// Scenario "Native draft".
#[test]
fn native_draft_sends_the_draft_flag_and_unchanged_title() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, pr_json("DF-1: Fix login", true))]);
    let pr = github(&fake, &[("draft-mode", "native")]).create(&new_pr(true))?;
    let body = sent_json(&fake, 0);
    assert_eq!(body["title"], "DF-1: Fix login");
    assert_eq!(body["draft"], true);
    assert!(pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login");
    Ok(())
}

// Scenario "Duplicate".
#[test]
fn existing_pull_request_is_a_conflict() {
    let fake = fake(vec![reply(
        422,
        json!({"message": "Validation Failed", "errors": [
            {"message": "A pull request already exists for o:factory/task/DF-1."}]}),
    )]);
    let err = github(&fake, &[]).create(&new_pr(true)).unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err:?}");
}

// Scenario "Drafts unsupported".
#[test]
fn unsupported_native_drafts_point_to_title_prefix_mode() {
    let fake = fake(vec![reply(
        422,
        json!({"message": "Validation Failed", "errors": [
            {"message": "Draft pull requests are not supported in this repository."}]}),
    )]);
    let err = github(&fake, &[("draft-mode", "native")])
        .create(&new_pr(true))
        .unwrap_err();
    assert!(
        matches!(&err, Error::InvalidInput(m)
            if m.contains("not supported") && m.contains("draft-mode: title-prefix")),
        "{err:?}"
    );
}

// Scenario "Auto mode with draft support".
#[test]
fn auto_draft_sends_the_draft_flag_and_unchanged_title_once() -> anyhow::Result<()> {
    let fake = fake(vec![reply(201, pr_json("DF-1: Fix login", true))]);
    let pr = github(&fake, &[("draft-mode", "auto")]).create(&new_pr(true))?;
    assert_eq!(fake.requests.borrow().len(), 1);
    let body = sent_json(&fake, 0);
    assert_eq!(body["title"], "DF-1: Fix login");
    assert_eq!(body["draft"], true);
    assert!(pr.draft);
    assert!(fake.warnings.borrow().is_empty());
    Ok(())
}

// Scenario "Auto mode without draft support".
#[test]
fn auto_draft_without_draft_support_retries_once_with_the_title_prefix_and_warns()
-> anyhow::Result<()> {
    let fake = fake(vec![
        reply(
            422,
            json!({"message": "Validation Failed", "errors": [
                {"message": "Draft pull requests are not supported in this repository."}]}),
        ),
        reply(201, pr_json("WIP: DF-1: Fix login", false)),
    ]);
    let pr = github(&fake, &[("draft-mode", "auto")]).create(&new_pr(true))?;
    let pulls = "https://api.github.com/repos/o/r/pulls".to_string();
    assert_eq!(
        calls(&fake),
        [(Method::Post, pulls.clone()), (Method::Post, pulls)]
    );
    let retry = sent_json(&fake, 1);
    assert_eq!(retry["title"], "WIP: DF-1: Fix login");
    assert_eq!(retry.get("draft"), None);
    assert!(pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login");
    let warnings = fake.warnings.borrow();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("o/r")
            && warnings[0].contains("title prefix")
            && warnings[0].contains("draft-mode: title-prefix"),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn auto_draft_passes_other_validation_errors_through_after_one_request() {
    let fake = fake(vec![reply(
        422,
        json!({"message": "Validation Failed", "errors": [
            {"message": "A pull request already exists for o:factory/task/DF-1."}]}),
    )]);
    let err = github(&fake, &[("draft-mode", "auto")])
        .create(&new_pr(true))
        .unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err:?}");
    assert_eq!(fake.requests.borrow().len(), 1);
    assert!(fake.warnings.borrow().is_empty());
}

fn find_open(github: &GitHub<&Fake, FixedClock, &Fake>) -> Result<Option<PullRequest>, Error> {
    github.find_open(&pr_ref().repo, "factory/task/DF-1", "main")
}

// Scenario "Recover after crash".
#[test]
fn find_open_returns_the_matching_pull_request_after_one_request() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, json!([pr_json("DF-1: Fix login", false)]))]);
    let pr = find_open(&github(&fake, &[]))?.expect("an open pull request");
    assert_eq!(
        urls(&fake),
        [
            "https://api.github.com/repos/o/r/pulls?state=open&head=o%3Afactory%2Ftask%2FDF-1&base=main&per_page=100"
        ]
    );
    assert_eq!(fake.requests.borrow()[0].method, Method::Get);
    assert_eq!(pr.id.number, 7);
    assert_eq!(pr.url, "https://github.com/o/r/pull/7");
    assert_eq!(pr.head_sha, "abc");
    Ok(())
}

// Scenario "No open pull request".
#[test]
fn find_open_without_match_is_none() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, json!([]))]);
    assert!(find_open(&github(&fake, &[]))?.is_none());
    Ok(())
}

#[test]
fn find_open_skips_entries_with_another_head_or_base() -> anyhow::Result<()> {
    let mut other_head = pr_json("Other", false);
    other_head["number"] = json!(5);
    other_head["head"]["ref"] = json!("factory/task/DF-10");
    let mut other_base = pr_json("Other", false);
    other_base["number"] = json!(6);
    other_base["base"]["ref"] = json!("release");
    let fake = fake(vec![reply(
        200,
        json!([other_head, other_base, pr_json("DF-1: Fix login", false)]),
    )]);
    let pr = find_open(&github(&fake, &[]))?.expect("an open pull request");
    assert_eq!(pr.id.number, 7);
    Ok(())
}

#[test]
fn rate_limit_wait_is_measured_from_the_clock() {
    let fake = fake(vec![Response {
        ratelimit_remaining: Some(0),
        ratelimit_reset: Some(FixedClock.now() + 600),
        ..reply(403, json!({"message": "API rate limit exceeded"}))
    }]);
    let err = github(&fake, &[]).get(&pr_ref()).unwrap_err();
    assert!(
        matches!(
            &err,
            Error::RateLimited(RateLimit {
                retry_after_seconds: Some(600),
                ..
            })
        ),
        "{err:?}"
    );
}

fn issue_ref(number: u64) -> IssueRef {
    IssueRef {
        repo: RepoRef {
            owner: "o".into(),
            name: "r".into(),
        },
        number,
    }
}

fn issue_json(body: serde_json::Value) -> serde_json::Value {
    json!({
        "number": 3, "html_url": "https://github.com/o/r/issues/3", "title": "Login fails",
        "body": body, "state": "closed",
        "labels": [{"id": 1, "name": "bug"}, {"id": 2, "name": "ui"}],
    })
}

#[test]
fn get_issue_maps_title_body_state_labels_and_url() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, issue_json(json!("Steps")))]);
    let issue = github(&fake, &[]).get_issue(&issue_ref(3))?;
    let requests = fake.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::Get);
    assert_eq!(requests[0].url, "https://api.github.com/repos/o/r/issues/3");
    assert_eq!(issue.id.number, 3);
    assert_eq!(issue.url, "https://github.com/o/r/issues/3");
    assert_eq!(issue.title, "Login fails");
    assert_eq!(issue.body, "Steps");
    assert_eq!(issue.state, IssueState::Closed);
    assert_eq!(issue.labels, ["bug", "ui"]);
    Ok(())
}

// Scenario "Issue without body".
#[test]
fn issue_without_body_has_an_empty_body() -> anyhow::Result<()> {
    let mut json = issue_json(json!(null));
    json["state"] = json!("open");
    let fake = fake(vec![reply(200, json)]);
    let issue = github(&fake, &[]).get_issue(&issue_ref(3))?;
    assert_eq!(issue.body, "");
    assert_eq!(issue.state, IssueState::Open);
    Ok(())
}

// Scenario "Comment on a pull request".
#[test]
fn comment_on_a_pull_request_goes_through_the_issues_endpoint() -> anyhow::Result<()> {
    let fake = fake(vec![reply(
        201,
        json!({"id": 99, "html_url": "https://github.com/o/r/pull/7#issuecomment-99"}),
    )]);
    let comment = github(&fake, &[]).add_comment(&issue_ref(7), "**Done**")?;
    let requests = fake.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::Post);
    assert_eq!(
        requests[0].url,
        "https://api.github.com/repos/o/r/issues/7/comments"
    );
    let body: serde_json::Value = serde_json::from_slice(requests[0].body.as_deref().unwrap())?;
    assert_eq!(body, json!({"body": "**Done**"}));
    assert_eq!(comment.id, "99");
    assert_eq!(comment.url, "https://github.com/o/r/pull/7#issuecomment-99");
    Ok(())
}

#[test]
fn missing_issue_is_not_found() {
    let fake = fake(vec![reply(404, json!({"message": "Not Found"}))]);
    let err = github(&fake, &[]).get_issue(&issue_ref(3)).unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

fn changes(title: Option<&str>, body: Option<&str>, draft: Option<bool>) -> PullRequestUpdate {
    PullRequestUpdate {
        title: title.map(str::to_string),
        body: body.map(str::to_string),
        draft,
    }
}

/// Method and URL of every request sent.
fn calls(fake: &Fake) -> Vec<(Method, String)> {
    fake.requests
        .borrow()
        .iter()
        .map(|r| (r.method, r.url.clone()))
        .collect()
}

fn graphql_ok() -> Response {
    reply(
        200,
        json!({"data": {"mutation": {"clientMutationId": null}}}),
    )
}

#[test]
fn body_only_update_patches_the_body_without_reading_first() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    github(&fake, &[]).update(&pr_ref(), &changes(None, Some("new"), None))?;
    assert_eq!(calls(&fake), [(Method::Patch, PR_URL.to_string())]);
    assert_eq!(sent_json(&fake, 0), json!({"body": "new"}));
    Ok(())
}

#[test]
fn owner_and_name_are_percent_encoded_as_path_segments() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    let id = PullRequestRef {
        repo: RepoRef {
            owner: "a/b".into(),
            name: "x?y".into(),
        },
        number: 7,
    };
    github(&fake, &[]).update(&id, &changes(None, Some("new"), None))?;
    assert_eq!(
        urls(&fake),
        ["https://api.github.com/repos/a%2Fb/x%3Fy/pulls/7"]
    );
    Ok(())
}

#[test]
fn native_draft_without_node_id_is_internal_and_sends_no_graphql() {
    let mut pr = pr_json("DF-1: Fix login", true);
    pr.as_object_mut().expect("object").remove("node_id");
    let fake = fake(vec![reply(200, pr)]);
    let err = github(&fake, &[])
        .update(&pr_ref(), &changes(None, None, Some(false)))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Internal(m) if m.contains("node_id")),
        "{err:?}"
    );
    assert_eq!(calls(&fake), [(Method::Get, PR_URL.to_string())]);
}

// Scenario "Ready for review with title prefix".
#[test]
fn ready_for_review_strips_the_title_prefix() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("WIP: DF-1: Fix login", false)),
        reply(200, pr_json("DF-1: Fix login", false)),
    ]);
    let pr = github(&fake, &[]).update(&pr_ref(), &changes(None, None, Some(false)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert_eq!(sent_json(&fake, 1), json!({"title": "DF-1: Fix login"}));
    assert!(!pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login");
    Ok(())
}

// Scenario "Ready for review with native draft".
#[test]
fn ready_for_review_marks_a_native_draft_ready_through_graphql() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", true)),
        graphql_ok(),
        reply(200, pr_json("DF-1: Fix login", false)),
    ]);
    let pr = github(&fake, &[]).update(&pr_ref(), &changes(None, None, Some(false)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Post, GRAPHQL_URL.to_string()),
            (Method::Get, PR_URL.to_string())
        ]
    );
    let sent = sent_json(&fake, 1);
    assert!(
        sent["query"]
            .as_str()
            .is_some_and(|q| q.contains("markPullRequestReadyForReview")),
        "{sent}"
    );
    assert_eq!(sent["variables"], json!({"id": "PR_kwDO7"}));
    assert!(!pr.draft);
    Ok(())
}

#[test]
fn ready_and_retitled_native_draft_returns_the_patch_response_without_rereading()
-> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", true)),
        graphql_ok(),
        reply(200, pr_json("DF-1: Fix login redirect", false)),
    ]);
    let pr = github(&fake, &[("draft-mode", "native")]).update(
        &pr_ref(),
        &changes(Some("DF-1: Fix login redirect"), None, Some(false)),
    )?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Post, GRAPHQL_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert!(
        sent_json(&fake, 1)["query"]
            .as_str()
            .is_some_and(|q| q.contains("markPullRequestReadyForReview"))
    );
    assert_eq!(
        sent_json(&fake, 2),
        json!({"title": "DF-1: Fix login redirect"})
    );
    assert!(!pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login redirect");
    Ok(())
}

#[test]
fn draft_request_for_an_existing_draft_only_reads_it() -> anyhow::Result<()> {
    for (mode, title, native) in [
        ("native", "DF-1: Fix login", true),
        ("native", "WIP: DF-1: Fix login", false),
        ("title-prefix", "DF-1: Fix login", true),
        ("title-prefix", "WIP: DF-1: Fix login", false),
    ] {
        let fake = fake(vec![reply(200, pr_json(title, native))]);
        let pr = github(&fake, &[("draft-mode", mode)])
            .update(&pr_ref(), &changes(None, None, Some(true)))?;
        assert_eq!(
            calls(&fake),
            [(Method::Get, PR_URL.to_string())],
            "{mode}: {title}"
        );
        assert!(pr.draft, "{mode}: {title}");
    }
    Ok(())
}

#[test]
fn draft_request_for_a_ready_pull_request_in_auto_mode_adds_the_prefix() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", false)),
        reply(200, pr_json("WIP: DF-1: Fix login", false)),
    ]);
    let pr = github(&fake, &[("draft-mode", "auto")])
        .update(&pr_ref(), &changes(None, None, Some(true)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert_eq!(
        sent_json(&fake, 1),
        json!({"title": "WIP: DF-1: Fix login"})
    );
    assert!(pr.draft);
    Ok(())
}

// Scenario "Mode switched with an open draft".
#[test]
fn ready_for_review_in_native_mode_strips_the_prefix_without_graphql() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("WIP: DF-1: Fix login", false)),
        reply(200, pr_json("DF-1: Fix login", false)),
    ]);
    let github = github(&fake, &[("draft-mode", "native")]);
    let pr = github.update(&pr_ref(), &changes(None, None, Some(false)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert_eq!(sent_json(&fake, 1), json!({"title": "DF-1: Fix login"}));
    assert!(!pr.draft);
    Ok(())
}

// Scenario "Retitle a draft" with a title prefix.
#[test]
fn retitled_prefixed_draft_keeps_the_prefix() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("WIP: DF-1: Fix login", false)),
        reply(200, pr_json("WIP: DF-1: Fix login redirect", false)),
    ]);
    let pr = github(&fake, &[("draft-mode", "native")]).update(
        &pr_ref(),
        &changes(Some("DF-1: Fix login redirect"), None, None),
    )?;
    assert_eq!(
        sent_json(&fake, 1),
        json!({"title": "WIP: DF-1: Fix login redirect"})
    );
    assert_eq!(fake.requests.borrow().len(), 2);
    assert!(pr.draft);
    assert_eq!(pr.title, "DF-1: Fix login redirect");
    Ok(())
}

// Scenario "Retitle a draft" with a native draft.
#[test]
fn retitled_native_draft_gets_a_plain_title() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", true)),
        reply(200, pr_json("DF-1: Fix login redirect", true)),
    ]);
    let pr = github(&fake, &[]).update(
        &pr_ref(),
        &changes(Some("DF-1: Fix login redirect"), None, None),
    )?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert_eq!(
        sent_json(&fake, 1),
        json!({"title": "DF-1: Fix login redirect"})
    );
    assert!(pr.draft);
    Ok(())
}

// Scenario "Already ready".
#[test]
fn ready_pull_request_sends_no_draft_change() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr_json("DF-1: Fix login", false))]);
    let pr = github(&fake, &[("draft-mode", "native")])
        .update(&pr_ref(), &changes(None, None, Some(false)))?;
    assert_eq!(calls(&fake), [(Method::Get, PR_URL.to_string())]);
    assert!(!pr.draft);
    Ok(())
}

#[test]
fn draft_in_title_prefix_mode_adds_the_first_prefix() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", false)),
        reply(200, pr_json("WIP: DF-1: Fix login", false)),
    ]);
    let pr = github(&fake, &[]).update(&pr_ref(), &changes(None, None, Some(true)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Patch, PR_URL.to_string())
        ]
    );
    assert_eq!(
        sent_json(&fake, 1),
        json!({"title": "WIP: DF-1: Fix login"})
    );
    assert!(pr.draft);
    Ok(())
}

#[test]
fn draft_in_native_mode_converts_through_graphql() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", false)),
        graphql_ok(),
        reply(200, pr_json("DF-1: Fix login", true)),
    ]);
    let pr = github(&fake, &[("draft-mode", "native")])
        .update(&pr_ref(), &changes(None, None, Some(true)))?;
    assert_eq!(
        calls(&fake),
        [
            (Method::Get, PR_URL.to_string()),
            (Method::Post, GRAPHQL_URL.to_string()),
            (Method::Get, PR_URL.to_string())
        ]
    );
    let sent = sent_json(&fake, 1);
    assert!(
        sent["query"]
            .as_str()
            .is_some_and(|q| q.contains("convertPullRequestToDraft")),
        "{sent}"
    );
    assert!(pr.draft);
    Ok(())
}

// Scenario "GraphQL error".
#[test]
fn graphql_not_found_is_not_found() {
    let fake = fake(vec![
        reply(200, pr_json("DF-1: Fix login", true)),
        reply(
            200,
            json!({"errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a node"}]}),
        ),
    ]);
    let err = github(&fake, &[])
        .update(&pr_ref(), &changes(None, None, Some(false)))
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}
