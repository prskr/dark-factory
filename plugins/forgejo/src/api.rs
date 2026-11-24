//! Forgejo REST API (`/api/v1`) behind the `pull-requests` and `issues` interfaces.

use std::cell::Cell;

use serde::{Deserialize, de::DeserializeOwned};

use crate::darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use crate::error;
use crate::exports::darkfactory::integrations::issues::{Comment, Issue};
use crate::exports::darkfactory::integrations::pull_requests::{
    ListedPullRequest, NewPullRequest, PullRequest, PullRequestState, PullRequestUpdate,
};
use forge_common::draft;
use forge_common::http;
use forge_common::log::Log;
use forge_common::open_filter::{Candidate, OpenFilter};

mod ci;
mod feedback;
mod issues;
mod merge;

const PAGE_SIZE: usize = 50;

pub use forge_common::http::Method;

/// A request relative to the configured `base-url`, e.g. `/api/v1/repos/o/r/pulls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub body: Option<Vec<u8>>,
    /// Besides the JSON `accept` and `content-type`; never `authorization`: the host injects it.
    pub headers: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// `Retry-After` in seconds, if present and numeric.
    pub retry_after: Option<u32>,
    pub body: Vec<u8>,
}

/// Sends a request to Forgejo; the host adds credentials.
pub trait Transport {
    fn send(&self, request: Request) -> Result<Response, Error>;

    /// Like [`Self::send`], but the body is cut to its last `max` bytes; true if its beginning
    /// was dropped. Implementations may stop reading early instead of buffering a huge body.
    fn send_tail(&self, request: Request, max: usize) -> Result<(Response, bool), Error> {
        let mut response = self.send(request)?;
        let cut = forge_common::log_tail::keep_tail(&mut response.body, max);
        Ok((response, cut))
    }
}

/// A failed request: the error and the HTTP status of the response, if there was one.
struct Failure {
    status: Option<u16>,
    error: Error,
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self {
            status: None,
            error,
        }
    }
}

pub struct Forgejo<T, L> {
    transport: T,
    log: L,
    prefixes: Vec<String>,
    /// Major version of the server, once asked for.
    version: Cell<Option<u32>>,
}

impl<T: Transport, L: Log> Forgejo<T, L> {
    pub fn new(transport: T, log: L, prefixes: Vec<String>) -> Self {
        Self {
            transport,
            log,
            prefixes,
            version: Cell::new(None),
        }
    }

    pub fn create(&self, request: NewPullRequest) -> Result<PullRequest, Error> {
        let body = pull_request_body(
            Some(&draft::apply_title(
                &request.title,
                request.draft,
                &self.prefixes,
            )),
            Some(&request.body),
            Some((&request.head, &request.base)),
        );
        let pr: ApiPullRequest = self.call(
            Method::Post,
            format!("{}/pulls", repo_path(&request.repo)),
            Some(body),
        )?;
        Ok(self.pull_request(request.repo, pr))
    }

