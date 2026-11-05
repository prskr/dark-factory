//! `list-feedback` and `reply` through GraphQL, which has review threads with ids and resolved state.
//! One query reads the newest 100 items per connection with `last: 100` (in chronological order)
//! and no inner paging; any older page sets `truncated`. Author roles need one REST permission
//! lookup per distinct owner, member or collaborator.

use std::collections::HashMap;

use percent_encoding::utf8_percent_encode;
use serde::Deserialize;
use serde_json::json;

use crate::darkfactory::integrations::types::{Error, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::{
    Author, AuthorRole, Feedback, PostedComment, PrComment, Review, ReviewComment, ReviewThread,
    ReviewVerdict,
};
use crate::timestamp::{self, touched_since};

use super::{Clock, GitHub, Log, Method, QUERY_VALUE, Transport, repo_path};

/// Pending reviews are left out; only their author could see them.
const FEEDBACK_QUERY: &str = r"query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      comments(last: 100) {
        pageInfo { hasPreviousPage }
        nodes { id author { login } authorAssociation body url createdAt updatedAt }
      }
      reviews(last: 100, states: [APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED]) {
        pageInfo { hasPreviousPage }
        nodes { id author { login } authorAssociation state body url submittedAt updatedAt }
      }
      reviewThreads(last: 100) {
        pageInfo { hasPreviousPage }
        nodes {
          id isResolved path line
          comments(last: 100) {
            pageInfo { hasPreviousPage }
            nodes { id author { login } authorAssociation state body url createdAt updatedAt }
          }
        }
      }
    }
  }
}";

const THREAD_QUERY: &str = r"query($id: ID!) {
  node(id: $id) {
    ... on PullRequestReviewThread { pullRequest { number repository { nameWithOwner } } }
  }
}";

const VIEWER_QUERY: &str = "query { viewer { login } }";

const REPLY_MUTATION: &str = r"mutation($thread: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $thread, body: $body}) {
    comment { id url }
  }
}";

