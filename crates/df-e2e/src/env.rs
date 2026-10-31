//! Targets of the suite, read from the environment.

use anyhow::{Context, Result};

/// Where the instance, the browser, the database and Forgejo are.
#[derive(Debug, Clone)]
pub struct E2eEnv {
    /// URL of the web UI as the browser reaches it (`E2E_BASE_URL`).
    pub base_url: String,
    /// WebSocket endpoint of the Playwright server (`E2E_PLAYWRIGHT_WS`).
    pub playwright_ws: String,
    /// Database of the instance (`E2E_DATABASE_URL`).
    pub database_url: String,
    /// Forgejo API base as the test host reaches it (`E2E_FORGEJO_URL`).
    pub forgejo_url: String,
    /// Forgejo token of the bot user (`E2E_FORGEJO_TOKEN`).
    pub forgejo_token: String,
    /// Login at the identity provider (`E2E_USER`, `E2E_PASSWORD`).
    pub user: String,
    pub password: String,
}

impl E2eEnv {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            base_url: required("E2E_BASE_URL")?,
            playwright_ws: required("E2E_PLAYWRIGHT_WS")?,
            database_url: required("E2E_DATABASE_URL")?,
            forgejo_url: required("E2E_FORGEJO_URL")?,
            forgejo_token: required("E2E_FORGEJO_TOKEN")?,
            user: required("E2E_USER")?,
            password: required("E2E_PASSWORD")?,
        })
    }
}

fn required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("environment variable {name} is not set"))
}
