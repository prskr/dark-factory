//! GitHub's REST API behind `pull-requests` and `issues`.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;

use crate::darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use crate::draft;
use crate::error;
use crate::exports::darkfactory::integrations::issues::{Comment, Issue, IssueState};
use crate::exports::darkfactory::integrations::pull_requests::{
    NewPullRequest, PullRequest, PullRequestState, PullRequestUpdate,
};
use crate::settings::Settings;

/// `User-Agent` of every request; GitHub rejects requests without one.
const USER_AGENT: &str = concat!("dark-factory-github-plugin/", env!("CARGO_PKG_VERSION"));

/// Query values and path segments keep only RFC 3986 unreserved characters; branch names contain `/`.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// GitHub's message when a repository's plan has no draft pull requests.
const DRAFTS_UNSUPPORTED: &str = "Draft pull requests are not supported";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
}

/// A request to GitHub's API host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// Absolute URL, e.g. `https://api.github.com/repos/o/r/pulls/1`.
    pub url: String,
    /// Never `authorization`: the host injects it.
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Vec<u8>>,
}

/// Status, rate-limit headers and body of a response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// `retry-after` in seconds, if present and numeric.
    pub retry_after: Option<u32>,
    /// `x-ratelimit-remaining`, if present and numeric.
    pub ratelimit_remaining: Option<u64>,
    /// `x-ratelimit-reset` in Unix seconds, if present and numeric.
    pub ratelimit_reset: Option<u64>,
    pub body: Vec<u8>,
}

/// Sends a request to GitHub; the host adds credentials.
pub trait Transport {
    fn send(&self, request: Request) -> Result<Response, Error>;
}

/// The current Unix time in seconds, against which `x-ratelimit-reset` becomes a wait.
pub trait Clock {
    fn now(&self) -> u64;
}

/// Warnings for the operator.
pub trait Log {
    fn warn(&self, message: &str);
}

/// GitHub client of one plugin instance.
pub struct GitHub<T, C, L> {
    transport: T,
    clock: C,
    log: L,
    settings: Settings,
}

impl<T: Transport, C: Clock, L: Log> GitHub<T, C, L> {
    pub fn new(transport: T, clock: C, log: L, settings: Settings) -> Self {
        Self {
            transport,
            clock,
            log,
            settings,
        }
    }

    pub fn get(&self, id: &PullRequestRef) -> Result<PullRequest, Error> {
        let path = format!("{}/pulls/{}", repo_path(&id.repo), id.number);
        let pr: ApiPullRequest = self.rest(Method::Get, &path, None)?;
        Ok(self.pull_request(id.repo.clone(), pr))
    }

    /// Opens a pull request; a draft is marked in the configured draft mode.
    pub fn create(&self, new: &NewPullRequest) -> Result<PullRequest, Error> {
        let mode = self.settings.draft_mode;
        let native = new.draft && mode.creates_native_drafts();
        let pr = match self.post_pull_request(new, native) {
            Err(Error::InvalidInput(m))
                if native && m.contains(DRAFTS_UNSUPPORTED) && mode.falls_back_to_prefix() =>
            {
                self.log.warn(&format!(
                    "{}/{} has no draft pull requests: drafts fall back to the title prefix \
                     (set `draft-mode: title-prefix` to silence this warning)",
                    new.repo.owner, new.repo.name
                ));
                self.post_pull_request(new, false)
            }
            Err(Error::InvalidInput(m)) if native && m.contains(DRAFTS_UNSUPPORTED) => {
                Err(Error::InvalidInput(format!(
                    "{m} (set `draft-mode: title-prefix` for repositories without drafts)"
                )))
            }
            other => other,
        }?;
        Ok(self.pull_request(new.repo.clone(), pr))
    }

