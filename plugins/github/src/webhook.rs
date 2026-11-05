//! `handle-webhook`: maps GitHub webhook deliveries to the normalized events. The host checks the
//! signature; this module extracts it and parses the payload, without network access.

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::ReviewVerdict;
use crate::exports::darkfactory::integrations::webhooks::{
    CiFailed, CommentCreated, ParsedEvent, PullRequestClosed, ReviewSubmitted, WebhookEvent,
};

const EVENT_HEADER: &str = "x-github-event";
const SIGNATURE_HEADER: &str = "x-hub-signature-256";

/// The hex-decoded `X-Hub-Signature-256` after its `sha256=` prefix, `None` if the header or the
/// prefix is missing or the rest is not hex. The SHA-1 `X-Hub-Signature` is ignored.
pub(crate) fn signature(headers: &[(String, String)]) -> Option<Vec<u8>> {
    let hex = header(headers, SIGNATURE_HEADER)?
        .trim()
        .strip_prefix("sha256=")?;
    hex::decode(hex)
        .ok()
        .filter(|signature| !signature.is_empty())
}

/// The events of one delivery: none for events and actions the factory doesn't react to.
pub(crate) fn parse(headers: &[(String, String)], body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let event = header(headers, EVENT_HEADER)
        .ok_or_else(|| Error::InvalidInput("missing `X-GitHub-Event` header".to_string()))?;
    match event.trim() {
        "pull_request_review" => review(body),
        "pull_request" => closed(body),
        "check_suite" => check_suite(body),
        "status" => status(body),
        "issue_comment" => comment(body),
        _ => Ok(Vec::new()),
    }
}

fn review(body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let p: ReviewPayload = payload(body)?;
    if p.action != "submitted" {
        return Ok(Vec::new());
    }
    let verdict = match p.review.state.to_ascii_lowercase().as_str() {
        "approved" => ReviewVerdict::Approved,
        "changes_requested" => ReviewVerdict::ChangesRequested,
        "commented" => ReviewVerdict::Commented,
        _ => return Ok(Vec::new()),
    };
    let Some(head) = p.pull_request.head.of_repository(p.repository.id) else {
        return Ok(Vec::new());
    };
    let (repo_url, repo) = p.repository.into_parts();
    Ok(vec![ParsedEvent {
        repo_url,
        event: WebhookEvent::ReviewSubmitted(ReviewSubmitted {
            pr: PullRequestRef {
                repo,
                number: p.pull_request.number,
            },
            head,
            actor: p.sender.login,
            verdict,
        }),
    }])
}

fn closed(body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let p: PullRequestPayload = payload(body)?;
    if p.action != "closed" {
        return Ok(Vec::new());
    }
    let Some(head) = p.pull_request.head.of_repository(p.repository.id) else {
        return Ok(Vec::new());
    };
    let (repo_url, repo) = p.repository.into_parts();
    Ok(vec![ParsedEvent {
        repo_url,
        event: WebhookEvent::PullRequestClosed(PullRequestClosed {
            pr: PullRequestRef {
                repo,
                number: p.pull_request.number,
            },
            head,
            actor: p.sender.login,
            merged: p.pull_request.merged.unwrap_or_default(),
        }),
    }])
}

/// A completed suite that failed or timed out; a suite without head branch is skipped, which
/// includes every fork (GitHub sends `head_branch: null` for a push to a fork).
fn check_suite(body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let p: CheckSuitePayload = payload(body)?;
    let failed = matches!(
        p.check_suite.conclusion.as_deref(),
        Some("failure" | "timed_out")
    );
    let (true, "completed", Some(head)) = (failed, p.action.as_str(), p.check_suite.head_branch)
    else {
        return Ok(Vec::new());
    };
    let (repo_url, repo) = p.repository.into_parts();
    Ok(vec![ParsedEvent {
        repo_url,
        event: WebhookEvent::CiFailed(CiFailed {
            repo,
            head,
            check: p.check_suite.app.map(|a| a.name).unwrap_or_default(),
            // A check suite has no web URL of its own.
            url: String::new(),
        }),
    }])
}

/// A failed or errored commit status: one event per branch whose head is the commit.
fn status(body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let p: StatusPayload = payload(body)?;
    if !matches!(p.state.as_str(), "failure" | "error") {
        return Ok(Vec::new());
    }
    let (repo_url, repo) = p.repository.into_parts();
    Ok(p.branches
        .into_iter()
        .map(|branch| ParsedEvent {
            repo_url: repo_url.clone(),
            event: WebhookEvent::CiFailed(CiFailed {
                repo: repo.clone(),
                head: branch.name,
                check: p.context.clone(),
                url: p.target_url.clone().unwrap_or_default(),
            }),
        })
        .collect())
}