impl<T: Transport, C: Clock, L: Log> GitHub<T, C, L> {
    pub fn list_feedback(
        &self,
        id: &PullRequestRef,
        since: Option<&str>,
    ) -> Result<Feedback, Error> {
        let since = since
            .map(|s| {
                timestamp::parse(s).ok_or_else(|| {
                    Error::InvalidInput(format!("`since` is not an RFC 3339 timestamp: {s}"))
                })
            })
            .transpose()?;
        let data: FeedbackData = self.query(
            FEEDBACK_QUERY,
            json!({ "owner": id.repo.owner, "name": id.repo.name, "number": id.number }),
        )?;
        let pr = data
            .repository
            .and_then(|r| r.pull_request)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "pull request {}/{}#{}",
                    id.repo.owner, id.repo.name, id.number
                ))
            })?;
        let truncated = pr.comments.page_info.has_previous_page
            || pr.reviews.page_info.has_previous_page
            || pr.review_threads.page_info.has_previous_page
            || pr
                .review_threads
                .nodes
                .iter()
                .flatten()
                .any(|t| t.comments.page_info.has_previous_page);
        let newer = |timestamps: &[&str]| since.is_none_or(|s| touched_since(timestamps, s));
        let mut roles = Roles {
            github: self,
            repo: &id.repo,
            bot: data.viewer.login,
            known: HashMap::new(),
        };

        let comments = pr
            .comments
            .nodes
            .into_iter()
            .flatten()
            .filter(|c| newer(&[&c.created_at, &c.updated_at]))
            .map(|c| roles.pr_comment(c))
            .collect::<Result<_, _>>()?;
        let reviews = pr
            .reviews
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|r| verdict(&r.state).map(|verdict| (r, verdict)))
            .filter(|(r, _)| newer(&[r.submitted_at.as_deref().unwrap_or(""), &r.updated_at]))
            .map(|(r, verdict)| roles.review(r, verdict))
            .collect::<Result<_, _>>()?;
        let mut threads = Vec::new();
        for t in pr.review_threads.nodes.into_iter().flatten() {
            let t = without_pending(t);
            let touched = t
                .comments
                .nodes
                .iter()
                .flatten()
                .any(|c| newer(&[&c.created_at, &c.updated_at]));
            if touched {
                threads.push(roles.thread(t)?);
            }
        }
        Ok(Feedback {
            comments,
            reviews,
            threads,
            truncated,
        })
    }

    /// Login of the account owning the token.
    pub fn whoami(&self) -> Result<String, Error> {
        let data: ViewerData = self.query(VIEWER_QUERY, json!({}))?;
        Ok(data.viewer.login)
    }

    /// `login`'s role in `repo`; without an author association, write access is always looked up.
    pub fn author_role(&self, repo: &RepoRef, login: &str) -> Result<AuthorRole, Error> {
        Roles {
            github: self,
            repo,
            bot: self.whoami()?,
            known: HashMap::new(),
        }
        .role(login, None)
    }

    /// Replies in `thread` after checking that it belongs to the pull request `id`.
    pub fn reply(
        &self,
        id: &PullRequestRef,
        thread: &str,
        body: &str,
    ) -> Result<PostedComment, Error> {
        let repo = format!("{}/{}", id.repo.owner, id.repo.name);
        let invalid = || {
            Error::InvalidInput(format!(
                "`{thread}` is not a review thread of pull request {repo}#{}",
                id.number
            ))
        };
        if thread.is_empty() {
            return Err(invalid());
        }
        // An unknown id fails here as GraphQL `NOT_FOUND`.
        let data: NodeData = self.query(THREAD_QUERY, json!({ "id": thread }))?;
        let pr = data.node.and_then(|n| n.pull_request).ok_or_else(invalid)?;
        if pr.number != id.number || !pr.repository.name_with_owner.eq_ignore_ascii_case(&repo) {
            return Err(invalid());
        }
        let data: ReplyData =
            self.query(REPLY_MUTATION, json!({ "thread": thread, "body": body }))?;
        let comment = data
            .add_pull_request_review_thread_reply
            .comment
            .ok_or_else(|| {
                Error::Internal("GitHub returned no comment for the reply".to_string())
            })?;
        Ok(PostedComment {
            id: comment.id,
            url: comment.url,
        })
    }
}

/// Assigns author roles: `bot` for the viewer; `member` for an owner, member or collaborator
/// association with write permission, one permission lookup per distinct login.
struct Roles<'a, T, C, L> {
    github: &'a GitHub<T, C, L>,
    repo: &'a RepoRef,
    bot: String,
    /// Keyed by lowercase login: GitHub logins are case-insensitive.
    known: HashMap<String, AuthorRole>,
}

