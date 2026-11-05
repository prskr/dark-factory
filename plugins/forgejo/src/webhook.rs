//! `handle-webhook`: maps Forgejo webhook deliveries to the normalized events. The host checks the
//! signature; this module extracts it and parses the payload, without network access.

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::ReviewVerdict;
use crate::exports::darkfactory::integrations::webhooks::{
    CiFailed, CommentCreated, ParsedEvent, PullRequestClosed, ReviewSubmitted, WebhookEvent,
};

const EVENT_HEADER: &str = "x-forgejo-event";
const SIGNATURE_HEADER: &str = "x-forgejo-signature";

/// The hex-decoded `X-Forgejo-Signature`, `None` if the header is missing or not hex.
pub(crate) fn signature(headers: &[(String, String)]) -> Option<Vec<u8>> {
    hex::decode(header(headers, SIGNATURE_HEADER)?.trim())
        .ok()
        .filter(|signature| !signature.is_empty())
}

/// The events of one delivery: none for events the factory doesn't react to.
pub(crate) fn parse(headers: &[(String, String)], body: &[u8]) -> Result<Vec<ParsedEvent>, Error> {
    let event = header(headers, EVENT_HEADER)
        .ok_or_else(|| Error::InvalidInput("missing `X-Forgejo-Event` header".to_string()))?;
    let parsed = match event.trim() {
        "pull_request_approved" => review(body, ReviewVerdict::Approved)?,
        "pull_request_rejected" => review(body, ReviewVerdict::ChangesRequested)?,
        // A single line comment arrives as a comment review without body.
        "pull_request_comment" => review(body, ReviewVerdict::Commented)?,
        "pull_request" => closed(body)?,
        "issue_comment" => comment(body)?,
        "action_run_failure" => ci_failed(body)?,
        _ => None,
    };
    Ok(parsed.into_iter().collect())
}

fn review(body: &[u8], verdict: ReviewVerdict) -> Result<Option<ParsedEvent>, Error> {
    let p: PullRequestPayload = payload(body)?;
    if p.action != "reviewed" {
        return Ok(None);
    }
    let Some(head) = p.pull_request.head.of_repository(p.repository.id) else {
        return Ok(None);
    };
    let (repo_url, repo) = p.repository.into_parts();
    Ok(Some(ParsedEvent {
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
    }))
}

fn closed(body: &[u8]) -> Result<Option<ParsedEvent>, Error> {
    let p: PullRequestPayload = payload(body)?;
    if p.action != "closed" {
        return Ok(None);
    }
    let Some(head) = p.pull_request.head.of_repository(p.repository.id) else {
        return Ok(None);
    };
    let (repo_url, repo) = p.repository.into_parts();
    Ok(Some(ParsedEvent {
        repo_url,
        event: WebhookEvent::PullRequestClosed(PullRequestClosed {
            pr: PullRequestRef {
                repo,
                number: p.pull_request.number,
            },
            head,
            actor: p.sender.login,
            merged: p.pull_request.merged,
        }),
    }))
}

fn comment(body: &[u8]) -> Result<Option<ParsedEvent>, Error> {
    let p: CommentPayload = payload(body)?;
    if p.action != "created" {
        return Ok(None);
    }
    let (repo_url, repo) = p.repository.into_parts();
    Ok(Some(ParsedEvent {
        repo_url,
        event: WebhookEvent::CommentCreated(CommentCreated {
            issue: IssueRef {
                repo,
                number: p.issue.number,
            },
            is_pull_request: p.is_pull,
            actor: p.sender.login,
            mentions: mentions(&p.comment.body),
        }),
    }))
}

fn ci_failed(body: &[u8]) -> Result<Option<ParsedEvent>, Error> {
    let RunPayload { run } = payload(body)?;
    // A pull request run names `#<index>` as its ref; the branch is in the triggering payload.
    let head = if run.event.starts_with("pull_request") {
        let trigger: PullRequestTrigger = payload(run.event_payload.as_bytes())?;
        let Some(head) = trigger.pull_request.head.of_repository(run.repository.id) else {
            return Ok(None);
        };
        head
    } else {
        run.prettyref
    };
    let (repo_url, repo) = run.repository.into_parts();
    Ok(Some(ParsedEvent {
        repo_url,
        event: WebhookEvent::CiFailed(CiFailed {
            repo,
            head,
            check: run.workflow_id,
            url: run.html_url,
        }),
    }))
}

/// Logins mentioned as `@login` outside code spans, in order of first appearance. Like Forgejo, a
/// mention starts the text or follows whitespace or an opening bracket, so emails are none.
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
            .find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c)))
            .unwrap_or(rest.len());
        let login = rest
            .get(..end)
            .unwrap_or_default()
            .trim_end_matches(['-', '_', '.']);
        if !login.is_empty() && !found.iter().any(|f| f.eq_ignore_ascii_case(login)) {
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
        .map_err(|e| Error::InvalidInput(format!("malformed Forgejo webhook payload: {e}")))
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
    #[serde(default)]
    merged: bool,
}

#[derive(Deserialize)]
struct PullRequestPayload {
    action: String,
    pull_request: PullRequest,
    repository: Repository,
    sender: User,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
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
    #[serde(default)]
    is_pull: bool,
    repository: Repository,
    sender: User,
}

#[derive(Deserialize)]
struct Run {
    event: String,
    prettyref: String,
    workflow_id: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    event_payload: String,
    repository: Repository,
}

#[derive(Deserialize)]
struct RunPayload {
    run: Run,
}

/// The part of a run's `event_payload` naming a pull request's head.
#[derive(Deserialize)]
struct PullRequestTrigger {
    pull_request: TriggerPullRequest,
}

#[derive(Deserialize)]
struct TriggerPullRequest {
    head: Branch,
}

#[cfg(test)]
mod tests;
