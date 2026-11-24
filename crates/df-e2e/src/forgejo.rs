//! The few Forgejo API calls the tests use to check what the UI does not show, and the calls
//! `setup forgejo` needs to prepare an instance.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use crate::data;
use crate::wait::eventually;

/// Bounds each API call, so a stalled port-forward fails the test instead of hanging it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a merge is retried while Forgejo re-checks the pull request.
const MERGE_TIMEOUT: Duration = Duration::from_secs(90);

/// The verdict of a review submitted with [`Forgejo::submit_review`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewEvent {
    /// The value of the `event` field of Forgejo's review API.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "APPROVED",
            Self::RequestChanges => "REQUEST_CHANGES",
            Self::Comment => "COMMENT",
        }
    }
}

/// Forgejo API client for one repository (`factory/demo` unless [`Forgejo::for_repository`] says
/// otherwise), authenticated with the token or the user it is built with.
#[derive(Debug, Clone)]
pub struct Forgejo {
    http: reqwest::Client,
    base_url: String,
    auth: Auth,
    owner: String,
    repository: String,
}

impl Forgejo {
    /// A client that authenticates with access token `token` (the bot's or the reviewer's).
    pub fn new(base_url: &str, token: &str) -> Result<Self> {
        Self::build(base_url, Auth::Token(token.to_owned()))
    }

    /// A client that authenticates as `user` with `password` (basic auth).
    pub fn with_password(base_url: &str, user: &str, password: &str) -> Result<Self> {
        Self::build(
            base_url,
            Auth::Basic {
                user: user.to_owned(),
                password: password.to_owned(),
            },
        )
    }

    /// The same client for repository `repository` of the default owner.
    #[must_use]
    pub fn for_repository(mut self, repository: &str) -> Self {
        repository.clone_into(&mut self.repository);
        self
    }

    /// Whether `branch` exists in the repository.
    pub async fn branch_exists(&self, branch: &str) -> Result<bool> {
        let response = self.get(&format!("branches/{branch}"), &[404]).await?;
        Ok(response.status().is_success())
    }

    /// The pull request from branch `head` into `base`, open or closed; `None` when there is none.
    /// Looked up by the two branches, so no page limit hides it behind other tests' pull requests.
    pub async fn pull_of(&self, base: &str, head: &str) -> Result<Option<Value>> {
        let path = format!("pulls/{base}/{head}");
        let response = self.get(&path, &[404]).await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        response
            .json()
            .await
            .with_context(|| format!("decode GET {path}"))
    }

    /// Creates an issue and returns its number.
    pub async fn create_issue(&self, title: &str, body: &str) -> Result<u64> {
        let issue = self
            .post(
                "issues",
                &serde_json::json!({ "title": title, "body": body }),
            )
            .await?;
        issue
            .get("number")
            .and_then(Value::as_u64)
            .context("issue number")
    }

    /// Title and body of issue `number`.
    pub async fn issue_text(&self, number: u64) -> Result<(String, String)> {
        let issue: Value = self.json(&format!("issues/{number}")).await?;
        let text = |key: &str| {
            issue
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .with_context(|| format!("`{key}` of issue {number}"))
        };
        Ok((text("title")?, text("body")?))
    }

    /// Comments of issue (or pull request) `number`.
    pub async fn comments(&self, number: u64) -> Result<Vec<Value>> {
        self.json(&format!("issues/{number}/comments")).await
    }

    /// Commits on `branch`, newest first.
    pub async fn commits(&self, branch: &str) -> Result<Vec<Value>> {
        self.json(&format!("commits?sha={branch}&limit=50")).await
    }