    pub fn find_open(
        &self,
        repo: &RepoRef,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, Error> {
        for page in 1.. {
            let prs: Vec<ApiPullRequest> = self.call(
                Method::Get,
                format!(
                    "{}/pulls?state=open&page={page}&limit={PAGE_SIZE}",
                    repo_path(repo)
                ),
                None,
            )?;
            // Only an empty page ends the search: the server may cap `limit` below PAGE_SIZE.
            let last_page = prs.is_empty();
            if let Some(pr) = prs
                .into_iter()
                .find(|pr| pr.head.branch == head && pr.base.branch == base)
            {
                return Ok(Some(self.pull_request(repo.clone(), pr)));
            }
            if last_page {
                break;
            }
        }
        Ok(None)
    }

    /// Every open pull request of `repo` that matches `filter`. The author is filtered by Forgejo
    /// (v10 or later, `unsupported` below), the rest here.
    pub fn list_open(
        &self,
        repo: &RepoRef,
        filter: &OpenFilter<'_>,
    ) -> Result<Vec<ListedPullRequest>, Error> {
        let poster = match filter.author {
            Some(author) => {
                self.require_version(10, "filtering pull requests by author needs")?;
                format!("&poster={}", forge_common::url::encode(author))
            }
            None => String::new(),
        };
        let mut found = Vec::new();
        for page in 1.. {
            let prs: Vec<ApiPullRequest> = self.call(
                Method::Get,
                format!(
                    "{}/pulls?state=open{poster}&page={page}&limit={PAGE_SIZE}",
                    repo_path(repo)
                ),
                None,
            )?;
            if prs.is_empty() {
                break;
            }
            for pr in prs.into_iter().filter(|pr| pr.state == "open") {
                let labels: Vec<String> = pr.labels.iter().map(|l| l.name.clone()).collect();
                let author = pr
                    .user
                    .as_ref()
                    .map(|u| u.login.clone())
                    .unwrap_or_default();
                let candidate = Candidate {
                    head: &pr.head.branch,
                    head_repo: pr.head.repo.as_ref().map(|r| r.full_name.as_str()),
                    base: &pr.base.branch,
                    author: &author,
                    labels: &labels,
                };
                if filter.matches(&repo.owner, &repo.name, &candidate) {
                    found.push(ListedPullRequest {
                        pr: self.pull_request(repo.clone(), pr),
                        author,
                        labels,
                    });
                }
            }
        }
        Ok(found)
    }

    /// Forgejo's major version, read once per instance from `/api/v1/version`.
    fn major_version(&self) -> Result<u32, Error> {
        if let Some(major) = self.version.get() {
            return Ok(major);
        }
        let v: ApiVersion = self.call(Method::Get, "/api/v1/version".into(), None)?;
        let digits: String = v
            .version
            .trim_start_matches('v')
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let major = digits.parse().map_err(|_unparsable| {
            Error::Internal(format!("unexpected Forgejo version `{}`", v.version))
        })?;
        self.version.set(Some(major));
        Ok(major)
    }

    /// `unsupported` unless Forgejo is at least v`min`; `what` is the subject and verb of the
    /// message, e.g. `job logs need`.
    fn require_version(&self, min: u32, what: &str) -> Result<(), Error> {
        if self.major_version()? >= min {
            return Ok(());
        }
        Err(Error::Unsupported(format!(
            "{what} Forgejo v{min} or later"
        )))
    }

    pub fn get(&self, id: &PullRequestRef) -> Result<PullRequest, Error> {
        let pr = self.get_raw(id)?;
        Ok(self.pull_request(id.repo.clone(), pr))
    }

    pub fn update(
        &self,
        id: &PullRequestRef,
        changes: PullRequestUpdate,
    ) -> Result<PullRequest, Error> {
        let title = if changes.title.is_some() || changes.draft.is_some() {
            // The marker is part of the title: derive the new state from the current one.
            let current = self.get_raw(id)?;
            let (was_draft, plain) = draft::split_title(&current.title, &self.prefixes);
            let draft = changes.draft.unwrap_or(was_draft);
            Some(draft::apply_title(
                changes.title.as_deref().unwrap_or(&plain),
                draft,
                &self.prefixes,
            ))
        } else {
            None
        };
        let pr: ApiPullRequest = self.call(
            Method::Patch,
            format!("{}/pulls/{}", repo_path(&id.repo), id.number),
            Some(pull_request_body(
                title.as_deref(),
                changes.body.as_deref(),
                None,
            )),
        )?;
        Ok(self.pull_request(id.repo.clone(), pr))
    }

    pub fn get_issue(&self, id: &IssueRef) -> Result<Issue, Error> {
        let issue: ApiIssue = self.call(
            Method::Get,
            format!("{}/issues/{}", repo_path(&id.repo), id.number),
            None,
        )?;
        Ok(issues::issue_of(&id.repo, issue))
    }

    pub fn add_comment(&self, id: &IssueRef, body: &str) -> Result<Comment, Error> {
        let comment: ApiComment = self.call(
            Method::Post,
            format!("{}/issues/{}/comments", repo_path(&id.repo), id.number),
            Some(serde_json::json!({ "body": body })),
        )?;
        Ok(Comment {
            id: comment.id.to_string(),
            url: comment.html_url,
        })
    }

    fn get_raw(&self, id: &PullRequestRef) -> Result<ApiPullRequest, Error> {
        self.call(
            Method::Get,
            format!("{}/pulls/{}", repo_path(&id.repo), id.number),
            None,
        )
    }

    fn call<R: DeserializeOwned>(
        &self,
        method: Method,
        path: String,
        body: Option<serde_json::Value>,
    ) -> Result<R, Error> {
        self.call_with_status(method, path, body)
            .map_err(|failure| failure.error)
    }

    /// Like [`Self::call`]; a failure also carries the HTTP status of the response, if any.
    fn call_with_status<R: DeserializeOwned>(
        &self,
        method: Method,
        path: String,
        body: Option<serde_json::Value>,
    ) -> Result<R, Failure> {
        let response = self.send_checked(method, path, body)?;
        Ok(serde_json::from_slice(&response.body)
            .map_err(|e| Error::Internal(format!("unexpected Forgejo response: {e}")))?)
    }

    /// Sends the request and returns the response of a 2xx status, without reading its body.
    fn send_checked(
        &self,
        method: Method,
        path: String,
        body: Option<serde_json::Value>,
    ) -> Result<Response, Failure> {
        let body = body
            .map(|b| serde_json::to_vec(&b))
            .transpose()
            .map_err(|e| Error::Internal(format!("encoding request: {e}")))?;
        let request = http::request_line(method, &path);
        let response = self.transport.send(Request {
            method,
            path,
            body,
            headers: Vec::new(),
        })?;
        if !(200..300).contains(&response.status) {
            return Err(Failure {
                status: Some(response.status),
                error: error::with_request(
                    error::from_status(response.status, response.retry_after, &response.body),
                    &request,
                ),
            });
        }
        Ok(response)
    }

    fn pull_request(&self, repo: RepoRef, pr: ApiPullRequest) -> PullRequest {
        let (draft, title) = draft::split_title(&pr.title, &self.prefixes);
        PullRequest {
            id: PullRequestRef {
                repo,
                number: pr.number,
            },
            url: pr.html_url,
            head: pr.head.branch,
            base: pr.base.branch,
            title,
            body: pr.body,
            draft,
            state: match (pr.state.as_str(), pr.merged) {
                (_, true) => PullRequestState::Merged,
                ("closed", false) => PullRequestState::Closed,
                _ => PullRequestState::Open,
            },
            head_sha: pr.head.sha,
        }
    }
}

fn repo_path(repo: &RepoRef) -> String {
    format!("/api/v1/repos/{}/{}", repo.owner, repo.name)
}

/// Request body with only the given fields; `branches` is `(head, base)` for creation.
fn pull_request_body(
    title: Option<&str>,
    body: Option<&str>,
    branches: Option<(&str, &str)>,
) -> serde_json::Value {
    let mut value = serde_json::Map::new();
    if let Some(title) = title {
        value.insert("title".into(), title.into());
    }
    if let Some(body) = body {
        value.insert("body".into(), body.into());
    }
    if let Some((head, base)) = branches {
        value.insert("head".into(), head.into());
        value.insert("base".into(), base.into());
    }
    value.into()
}

#[derive(Deserialize)]
struct ApiBranch {
    #[serde(rename = "ref")]
    branch: String,
    #[serde(default)]
    sha: String,
    /// The repository of the branch; absent when a fork was deleted.
    #[serde(default)]
    repo: Option<ApiHeadRepo>,
}

#[derive(Deserialize)]
struct ApiHeadRepo {
    full_name: String,
}

#[derive(Deserialize)]
struct ApiPullRequest {
    number: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    body: String,
    state: String,
    #[serde(default)]
    merged: bool,
    head: ApiBranch,
    base: ApiBranch,
    #[serde(default)]
    user: Option<ApiUser>,
    #[serde(default, deserialize_with = "null_as_empty")]
    labels: Vec<ApiLabel>,
}

/// Forgejo writes an empty list as `null` in places (the statuses past the last page, no labels).
fn null_as_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Deserialize)]
struct ApiVersion {
    version: String,
}

