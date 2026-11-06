//! `list-feedback` and `reply`: Forgejo has no review thread ids, so review comments of one review
//! on the same path and line form a thread, as Forgejo's UI shows them, and the thread id is
//! `<review-id>:<path>:<line>`. Old-side lines are negative, as Forgejo stores them.

use std::collections::HashMap;

use serde::Deserialize;

use crate::darkfactory::integrations::types::{Error, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::{
    Author, AuthorRole, Feedback, PostedComment, PrComment, Review, ReviewComment, ReviewThread,
    ReviewVerdict,
};
use forge_common::log::Log;
use forge_common::timestamp::{self, touched_since};

use super::{Failure, Forgejo, Method, PAGE_SIZE, Transport, repo_path};

/// Pages read per listing before `truncated` is set.
const MAX_PAGES: usize = 20;

/// Forgejo has no per-thread listing and does not move a review's `updated_at` when a comment is
/// added, so the comments of the newest reviews only are read; older ones set `truncated`.
const MAX_REVIEWS_WITH_COMMENTS: usize = 20;

impl<T: Transport, L: Log> Forgejo<T, L> {
    pub fn list_feedback(
        &self,
        id: &PullRequestRef,
        since: Option<&str>,
    ) -> Result<Feedback, Error> {
        let since = since
            .map(|s| {
                timestamp::parse(s)
                    .map(|instant| (s, instant))
                    .ok_or_else(|| {
                        Error::InvalidInput(format!("`since` is not an RFC 3339 timestamp: {s}"))
                    })
            })
            .transpose()?;
        let bot = self.whoami()?;
        let pr = format!("{}/pulls/{}", repo_path(&id.repo), id.number);

        let mut comments_path = format!("{}/issues/{}/comments", repo_path(&id.repo), id.number);
        if let Some((raw, _)) = since {
            // Forgejo filters PR comments itself; reviews and threads are filtered below.
            comments_path = format!("{comments_path}?since={}", query_value(raw));
        }
        // The endpoint ignores `page` and `limit` and always answers the whole list.
        let comments: Vec<ApiIssueComment> = self.call(Method::Get, comments_path, None)?;
        let (reviews, reviews_truncated) = self.pages::<ApiReview>(&format!("{pr}/reviews"))?;
        let mut reviews: Vec<_> = reviews
            .into_iter()
            .filter_map(|review| verdict(&review.state).map(|verdict| (review, verdict)))
            .collect();
        let newest = newest_review_ids(&reviews);
        let reviews_skipped = reviews.len() > newest.len();
        let mut review_comments = Vec::new();
        for (review, _) in reviews.iter().filter(|(r, _)| newest.contains(&r.id)) {
            let listed: Vec<ApiReviewComment> = self.call(
                Method::Get,
                format!("{pr}/reviews/{}/comments", review.id),
                None,
            )?;
            review_comments.extend(listed.into_iter().map(|comment| (review.id, comment)));
        }
        let mut threads = group(review_comments);

        if let Some((_, since)) = since {
            reviews.retain(|(r, _)| touched_since(&[&r.submitted_at, &r.updated_at], since));
            threads.retain(|t| {
                t.comments
                    .iter()
                    .any(|c| touched_since(&[&c.created_at, &c.updated_at], since))
            });
        }

        let mut roles = Roles {
            forgejo: self,
            repo: &id.repo,
            bot,
            known: HashMap::new(),
        };
        Ok(Feedback {
            comments: comments
                .into_iter()
                .map(|c| roles.pr_comment(c))
                .collect::<Result<_, _>>()?,
            reviews: reviews
                .into_iter()
                .map(|(r, verdict)| roles.review(r, verdict))
                .collect::<Result<_, _>>()?,
            threads: threads
                .into_iter()
                .map(|t| roles.thread(t))
                .collect::<Result<_, _>>()?,
            truncated: reviews_truncated || reviews_skipped,
        })
    }

    /// Login of the token owner.
    pub fn whoami(&self) -> Result<String, Error> {
        let bot: ApiUser = self.call(Method::Get, "/api/v1/user".into(), None)?;
        Ok(bot.login)
    }

    pub fn author_role(&self, repo: &RepoRef, login: &str) -> Result<AuthorRole, Error> {
        Roles {
            forgejo: self,
            repo,
            bot: self.whoami()?,
            known: HashMap::new(),
        }
        .role(login)
    }

    pub fn reply(
        &self,
        id: &PullRequestRef,
        thread: &str,
        body: &str,
    ) -> Result<PostedComment, Error> {
        let invalid = || {
            Error::InvalidInput(format!(
                "`{thread}` is not a review thread of pull request #{}",
                id.number
            ))
        };
        let (review_id, path, line) = parse_thread_id(thread).ok_or_else(invalid)?;
        let review_path = format!(
            "{}/pulls/{}/reviews/{review_id}",
            repo_path(&id.repo),
            id.number
        );
        // Forgejo answers 404 for a review of another pull request.
        let review: ApiReview = match self.call(Method::Get, review_path.clone(), None) {
            Err(Error::NotFound(_)) => return Err(invalid()),
            other => other?,
        };
        if verdict(&review.state).is_none()
            || !review
                .pull_request_url
                .ends_with(&format!("/pulls/{}", id.number))
        {
            return Err(invalid());
        }
        // Forgejo would start a new conversation for a path and line the review has no comment on.
        let comments: Vec<ApiReviewComment> =
            self.call(Method::Get, format!("{review_path}/comments"), None)?;
        if !comments.iter().any(|c| c.path == path && c.line() == line) {
            return Err(invalid());
        }
        let position = if line < 0 {
            "old_position"
        } else {
            "new_position"
        };
        let comment: ApiReviewComment = self.call(
            Method::Post,
            format!("{review_path}/comments"),
            Some(serde_json::json!({
                "path": path,
                "body": body,
                position: line.unsigned_abs(),
            })),
        )?;
        Ok(PostedComment {
            id: comment.id.to_string(),
            url: comment.html_url,
        })
    }

    /// Reads pages of `path` until an empty page or [`MAX_PAGES`]; `true` if it stopped at the limit.
    fn pages<R: serde::de::DeserializeOwned>(&self, path: &str) -> Result<(Vec<R>, bool), Error> {
        let sep = if path.contains('?') { '&' } else { '?' };
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let listed: Vec<R> = self.call(
                Method::Get,
                format!("{path}{sep}page={page}&limit={PAGE_SIZE}"),
                None,
            )?;
            // Only an empty page ends the listing: the server may cap `limit` below PAGE_SIZE.
            if listed.is_empty() {
                return Ok((items, false));
            }
            items.extend(listed);
        }
        Ok((items, true))
    }
}