    /// The open pull request from `head` into `base`, found with one filtered request.
    pub fn find_open(
        &self,
        repo: &RepoRef,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, Error> {
        let path = format!(
            "{}/pulls?state=open&head={}&base={}&per_page=100",
            repo_path(repo),
            utf8_percent_encode(&format!("{}:{head}", repo.owner), QUERY_VALUE),
            utf8_percent_encode(base, QUERY_VALUE),
        );
        let prs: Vec<ApiPullRequest> = self.rest(Method::Get, &path, None)?;
        Ok(prs
            .into_iter()
            .find(|pr| pr.head.branch == head && pr.base.branch == base)
            .map(|pr| self.pull_request(repo.clone(), pr)))
    }

    /// Changes only the given fields; draft changes follow the draft mode (design D2).
    pub fn update(
        &self,
        id: &PullRequestRef,
        changes: &PullRequestUpdate,
    ) -> Result<PullRequest, Error> {
        let path = format!("{}/pulls/{}", repo_path(&id.repo), id.number);
        let mut patch = serde_json::Map::new();
        let mut current = None;
        let mut mutated = false;
        if changes.title.is_some() || changes.draft.is_some() {
            let pr: ApiPullRequest = self.rest(Method::Get, &path, None)?;
            let (title, mutation) = self.draft_change(&pr, changes);
            if let Some(title) = title {
                patch.insert("title".to_string(), json!(title));
            }
            if let Some(mutation) = mutation {
                self.graphql(mutation, &pr.node_id)?;
                mutated = true;
            }
            current = Some(pr);
        }
        if let Some(body) = &changes.body {
            patch.insert("body".to_string(), json!(body));
        }
        let pr = if !patch.is_empty() {
            self.rest(Method::Patch, &path, Some(serde_json::Value::Object(patch)))?
        } else if let Some(pr) = current.filter(|_| !mutated) {
            pr
        } else {
            self.rest(Method::Get, &path, None)?
        };
        Ok(self.pull_request(id.repo.clone(), pr))
    }

    /// An issue; a pull request number returns the pull request as an issue.
    pub fn get_issue(&self, id: &IssueRef) -> Result<Issue, Error> {
        let path = format!("{}/issues/{}", repo_path(&id.repo), id.number);
        let issue: ApiIssue = self.rest(Method::Get, &path, None)?;
        Ok(Issue {
            id: id.clone(),
            url: issue.html_url,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            state: if issue.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            labels: issue.labels.into_iter().map(|l| l.name).collect(),
        })
    }

    /// Comments on an issue or, by its number, a pull request.
    pub fn add_comment(&self, id: &IssueRef, body: &str) -> Result<Comment, Error> {
        let path = format!("{}/issues/{}/comments", repo_path(&id.repo), id.number);
        let comment: ApiComment = self.rest(Method::Post, &path, Some(json!({ "body": body })))?;
        Ok(Comment {
            id: comment.id.to_string(),
            url: comment.html_url,
        })
    }

    /// Title to send and native draft mutation that turn `current` into the requested state.
    fn draft_change(
        &self,
        current: &ApiPullRequest,
        changes: &PullRequestUpdate,
    ) -> (Option<String>, Option<Mutation>) {
        let (prefixed, plain) = draft::split_title(&current.title, &self.settings.prefixes);
        let is_draft = current.draft || prefixed;
        let (prefix, mutation) = match changes.draft {
            Some(false) => (false, current.draft.then_some(Mutation::MarkReady)),
            Some(true) if !is_draft && self.settings.draft_mode.converts_natively() => {
                (false, Some(Mutation::ConvertToDraft))
            }
            Some(true) if !is_draft => (true, None),
            _ => (prefixed, None),
        };
        let title = (changes.title.is_some() || prefix != prefixed).then(|| {
            draft::apply_title(
                changes.title.as_deref().unwrap_or(&plain),
                prefix,
                &self.settings.prefixes,
            )
        });
        (title, mutation)
    }

    /// Posts `new`; a draft gets GitHub's draft flag if `native`, otherwise the title prefix.
    fn post_pull_request(
        &self,
        new: &NewPullRequest,
        native: bool,
    ) -> Result<ApiPullRequest, Error> {
        let title = if native {
            new.title.clone()
        } else {
            draft::apply_title(&new.title, new.draft, &self.settings.prefixes)
        };
        let mut body = json!({
            "title": title,
            "head": new.head,
            "base": new.base,
            "body": new.body,
        });
        if native && let Some(fields) = body.as_object_mut() {
            fields.insert("draft".to_string(), json!(true));
        }
        let path = format!("{}/pulls", repo_path(&new.repo));
        self.rest(Method::Post, &path, Some(body))
    }

    /// Runs `mutation` on the pull request with the GraphQL id `node_id`.
    fn graphql(&self, mutation: Mutation, node_id: &str) -> Result<(), Error> {
        if node_id.is_empty() {
            return Err(Error::Internal(
                "GitHub returned no `node_id` for the pull request".to_string(),
            ));
        }
        let body = json!({
            "query": mutation.document(),
            "variables": { "id": node_id },
        });
        let response = self.send(Method::Post, self.settings.graphql_url.clone(), Some(body))?;
        if !(200..300).contains(&response.status) {
            return Err(error::from_response(&response, self.clock.now()));
        }
        error::from_graphql(&response.body)
    }

    /// Sends a request to `path` below `base-url` and decodes the JSON response.
    fn rest<R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<R, Error> {
        let url = format!("{}{path}", self.settings.rest_url);
        let response = self.send(method, url, body)?;
        if !(200..300).contains(&response.status) {
            return Err(error::from_response(&response, self.clock.now()));
        }
        serde_json::from_slice(&response.body)
            .map_err(|e| Error::Internal(format!("unexpected GitHub response: {e}")))
    }

    fn send(
        &self,
        method: Method,
        url: String,
        body: Option<serde_json::Value>,
    ) -> Result<Response, Error> {
        let body = body
            .map(|b| serde_json::to_vec(&b))
            .transpose()
            .map_err(|e| Error::Internal(format!("encoding request: {e}")))?;
        let mut headers = vec![
            ("user-agent", USER_AGENT.to_string()),
            ("accept", "application/vnd.github+json".to_string()),
            ("x-github-api-version", self.settings.api_version.clone()),
        ];
        if body.is_some() {
            headers.push(("content-type", "application/json".to_string()));
        }
        self.transport.send(Request {
            method,
            url,
            headers,
            body,
        })
    }

    /// Draft is GitHub's native flag or a known title prefix; the title is returned without it.
    fn pull_request(&self, repo: RepoRef, pr: ApiPullRequest) -> PullRequest {
        let (prefixed, title) = draft::split_title(&pr.title, &self.settings.prefixes);
        let state = if pr.merged_at.is_some() {
            PullRequestState::Merged
        } else if pr.state == "closed" {
            PullRequestState::Closed
        } else {
            PullRequestState::Open
        };
        PullRequest {
            id: PullRequestRef {
                repo,
                number: pr.number,
            },
            url: pr.html_url,
            head: pr.head.branch,
            base: pr.base.branch,
            title,
            body: pr.body.unwrap_or_default(),
            draft: pr.draft || prefixed,
            state,
            head_sha: pr.head.sha,
        }
    }
}

/// GraphQL mutations of the native draft state, which the REST API cannot change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutation {
    MarkReady,
    ConvertToDraft,
}