#[derive(Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Deserialize)]
struct ApiLabel {
    name: String,
}

#[derive(Deserialize)]
struct ApiIssue {
    number: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    body: String,
    state: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    labels: Vec<ApiLabel>,
    /// Present (not null) for a pull request.
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiComment {
    id: u64,
    html_url: String,
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::exports::darkfactory::integrations::issues::IssueState;
    use forge_common::log::StderrLog;

    /// Replays canned responses and records the requests.
    struct Fake {
        responses: RefCell<VecDeque<Response>>,
        requests: RefCell<Vec<Request>>,
    }

    impl Transport for &Fake {
        fn send(&self, request: Request) -> Result<Response, Error> {
            self.requests.borrow_mut().push(request);
            Ok(self.responses.borrow_mut().pop_front().expect("response"))
        }
    }

    fn fake(responses: Vec<Response>) -> Fake {
        Fake {
            responses: RefCell::new(responses.into()),
            requests: RefCell::default(),
        }
    }

    fn json(status: u16, body: serde_json::Value) -> Response {
        Response {
            status,
            retry_after: None,
            body: body.to_string().into_bytes(),
        }
    }

    fn pr_json(title: &str, branch: &str) -> serde_json::Value {
        serde_json::json!({
            "number": 7, "html_url": "https://git/pr/7", "title": title, "body": "b",
            "state": "open", "merged": false,
            "head": {"ref": branch, "sha": "abc", "repo": {"full_name": "o/r"}},
            "base": {"ref": "main", "sha": "def"}
        })
    }

    fn numbered_pr(number: u64, branch: &str, base: &str, head_repo: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number, "html_url": format!("https://git/pr/{number}"), "title": "t",
            "body": "", "state": "open", "merged": false,
            "head": {"ref": branch, "sha": "abc", "repo": {"full_name": head_repo}},
            "base": {"ref": base, "sha": "def"}
        })
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

