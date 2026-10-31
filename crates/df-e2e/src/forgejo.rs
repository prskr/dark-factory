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

    async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self.get(path).await?.error_for_status()?;
        response
            .json()
            .await
            .with_context(|| format!("decode GET {path}"))
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/{path}",
            self.base_url,
            data::FORGE_OWNER,
            data::FORGE_REPO
        );
        self.http
            .get(&url)
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await
            .with_context(|| format!("GET {url}"))
    }
}
