//! The few Forgejo API calls the tests use to check what the UI does not show.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::data;

/// Bounds each API call, so a stalled port-forward fails the test instead of hanging it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Forgejo API client for the repository `factory/demo`, authenticated with the bot token.
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