    fn sent_json(fake: &Fake, index: usize) -> serde_json::Value {
        let body = fake.requests.borrow()[index].body.clone().expect("body");
        serde_json::from_slice(&body).expect("json")
    }

    #[test]
    fn create_draft_adds_marker_and_hides_it() -> anyhow::Result<()> {
        let fake = fake(vec![json(201, pr_json("WIP: DF-1: Fix login", "f/t/DF-1"))]);
        let pr = plugin(&fake).create(NewPullRequest {
            repo: repo(),
            head: "f/t/DF-1".into(),
            base: "main".into(),
            title: "DF-1: Fix login".into(),
            body: "b".into(),
            draft: true,
        })?;
        assert_eq!(sent_json(&fake, 0)["title"], "WIP: DF-1: Fix login");
        assert_eq!(fake.requests.borrow()[0].path, "/api/v1/repos/o/r/pulls");
        assert_eq!((pr.title.as_str(), pr.draft), ("DF-1: Fix login", true));
        assert_eq!(pr.head_sha, "abc");
        Ok(())
    }

    #[test]
    fn duplicate_create_is_a_conflict() {
        let fake = fake(vec![json(409, serde_json::json!({"message": "exists"}))]);
        let err = plugin(&fake)
            .create(NewPullRequest {
                repo: repo(),
                head: "h".into(),
                base: "main".into(),
                title: "t".into(),
                body: String::new(),
                draft: false,
            })
            .unwrap_err();
        assert!(matches!(err, Error::Conflict(_)));
    }

    #[test]
    fn find_open_matches_head_and_base_across_pages() -> anyhow::Result<()> {
        // The server may cap `limit`, so a page shorter than PAGE_SIZE is not the last one.
        let fake = fake(vec![
            json(200, vec![pr_json("x", "other")].into()),
            json(200, vec![pr_json("t", "f/t/DF-1")].into()),
        ]);
        let pr = plugin(&fake).find_open(&repo(), "f/t/DF-1", "main")?;
        assert_eq!(pr.map(|p| p.id.number), Some(7));
        assert!(fake.requests.borrow()[1].path.contains("page=2"));
        Ok(())
    }