    /// Submits a review of pull request `number` with one comment on the new side of `path`
    /// at `line`, and returns the review id.
    pub async fn submit_line_comment(
        &self,
        number: u64,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<u64> {
        let review: Value = self
            .post(
                &format!("pulls/{number}/reviews"),
                &serde_json::json!({
                    "event": "COMMENT",
                    "body": "",
                    "comments": [{ "path": path, "new_position": line, "body": body }],
                }),
            )
            .await?;
        review
            .get("id")
            .and_then(Value::as_u64)
            .context("review id")
    }

    /// Comments of review `review` of pull request `number`.
    pub async fn review_comments(&self, number: u64, review: u64) -> Result<Vec<Value>> {
        self.json(&format!("pulls/{number}/reviews/{review}/comments"))
            .await
    }

    /// Submits a review of pull request `number` with verdict `event` and `body`.
    pub async fn submit_review(&self, number: u64, event: ReviewEvent, body: &str) -> Result<()> {
        self.post(
            &format!("pulls/{number}/reviews"),
            &serde_json::json!({ "event": event.as_str(), "body": body }),
        )
        .await?;
        Ok(())
    }

    /// Creates branch `branch` at the head of `from`.
    pub async fn create_branch(&self, branch: &str, from: &str) -> Result<()> {
        self.post(
            "branches",
            &serde_json::json!({ "new_branch_name": branch, "old_ref_name": from }),
        )
        .await?;
        Ok(())
    }

    /// Commits a new file `path` with `content` to `branch`, as the identity of the suite's
    /// persona: the Git proxy rejects the push of a commit by anyone else (`df:identity`), so a
    /// branch that agents rebase must hold such commits.
    pub async fn create_file(
        &self,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Result<()> {
        let person = serde_json::json!({
            "name": data::PERSONA_NAME,
            "email": data::PERSONA_EMAIL,
        });
        self.post(
            &format!("contents/{path}"),
            &serde_json::json!({
                "branch": branch,
                "message": message,
                "content": STANDARD.encode(content),
                "author": person,
                "committer": person,
            }),
        )
        .await?;
        Ok(())
    }

    /// Opens a pull request from `head` into `base`; a draft has the `WIP: ` title prefix that
    /// Forgejo recognizes. Returns its number.
    pub async fn open_pull(&self, head: &str, base: &str, title: &str, draft: bool) -> Result<u64> {
        let title = if draft {
            format!("WIP: {title}")
        } else {
            title.to_owned()
        };
        let pull = self
            .post(
                "pulls",
                &serde_json::json!({ "head": head, "base": base, "title": title }),
            )
            .await?;
        pull.get("number")
            .and_then(Value::as_u64)
            .context("pull request number")
    }

    /// Reports commit status `state` (`success`, `pending`, `failure`) for check `context` on commit
    /// `sha`, as a CI system does; `target_url` is the check's page.
    pub async fn create_commit_status(
        &self,
        sha: &str,
        state: &str,
        context: &str,
        target_url: &str,
    ) -> Result<()> {
        self.post(
            &format!("statuses/{sha}"),
            &serde_json::json!({ "state": state, "context": context, "target_url": target_url }),
        )
        .await?;
        Ok(())
    }

    /// Closes pull request `number` without merging it.
    pub async fn close_pull(&self, number: u64) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.repo_path(&format!("pulls/{number}")),
            Some(&serde_json::json!({ "state": "closed" })),
            &[],
        )
        .await?;
        Ok(())
    }

