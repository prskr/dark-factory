//! The few Forgejo API calls the tests use to check what the UI does not show.

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

/// Forgejo API client for the repository `factory/demo`, authenticated with the token it is built
/// with (the bot's or the reviewer's).
#[derive(Debug, Clone)]
pub struct Forgejo {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Forgejo {
    pub fn new(base_url: &str, token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build the Forgejo HTTP client")?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        })
    }

    /// Whether `branch` exists in the repository.
    pub async fn branch_exists(&self, branch: &str) -> Result<bool> {
        let response = self.get(&format!("branches/{branch}")).await?;
        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            status => anyhow::bail!("GET branches/{branch}: HTTP {status}"),
        }
    }

    /// All pull requests (open and closed) of the repository, newest first.
    pub async fn pulls(&self) -> Result<Vec<Value>> {
        self.json("pulls?state=all&limit=50").await
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

    /// Submits a review of pull request `number` with verdict `event` (`APPROVED`,
    /// `REQUEST_CHANGES` or `COMMENT`) and `body`.
    pub async fn submit_review(&self, number: u64, event: &str, body: &str) -> Result<()> {
        self.post(
            &format!("pulls/{number}/reviews"),
            &serde_json::json!({ "event": event, "body": body }),
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

    /// Commits a new file `path` with `content` to `branch`, as the commit identity of the
    /// instance: the Git proxy rejects the push of a commit by anyone else (`df:identity`), so a
    /// branch that agents rebase must hold such commits.
    pub async fn create_file(
        &self,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Result<()> {
        let identity = df_core::config::CommitIdentityConfig::default();
        let person = serde_json::json!({ "name": identity.name, "email": identity.email });
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

    /// Takes pull request `number` out of draft by retitling it (Forgejo refuses to merge a
    /// draft).
    pub async fn retitle_pull(&self, number: u64, title: &str) -> Result<()> {
        let url = self.url(&format!("pulls/{number}"));
        self.http
            .patch(&url)
            .header("Authorization", format!("token {}", self.token))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .with_context(|| format!("PATCH {url}"))?
            .error_for_status()?;
        Ok(())
    }

    /// Merges pull request `number` with a merge commit. Forgejo answers 405 ("Please try again
    /// later") while it re-checks whether the pull request can be merged, for example right after
    /// another test moved the base branch, so a failed merge is retried until [`MERGE_TIMEOUT`].
    /// The error names Forgejo's last answer.
    pub async fn merge_pull(&self, number: u64) -> Result<()> {
        let url = self.url(&format!("pulls/{number}/merge"));
        eventually(
            &format!("the merge of pull request {number}"),
            MERGE_TIMEOUT,
            || async {
                let response = self
                    .http
                    .post(&url)
                    .header("Authorization", format!("token {}", self.token))
                    .json(&serde_json::json!({ "Do": "merge" }))
                    .send()
                    .await
                    .with_context(|| format!("POST {url}"))?;
                let status = response.status();
                if status.is_success() {
                    return Ok(Some(()));
                }
                let body = response.text().await.unwrap_or_default();
                bail!("POST {url}: {status}: {}", body.trim())
            },
        )
        .await
    }

    /// Content of `path` on `branch`.
    pub async fn file(&self, branch: &str, path: &str) -> Result<String> {
        let response = self
            .get(&format!("raw/{path}?ref={branch}"))
            .await?
            .error_for_status()?;
        response
            .text()
            .await
            .with_context(|| format!("read {path} on {branch}"))
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self.get(path).await?.error_for_status()?;
        response
            .json()
            .await
            .with_context(|| format!("decode GET {path}"))
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        let url = self.url(path);
        self.http
            .get(&url)
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await
            .with_context(|| format!("GET {url}"))
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = self.url(path);
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("token {}", self.token))
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?
            .error_for_status()?;
        response
            .json()
            .await
            .with_context(|| format!("decode POST {path}"))
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/api/v1/repos/{}/{}/{path}",
            self.base_url,
            data::FORGE_OWNER,
            data::FORGE_REPO
        )
    }
}