impl<T: Transport, C: Clock, L: Log> Roles<'_, T, C, L> {
    fn author(&mut self, author: Option<ApiAuthor>, association: &str) -> Result<Author, Error> {
        let login = author.map(|a| a.login).unwrap_or_default();
        let role = self.role(&login, Some(association))?;
        Ok(Author { login, role })
    }

    fn role(&mut self, login: &str, association: Option<&str>) -> Result<AuthorRole, Error> {
        if login.is_empty() {
            return Ok(AuthorRole::Outside);
        }
        if login.eq_ignore_ascii_case(&self.bot) {
            return Ok(AuthorRole::Bot);
        }
        // Other associations never have write access; they need no lookup.
        if association.is_some_and(|a| !matches!(a, "OWNER" | "MEMBER" | "COLLABORATOR")) {
            return Ok(AuthorRole::Outside);
        }
        let key = login.to_ascii_lowercase();
        if let Some(role) = self.known.get(&key) {
            return Ok(*role);
        }
        let path = format!(
            "{}/collaborators/{}/permission",
            repo_path(self.repo),
            utf8_percent_encode(login, QUERY_VALUE)
        );
        let role = match self.github.rest::<ApiPermission>(Method::Get, &path, None) {
            // `maintain` is reported as `write`.
            Ok(p) if matches!(p.permission.as_str(), "admin" | "write") => AuthorRole::Member,
            // A deleted account is not found.
            Ok(_) | Err(Error::NotFound(_)) => AuthorRole::Outside,
            Err(e) => return Err(e),
        };
        self.known.insert(key, role);
        Ok(role)
    }

    fn pr_comment(&mut self, c: GqlComment) -> Result<PrComment, Error> {
        Ok(PrComment {
            author: self.author(c.author, &c.author_association)?,
            id: c.id,
            body: c.body,
            url: c.url,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
    }

    fn review(&mut self, r: ApiReview, verdict: ReviewVerdict) -> Result<Review, Error> {
        Ok(Review {
            author: self.author(r.author, &r.author_association)?,
            id: r.id,
            verdict,
            body: r.body,
            url: r.url,
            submitted_at: r.submitted_at.unwrap_or_default(),
        })
    }

    fn thread(&mut self, t: ApiThread) -> Result<ReviewThread, Error> {
        let mut comments = Vec::new();
        for c in t.comments.nodes.into_iter().flatten() {
            comments.push(ReviewComment {
                author: self.author(c.author, &c.author_association)?,
                id: c.id,
                body: c.body,
                url: c.url,
                created_at: c.created_at,
                updated_at: c.updated_at,
            });
        }
        Ok(ReviewThread {
            id: t.id,
            path: t.path,
            line: t.line,
            resolved: t.is_resolved,
            comments,
        })
    }
}

/// The thread without comments of a pending review; a thread with only those has no comments.
fn without_pending(mut t: ApiThread) -> ApiThread {
    t.comments
        .nodes
        .retain(|c| c.as_ref().is_some_and(|c| c.state != "PENDING"));
    t
}

/// A dismissed review keeps its text but no longer its verdict.
fn verdict(state: &str) -> Option<ReviewVerdict> {
    match state {
        "APPROVED" => Some(ReviewVerdict::Approved),
        "CHANGES_REQUESTED" => Some(ReviewVerdict::ChangesRequested),
        "COMMENTED" | "DISMISSED" => Some(ReviewVerdict::Commented),
        _ => None,
    }
}

#[derive(Deserialize)]
struct ViewerData {
    viewer: ApiAuthor,
}

#[derive(Deserialize)]
struct FeedbackData {
    viewer: ApiAuthor,
    repository: Option<ApiRepository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRepository {
    pull_request: Option<ApiPullRequestFeedback>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiPullRequestFeedback {
    comments: Connection<GqlComment>,
    reviews: Connection<ApiReview>,
    review_threads: Connection<ApiThread>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<N> {
    page_info: PageInfo,
    /// GraphQL lists may hold `null` entries.
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<N>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_previous_page: bool,
}

#[derive(Deserialize)]
struct ApiAuthor {
    login: String,
}

/// A PR comment or a review comment; only review comments have a `state`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlComment {
    id: String,
    author: Option<ApiAuthor>,
    #[serde(default)]
    author_association: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiReview {
    id: String,
    author: Option<ApiAuthor>,
    #[serde(default)]
    author_association: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    url: String,
    submitted_at: Option<String>,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiThread {
    id: String,
    is_resolved: bool,
    #[serde(default)]
    path: String,
    /// `null` for a comment on the whole file or an outdated thread.
    line: Option<u32>,
    comments: Connection<GqlComment>,
}

/// `permission` is `admin`, `write` (also for `maintain`), `read` (also for `triage`) or `none`.
#[derive(Deserialize)]
struct ApiPermission {
    permission: String,
}

#[derive(Deserialize)]
struct NodeData {
    node: Option<ApiNode>,
}

/// A node that is not a review thread has no `pullRequest`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiNode {
    pull_request: Option<ApiThreadPullRequest>,
}

#[derive(Deserialize)]
struct ApiThreadPullRequest {
    number: u64,
    repository: ApiNameWithOwner,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiNameWithOwner {
    name_with_owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyData {
    add_pull_request_review_thread_reply: ApiReplyPayload,
}

#[derive(Deserialize)]
struct ApiReplyPayload {
    comment: Option<ApiPostedComment>,
}

#[derive(Deserialize)]
struct ApiPostedComment {
    id: String,
    url: String,
}

#[cfg(test)]
mod tests;