impl Mutation {
    fn document(self) -> &'static str {
        match self {
            Self::MarkReady => {
                "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { clientMutationId } }"
            }
            Self::ConvertToDraft => {
                "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { clientMutationId } }"
            }
        }
    }
}

fn repo_path(repo: &RepoRef) -> String {
    format!(
        "/repos/{}/{}",
        utf8_percent_encode(&repo.owner, QUERY_VALUE),
        utf8_percent_encode(&repo.name, QUERY_VALUE)
    )
}

#[derive(Deserialize)]
struct ApiBranch {
    #[serde(rename = "ref")]
    branch: String,
    #[serde(default)]
    sha: String,
}

#[derive(Deserialize)]
struct ApiPullRequest {
    number: u64,
    /// GraphQL id for the draft mutations.
    #[serde(default)]
    node_id: String,
    html_url: String,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    draft: bool,
    merged_at: Option<String>,
    head: ApiBranch,
    base: ApiBranch,
}

#[derive(Deserialize)]
struct ApiLabel {
    name: String,
}

#[derive(Deserialize)]
struct ApiIssue {
    html_url: String,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<ApiLabel>,
}

#[derive(Deserialize)]
struct ApiComment {
    id: u64,
    html_url: String,
}

#[cfg(test)]
mod tests;