    #[test]
    fn list_open_returns_every_base_across_pages() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(
                200,
                serde_json::json!([
                    numbered_pr(42, "fix/shared", "main", "o/r"),
                    numbered_pr(1, "other", "main", "o/r"),
                ]),
            ),
            json(
                200,
                serde_json::json!([numbered_pr(43, "fix/shared", "release/1.2", "O/R")]),
            ),
            json(200, serde_json::json!([])),
        ]);
        let prs = plugin(&fake).list_open(&repo(), &head("fix/shared"))?;
        let found: Vec<_> = prs
            .iter()
            .map(|p| (p.pr.id.number, p.pr.base.as_str()))
            .collect();
        assert_eq!(found, [(42, "main"), (43, "release/1.2")]);
        assert!(fake.requests.borrow()[1].path.contains("page=2"));
        Ok(())
    }

    #[test]
    fn list_open_asks_for_open_pull_requests_and_drops_closed_ones() -> anyhow::Result<()> {
        let mut closed = numbered_pr(5, "fix/login", "main", "o/r");
        closed["state"] = serde_json::json!("closed");
        let fake = fake(vec![
            json(200, serde_json::json!([closed])),
            json(200, serde_json::json!([])),
        ]);
        assert!(
            plugin(&fake)
                .list_open(&repo(), &head("fix/login"))?
                .is_empty()
        );
        assert!(fake.requests.borrow()[0].path.contains("state=open"));
        Ok(())
    }

    fn head(branch: &str) -> OpenFilter<'_> {
        OpenFilter {
            head: Some(branch),
            ..OpenFilter::default()
        }
    }

    fn renovate_pr(number: u64, branch: &str, labels: &[&str]) -> serde_json::Value {
        let mut pr = numbered_pr(number, branch, "main", "o/r");
        pr["user"] = serde_json::json!({"login": "renovate-bot"});
        pr["labels"] = labels
            .iter()
            .map(|l| serde_json::json!({"name": l}))
            .collect();
        pr
    }

    #[test]
    fn list_open_filters_by_head_prefix_and_label_and_returns_author_and_labels()
    -> anyhow::Result<()> {
        let fake = fake(vec![
            json(
                200,
                serde_json::json!([
                    renovate_pr(40, "renovate/a", &["major"]),
                    renovate_pr(41, "renovate/b", &[]),
                    renovate_pr(44, "fix/login", &["major"]),
                ]),
            ),
            json(200, serde_json::json!([])),
        ]);
        let filter = OpenFilter {
            head_prefix: Some("renovate/"),
            label: Some("major"),
            ..OpenFilter::default()
        };

        let prs = plugin(&fake).list_open(&repo(), &filter)?;

        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].pr.id.number, 40);
        assert_eq!(prs[0].author, "renovate-bot");
        assert_eq!(prs[0].labels, ["major"]);
        Ok(())
    }

    #[test]
    fn list_open_by_author_asks_forgejo_v10_or_later_for_that_poster() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(200, serde_json::json!({"version": "10.0.0+gitea-1.22.0"})),
            json(200, serde_json::json!([renovate_pr(40, "renovate/a", &[])])),
            json(200, serde_json::json!([])),
        ]);
        let filter = OpenFilter {
            author: Some("renovate-bot"),
            ..OpenFilter::default()
        };

        let prs = plugin(&fake).list_open(&repo(), &filter)?;

        assert_eq!(prs.len(), 1);
        assert!(
            fake.requests.borrow()[1]
                .path
                .contains("&poster=renovate-bot")
        );
        Ok(())
    }

    #[test]
    fn list_open_by_author_below_v10_is_unsupported() {
        let fake = fake(vec![json(
            200,
            serde_json::json!({"version": "9.0.3+gitea-1.22.0"}),
        )]);
        let filter = OpenFilter {
            author: Some("a"),
            ..OpenFilter::default()
        };

        let err = plugin(&fake).list_open(&repo(), &filter).unwrap_err();

        assert!(
            matches!(&err, Error::Unsupported(m) if m.contains("v10")),
            "{err:?}"
        );
        assert_eq!(fake.requests.borrow().len(), 1);
    }

    #[test]
    fn list_open_skips_fork_pull_requests_and_finds_none_when_closed() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(
                200,
                serde_json::json!([numbered_pr(5, "fix/login", "main", "fork/r")]),
            ),
            json(200, serde_json::json!([])),
        ]);
        assert!(
            plugin(&fake)
                .list_open(&repo(), &head("fix/login"))?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn find_open_without_match_is_none() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(200, vec![pr_json("x", "other")].into()),
            json(200, serde_json::json!([])),
        ]);
        assert!(plugin(&fake).find_open(&repo(), "h", "main")?.is_none());
        assert_eq!(fake.requests.borrow().len(), 2);
        Ok(())
    }

    #[test]
    fn update_ready_removes_marker() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(200, pr_json("WIP: Fix", "h")),
            json(200, pr_json("Fix", "h")),
        ]);
        let id = PullRequestRef {
            repo: repo(),
            number: 7,
        };
        let pr = plugin(&fake).update(
            &id,
            PullRequestUpdate {
                title: None,
                body: None,
                draft: Some(false),
            },
        )?;
        assert_eq!(sent_json(&fake, 1), serde_json::json!({"title": "Fix"}));
        assert!(!pr.draft);
        Ok(())
    }

    #[test]
    fn retitle_keeps_draft_state() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(200, pr_json("WIP: Fix", "h")),
            json(200, pr_json("WIP: Fix redirect", "h")),
        ]);
        let id = PullRequestRef {
            repo: repo(),
            number: 7,
        };
        let pr = plugin(&fake).update(
            &id,
            PullRequestUpdate {
                title: Some("Fix redirect".into()),
                body: None,
                draft: None,
            },
        )?;
        assert_eq!(
            sent_json(&fake, 1),
            serde_json::json!({"title": "WIP: Fix redirect"})
        );
        assert_eq!((pr.title.as_str(), pr.draft), ("Fix redirect", true));
        Ok(())
    }

    #[test]
    fn body_only_update_skips_the_lookup() -> anyhow::Result<()> {
        let fake = fake(vec![json(200, pr_json("t", "h"))]);
        let id = PullRequestRef {
            repo: repo(),
            number: 7,
        };
        plugin(&fake).update(
            &id,
            PullRequestUpdate {
                title: None,
                body: Some("new".into()),
                draft: None,
            },
        )?;
        assert_eq!(fake.requests.borrow().len(), 1);
        assert_eq!(sent_json(&fake, 0), serde_json::json!({"body": "new"}));
        Ok(())
    }

    #[test]
    fn merged_pull_request_state() -> anyhow::Result<()> {
        let mut merged = pr_json("t", "h");
        merged["state"] = "closed".into();
        merged["merged"] = true.into();
        let fake = fake(vec![json(200, merged)]);
        let pr = plugin(&fake).get(&PullRequestRef {
            repo: repo(),
            number: 7,
        })?;
        assert!(matches!(pr.state, PullRequestState::Merged));
        Ok(())
    }

    #[test]
    fn issue_and_comment() -> anyhow::Result<()> {
        let fake = fake(vec![
            json(
                200,
                serde_json::json!({"number": 3, "html_url": "u", "title": "T", "body": "B", "state": "closed",
                    "labels": [{"name": "bug"}]}),
            ),
            json(201, serde_json::json!({"id": 99, "html_url": "c"})),
        ]);
        let id = IssueRef {
            repo: repo(),
            number: 3,
        };
        let issue = plugin(&fake).get_issue(&id)?;
        assert_eq!(issue.labels, vec!["bug".to_string()]);
        assert!(matches!(issue.state, IssueState::Closed));
        let comment = plugin(&fake).add_comment(&id, "hi")?;
        assert_eq!((comment.id.as_str(), comment.url.as_str()), ("99", "c"));
        assert_eq!(
            fake.requests.borrow()[1].path,
            "/api/v1/repos/o/r/issues/3/comments"
        );
        Ok(())
    }

    #[test]
    fn unauthorized_is_mapped() {
        let fake = fake(vec![json(401, serde_json::json!({"message": "bad token"}))]);
        let err = plugin(&fake)
            .get_issue(&IssueRef {
                repo: repo(),
                number: 1,
            })
            .unwrap_err();
        assert!(matches!(err, Error::Unauthorized(_)));
    }
}