fn comment(body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let p: CommentPayload = payload(body)?;
    if p.action != "created" {
        return Ok(Vec::new());
    }
    let (repo_url, repo) = p.repository.into_parts();
    Ok(vec![ParsedEvent {
        repo_url,
        event: WebhookEvent::CommentCreated(CommentCreated {
            issue: IssueRef {
                repo,
                number: p.issue.number,
            },
            is_pull_request: p.issue.pull_request.is_some(),
            actor: p.sender.login,
            mentions: mentions(&p.comment.body),
        }),
    }])
}

/// Logins mentioned as `@login` outside code spans, in order of first appearance. A mention starts
/// the text or follows whitespace or an opening bracket, so emails are none; `@org/team` is a team.
fn mentions(text: &str) -> Vec<String> {
    let prose = without_code(text);
    let mut found: Vec<String> = Vec::new();
    let mut previous = None;
    for (i, c) in prose.char_indices() {
        let starts_mention = previous.is_none_or(|p: char| p.is_whitespace() || "([{".contains(p));
        previous = Some(c);
        if c != '@' || !starts_mention {
            continue;
        }
        let rest = prose.get(i + 1..).unwrap_or_default();
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(rest.len());
        let (login, after) = rest.split_at(end);
        let login = login.trim_end_matches('-');
        if login.is_empty() || after.starts_with('/') {
            continue;
        }
        if !found.iter().any(|f| f.eq_ignore_ascii_case(login)) {
            found.push(login.to_string());
        }
    }
    found
}

/// `text` with every code span and fenced block replaced by a space.
fn without_code(text: &str) -> String {
    let mut prose = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let (before, from) = rest.split_at(start);
        prose.push_str(before);
        let ticks = from.len() - from.trim_start_matches('`').len();
        let (fence, after) = from.split_at(ticks);
        if let Some(end) = after.find(fence) {
            prose.push(' ');
            rest = after.split_at(end + ticks).1;
        } else {
            // An unclosed run of backticks is literal text.
            prose.push_str(fence);
            rest = after;
        }
    }
    prose.push_str(rest);
    prose
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn payload<P: DeserializeOwned>(body: &[u8]) -> Result<P, Error> {
    serde_json::from_slice(body)
        .map_err(|e| Error::InvalidInput(format!("malformed GitHub webhook payload: {e}")))
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Repository {
    id: u64,
    html_url: String,
    name: String,
    owner: User,
}

impl Repository {
    fn into_parts(self) -> (String, RepoRef) {
        (
            self.html_url,
            RepoRef {
                owner: self.owner.login,
                name: self.name,
            },
        )
    }
}

#[derive(Deserialize)]
struct RepositoryId {
    id: u64,
}

#[derive(Deserialize)]
struct Branch {
    #[serde(rename = "ref")]
    branch: String,
    /// `null` once a fork is deleted.
    repo: Option<RepositoryId>,
}

impl Branch {
    /// The branch name, `None` for a branch of another repository than `base`: a fork's branch can
    /// carry a task branch name.
    fn of_repository(self, base: u64) -> Option<String> {
        self.repo
            .is_some_and(|r| r.id == base)
            .then_some(self.branch)
    }
}

#[derive(Deserialize)]
struct PullRequest {
    number: u64,
    head: Branch,
    /// `null` in review payloads.
    #[serde(default)]
    merged: Option<bool>,
}

#[derive(Deserialize)]
struct PullRequestPayload {
    action: String,
    pull_request: PullRequest,
    repository: Repository,
    sender: User,
}

#[derive(Deserialize)]
struct Review {
    state: String,
}

#[derive(Deserialize)]
struct ReviewPayload {
    action: String,
    review: Review,
    pull_request: PullRequest,
    repository: Repository,
    sender: User,
}

#[derive(Deserialize)]
struct App {
    name: String,
}

#[derive(Deserialize)]
struct CheckSuite {
    head_branch: Option<String>,
    conclusion: Option<String>,
    app: Option<App>,
}

#[derive(Deserialize)]
struct CheckSuitePayload {
    action: String,
    check_suite: CheckSuite,
    repository: Repository,
}

#[derive(Deserialize)]
struct StatusBranch {
    name: String,
}

#[derive(Deserialize)]
struct StatusPayload {
    state: String,
    context: String,
    target_url: Option<String>,
    #[serde(default)]
    branches: Vec<StatusBranch>,
    repository: Repository,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    /// Present for a pull request.
    pull_request: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Comment {
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct CommentPayload {
    action: String,
    issue: Issue,
    comment: Comment,
    repository: Repository,
    sender: User,
}

#[cfg(test)]
mod tests;