    /// Takes pull request `number` out of draft by retitling it (Forgejo refuses to merge a
    /// draft).
    pub async fn retitle_pull(&self, number: u64, title: &str) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.repo_path(&format!("pulls/{number}")),
            Some(&serde_json::json!({ "title": title })),
            &[],
        )
        .await?;
        Ok(())
    }

    /// Merges pull request `number` with a merge commit. Forgejo answers 405 ("Please try again
    /// later") while it re-checks whether the pull request can be merged, for example right after
    /// another test moved the base branch, so a failed merge is retried until [`MERGE_TIMEOUT`].
    /// The error names Forgejo's last answer, body included (unlike other calls: it is the
    /// reason for the retry).
    pub async fn merge_pull(&self, number: u64) -> Result<()> {
        let path = self.repo_path(&format!("pulls/{number}/merge"));
        eventually(
            &format!("the merge of pull request {number}"),
            MERGE_TIMEOUT,
            || async {
                let response = self
                    .send(
                        reqwest::Method::POST,
                        &path,
                        Some(&serde_json::json!({ "Do": "merge" })),
                    )
                    .await?;
                let status = response.status();
                if status.is_success() {
                    return Ok(Some(()));
                }
                let body = response.text().await.unwrap_or_default();
                bail!("POST {path}: {status}: {}", body.trim())
            },
        )
        .await
    }

    /// Content of `path` on `branch`.
    pub async fn file(&self, branch: &str, path: &str) -> Result<String> {
        let response = self.get(&format!("raw/{path}?ref={branch}"), &[]).await?;
        response
            .text()
            .await
            .with_context(|| format!("read {path} on {branch}"))
    }

    // Setup calls: what `setup forgejo` needs to prepare an instance for the suite.

    /// Whether user `name` exists.
    pub async fn user_exists(&self, name: &str) -> Result<bool> {
        let response = self
            .call(reqwest::Method::GET, &format!("users/{name}"), None, &[404])
            .await?;
        Ok(response.status().is_success())
    }

    /// Creates user `name` with `password` (admin only); the password need not be changed.
    pub async fn create_user(&self, name: &str, password: &str, email: &str) -> Result<()> {
        let body = serde_json::json!({
            "username": name,
            "password": password,
            "email": email,
            "must_change_password": false,
        });
        self.call(reqwest::Method::POST, "admin/users", Some(&body), &[])
            .await?;
        Ok(())
    }

    /// Deletes access token `token_name` of `user` (authenticated as that user); a missing token
    /// is fine.
    pub async fn delete_token(&self, user: &str, token_name: &str) -> Result<()> {
        let path = format!("users/{user}/tokens/{token_name}");
        self.call(reqwest::Method::DELETE, &path, None, &[404])
            .await?;
        Ok(())
    }

    /// Creates access token `token_name` with all scopes for `user` (authenticated as that user)
    /// and returns it.
    pub async fn create_token(&self, user: &str, token_name: &str) -> Result<String> {
        let path = format!("users/{user}/tokens");
        let body = serde_json::json!({ "name": token_name, "scopes": ["all"] });
        let created: Value = self
            .call(reqwest::Method::POST, &path, Some(&body), &[])
            .await?
            .json()
            .await
            .with_context(|| format!("decode POST {path}"))?;
        created
            .get("sha1")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("POST {path}: the response has no token"))
    }

    /// Creates repository `name` of the authenticated user, with an initial commit on `main`.
    pub async fn create_repository(&self, name: &str) -> Result<()> {
        let body = serde_json::json!({
            "name": name,
            "auto_init": true,
            "default_branch": data::BASE_BRANCH,
        });
        self.call(reqwest::Method::POST, "user/repos", Some(&body), &[])
            .await?;
        Ok(())
    }

    /// Deletes repository `name` of the owner; a missing repository is fine.
    pub async fn delete_repository(&self, name: &str) -> Result<()> {
        let path = format!("repos/{}/{name}", self.owner);
        self.call(reqwest::Method::DELETE, &path, None, &[404])
            .await?;
        Ok(())
    }

    /// Gives `user` write access to the repository.
    pub async fn add_collaborator(&self, user: &str) -> Result<()> {
        let body = serde_json::json!({ "permission": "write" });
        self.call(
            reqwest::Method::PUT,
            &self.repo_path(&format!("collaborators/{user}")),
            Some(&body),
            &[],
        )
        .await?;
        Ok(())
    }

    /// Adds a webhook of type `forgejo` that posts `events` to `url`, signed with `secret`.
    pub async fn create_webhook(&self, url: &str, secret: &str, events: &[&str]) -> Result<()> {
        let body = serde_json::json!({
            "type": "forgejo",
            "active": true,
            "events": events,
            "config": { "url": url, "content_type": "json", "secret": secret },
        });
        self.call(
            reqwest::Method::POST,
            &self.repo_path("hooks"),
            Some(&body),
            &[],
        )
        .await?;
        Ok(())
    }

    fn build(base_url: &str, auth: Auth) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build the Forgejo HTTP client")?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth,
            owner: data::FORGE_OWNER.to_owned(),
            repository: data::FORGE_REPO.to_owned(),
        })
    }

    /// `path` below `repos/{owner}/{repository}/`, relative to `/api/v1/`.
    fn repo_path(&self, path: &str) -> String {
        format!("repos/{}/{}/{path}", self.owner, self.repository)
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get(path, &[])
            .await?
            .json()
            .await
            .with_context(|| format!("decode GET {path}"))
    }

    /// GET of a repository path.
    async fn get(&self, path: &str, tolerated: &[u16]) -> Result<reqwest::Response> {
        self.call(reqwest::Method::GET, &self.repo_path(path), None, tolerated)
            .await
    }

    /// POST of a repository path; returns the JSON answer.
    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let path = self.repo_path(path);
        self.call(reqwest::Method::POST, &path, Some(body), &[])
            .await?
            .json()
            .await
            .with_context(|| format!("decode POST {path}"))
    }

    /// Sends `method` to `path` below `/api/v1/` and returns the response whatever its status.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/api/v1/{path}", self.base_url);
        let request = self.http.request(method.clone(), url);
        let mut request = match &self.auth {
            Auth::Token(token) => request.header("Authorization", format!("token {token}")),
            Auth::Basic { user, password } => request.basic_auth(user, Some(password)),
        };
        if let Some(body) = body {
            request = request.json(body);
        }
        request
            .send()
            .await
            .with_context(|| format!("{method} {path}"))
    }

    /// Like [`Self::send`], but fails unless the status is a success or one of `tolerated`. The
    /// error names method, path and status, never the body (it may echo a token).
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        tolerated: &[u16],
    ) -> Result<reqwest::Response> {
        let response = self.send(method.clone(), path, body).await?;
        let status = response.status();
        if !status.is_success() && !tolerated.contains(&status.as_u16()) {
            bail!("{method} {path}: HTTP {}", status.as_u16());
        }
        Ok(response)
    }
}

/// How a [`Forgejo`] client authenticates: with an access token (the tests) or as a user with a
/// password (the setup, which creates the tokens).
#[derive(Debug, Clone)]
enum Auth {
    Token(String),
    Basic { user: String, password: String },
}

#[cfg(test)]
mod tests;