/// Ids of the [`MAX_REVIEWS_WITH_COMMENTS`] reviews submitted last.
fn newest_review_ids(reviews: &[(ApiReview, ReviewVerdict)]) -> Vec<u64> {
    let mut by_age: Vec<_> = reviews
        .iter()
        .map(|(r, _)| (timestamp::parse(&r.submitted_at), r.id))
        .collect();
    by_age.sort_by(|a, b| b.cmp(a));
    by_age
        .into_iter()
        .take(MAX_REVIEWS_WITH_COMMENTS)
        .map(|(_, id)| id)
        .collect()
}

/// Assigns author roles, with one permission lookup per distinct author.
struct Roles<'a, T, L> {
    forgejo: &'a Forgejo<T, L>,
    repo: &'a RepoRef,
    bot: String,
    /// Keyed by lowercase login: Forgejo logins are case-insensitive.
    known: HashMap<String, AuthorRole>,
}

impl<T: Transport, L: Log> Roles<'_, T, L> {
    fn author(&mut self, user: Option<ApiUser>) -> Result<Author, Error> {
        let login = user.map(|u| u.login).unwrap_or_default();
        let role = self.role(&login)?;
        Ok(Author { login, role })
    }

    fn role(&mut self, login: &str) -> Result<AuthorRole, Error> {
        if login.is_empty() {
            return Ok(AuthorRole::Outside);
        }
        if login.eq_ignore_ascii_case(&self.bot) {
            return Ok(AuthorRole::Bot);
        }
        let key = login.to_ascii_lowercase();
        if let Some(role) = self.known.get(&key) {
            return Ok(*role);
        }
        let lookup = self.forgejo.call_with_status::<ApiPermission>(
            Method::Get,
            format!("{}/collaborators/{login}/permission", repo_path(self.repo)),
            None,
        );
        let role = match lookup {
            Ok(p) if matches!(p.permission.as_str(), "write" | "admin" | "owner") => {
                AuthorRole::Member
            }
            // A deleted account is not found.
            Ok(_)
            | Err(Failure {
                error: Error::NotFound(_),
                ..
            }) => AuthorRole::Outside,
            // Forgejo answers 403 unless the bot account is repository admin. A 401 is an invalid
            // credential and fails the call.
            Err(Failure {
                status: Some(403), ..
            }) => {
                self.forgejo.log.warn(&format!(
                    "cannot read the repository permission of {login}: the bot account needs \
                     repository admin; treating {login} as outside"
                ));
                AuthorRole::Outside
            }
            Err(failure) => return Err(failure.error),
        };
        self.known.insert(key, role);
        Ok(role)
    }

    fn pr_comment(&mut self, c: ApiIssueComment) -> Result<PrComment, Error> {
        Ok(PrComment {
            id: c.id.to_string(),
            author: self.author(c.user)?,
            body: c.body,
            url: c.html_url,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
    }

    fn review(&mut self, r: ApiReview, verdict: ReviewVerdict) -> Result<Review, Error> {
        Ok(Review {
            id: r.id.to_string(),
            author: self.author(r.user)?,
            verdict,
            body: r.body,
            url: r.html_url,
            submitted_at: r.submitted_at,
            updated_at: r.updated_at,
        })
    }

    fn thread(&mut self, t: Thread) -> Result<ReviewThread, Error> {
        Ok(ReviewThread {
            id: format!("{}:{}:{}", t.review_id, t.path, t.line),
            path: t.path,
            line: u32::try_from(t.line.unsigned_abs())
                .ok()
                .filter(|line| *line != 0),
            resolved: t.comments.iter().any(|c| c.resolver.is_some()),
            comments: t
                .comments
                .into_iter()
                .map(|c| {
                    Ok(ReviewComment {
                        id: c.id.to_string(),
                        author: self.author(c.user)?,
                        body: c.body,
                        url: c.html_url,
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    })
                })
                .collect::<Result<_, Error>>()?,
        })
    }
}

/// Review comments of one review at one path and signed line, oldest first.
struct Thread {
    review_id: u64,
    path: String,
    line: i64,
    comments: Vec<ApiReviewComment>,
}

/// Groups `(review id, comment)` pairs by review, path and line; threads are ordered by their first
/// comment.
fn group(mut comments: Vec<(u64, ApiReviewComment)>) -> Vec<Thread> {
    comments.sort_by_key(|(_, comment)| comment.id);
    let mut threads: Vec<Thread> = Vec::new();
    for (review_id, comment) in comments {
        let line = comment.line();
        match threads
            .iter_mut()
            .find(|t| t.review_id == review_id && t.path == comment.path && t.line == line)
        {
            Some(thread) => thread.comments.push(comment),
            None => threads.push(Thread {
                review_id,
                path: comment.path.clone(),
                line,
                comments: vec![comment],
            }),
        }
    }
    threads
}

/// `(review id, path, line)` of `<review-id>:<path>:<line>`; the path may contain `:`.
fn parse_thread_id(thread: &str) -> Option<(u64, &str, i64)> {
    let (review_id, rest) = thread.split_once(':')?;
    let (path, line) = rest.rsplit_once(':')?;
    if path.is_empty() || !review_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((review_id.parse().ok()?, path, line.parse().ok()?))
}

/// Submitted reviews only: `PENDING` is a draft and `REQUEST_REVIEW` a review request.
fn verdict(state: &str) -> Option<ReviewVerdict> {
    match state {
        "APPROVED" => Some(ReviewVerdict::Approved),
        "REQUEST_CHANGES" => Some(ReviewVerdict::ChangesRequested),
        "COMMENT" => Some(ReviewVerdict::Commented),
        _ => None,
    }
}

/// Escapes the characters of an RFC 3339 timestamp that are not literal in a query value.
fn query_value(timestamp: &str) -> String {
    timestamp
        .to_ascii_uppercase()
        .replace(':', "%3A")
        .replace('+', "%2B")
}

#[derive(Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Deserialize)]
struct ApiPermission {
    permission: String,
}

#[derive(Deserialize)]
struct ApiIssueComment {
    id: u64,
    user: Option<ApiUser>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct ApiReview {
    id: u64,
    user: Option<ApiUser>,
    #[serde(default)]
    state: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    pull_request_url: String,
    #[serde(default)]
    submitted_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct ApiReviewComment {
    id: u64,
    user: Option<ApiUser>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    path: String,
    /// New-side line, 0 if the comment is on the old side.
    #[serde(default)]
    position: u64,
    /// Old-side line.
    #[serde(default)]
    original_position: u64,
    resolver: Option<ApiUser>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

impl ApiReviewComment {
    /// Forgejo's signed line: negative on the old side, 0 for the whole file.
    fn line(&self) -> i64 {
        let signed = |line: u64| i64::try_from(line).unwrap_or(i64::MAX);
        if self.position > 0 {
            signed(self.position)
        } else {
            -signed(self.original_position)
        }
    }
}

#[cfg(test)]
mod tests;
