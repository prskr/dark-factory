//! Forgejo REST API (`/api/v1`) behind the `pull-requests` and `issues` interfaces.

use serde::{Deserialize, de::DeserializeOwned};

use crate::darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use crate::draft;
use crate::error;
use crate::exports::darkfactory::integrations::issues::{Comment, Issue, IssueState};
use crate::exports::darkfactory::integrations::pull_requests::{
    NewPullRequest, PullRequest, PullRequestState, PullRequestUpdate,
};

const PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
}

/// A request relative to the configured `base-url`, e.g. `/api/v1/repos/o/r/pulls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub body: Option<Vec<u8>>,
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
}

pub struct Forgejo<T> {
    transport: T,
    prefixes: Vec<String>,
}

impl<T: Transport> Forgejo<T> {
    pub fn new(transport: T, prefixes: Vec<String>) -> Self {
        Self {
            transport,
            prefixes,
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
        Ok(Issue {
            id: id.clone(),
            url: issue.html_url,
            title: issue.title,
            body: issue.body,
            state: if issue.state == "closed" {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            labels: issue.labels.into_iter().map(|l| l.name).collect(),
        })
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
        let body = body
            .map(|b| serde_json::to_vec(&b))
            .transpose()
            .map_err(|e| Error::Internal(format!("encoding request: {e}")))?;
        let response = self.transport.send(Request { method, path, body })?;
        if !(200..300).contains(&response.status) {
            return Err(error::from_status(
                response.status,
                response.retry_after,
                &response.body,
            ));
        }
        serde_json::from_slice(&response.body)
            .map_err(|e| Error::Internal(format!("unexpected Forgejo response: {e}")))
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
}

#[derive(Deserialize)]
struct ApiLabel {
    name: String,
}

#[derive(Deserialize)]
struct ApiIssue {
    html_url: String,
    title: String,
    #[serde(default)]
    body: String,
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
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

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
            "head": {"ref": branch, "sha": "abc"}, "base": {"ref": "main", "sha": "def"}
        })
    }

    fn repo() -> RepoRef {
        RepoRef {
            owner: "o".into(),
            name: "r".into(),
        }
    }

    fn plugin(fake: &Fake) -> Forgejo<&Fake> {
        Forgejo::new(fake, draft::parse_prefixes(draft::DEFAULT_PREFIXES))
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
                serde_json::json!({"html_url": "u", "title": "T", "body": "B", "state": "closed",
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
