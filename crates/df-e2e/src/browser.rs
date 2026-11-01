//! The browser of the suite: a Playwright server in the cluster, and the login at the identity
//! provider.

use std::time::Duration;

use anyhow::{Context, Result};
use playwright_rs::protocol::Page;
use playwright_rs::{Playwright, expect, expect_page};

use crate::env::E2eEnv;

/// How long a page waits for a card that already exists in the database.
const CARD_TIMEOUT: Duration = Duration::from_secs(30);

/// A page of the remote browser. Keeps the local Playwright driver alive.
pub struct Session {
    _playwright: Playwright,
    pub page: Page,
}

impl Session {
    /// Connects to the Playwright server and opens a page.
    pub async fn start(env: &E2eEnv) -> Result<Self> {
        let playwright = Playwright::launch()
            .await
            .context("start the local Playwright driver (`just e2e-local` installs it)")?;
        let browser = playwright
            .chromium()
            .connect(&env.playwright_ws, None)
            .await
            .with_context(|| format!("connect to {}", env.playwright_ws))?;
        let page = browser.new_page().await.context("open a page")?;
        Ok(Self {
            _playwright: playwright,
            page,
        })
    }

    /// Opens the UI and logs in at Dex; returns when the board is shown.
    pub async fn login(&self, env: &E2eEnv) -> Result<()> {
        self.page
            .goto(&env.base_url, None)
            .await
            .context("open the UI")?;
        self.page
            .locator("input[name=login]")
            .fill(&env.user, None)
            .await
            .context("fill the user")?;
        self.page
            .locator("input[name=password]")
            .fill(&env.password, None)
            .await
            .context("fill the password")?;
        self.page
            .locator("button[type=submit]")
            .click(None)
            .await
            .context("submit the login")?;
        expect_page(&self.page)
            .to_have_url_regex(&format!("^{}", regex::escape(&env.base_url)))
            .await
            .context("land on the UI after the login")
    }
}

impl Session {
    /// Creates a task of project `project_key` for the only persona through the composer, linked
    /// to issue `issue` when given. Returns its key once the UI has opened the task.
    pub async fn create_task(
        &self,
        env: &E2eEnv,
        project_key: &str,
        description: &str,
        issue: Option<u64>,
    ) -> Result<String> {
        self.page
            .goto(
                &format!("{}/tasks?project={project_key}", env.base_url),
                None,
            )
            .await
            .context("open the task list")?;
        self.page
            .locator("button[hx-get^='/tasks/compose']:visible")
            .first()
            .click(None)
            .await
            .context("open the composer")?;
        if let Some(issue) = issue {
            self.page
                .locator("button[aria-label='Link issue']")
                .click(None)
                .await
                .context("open the issue link")?;
            self.page
                .locator("#issue_number")
                .fill(&issue.to_string(), None)
                .await
                .context("fill the issue number")?;
        }
        self.page
            .locator("#description")
            .fill(description, None)
            .await
            .context("fill the description")?;
        self.page
            .locator("button[aria-label='Create task']")
            .click(None)
            .await
            .context("create the task")?;
        expect_page(&self.page)
            .to_have_url_regex(&format!("/tasks/{project_key}-[0-9]+$"))
            .await
            .context("land on the new task")?;
        let url = self.page.url();
        url.rsplit('/')
            .next()
            .map(str::to_owned)
            .with_context(|| format!("task key in {url}"))
    }

    /// Opens the page of task `key`.
    pub async fn open_task(&self, env: &E2eEnv, key: &str) -> Result<()> {
        self.page
            .goto(&format!("{}/tasks/{key}", env.base_url), None)
            .await
            .with_context(|| format!("open task {key}"))?;
        Ok(())
    }

    /// Approves request `id` in the `/requests` inbox for `scope` (`task`, `project` or
    /// `instance`) once its card shows `summary`. The decided card leaves the live inbox, so
    /// callers check the outcome elsewhere.
    pub async fn approve_request(
        &self,
        env: &E2eEnv,
        id: &str,
        summary: &str,
        scope: &str,
    ) -> Result<()> {
        self.page
            .goto(&format!("{}/requests", env.base_url), None)
            .await
            .context("open the requests inbox")?;
        let card = format!("#request-{id}");
        self.expect_region_text(&card, summary, CARD_TIMEOUT)
            .await?;
        self.page
            .locator(format!("{card} input[name=scope][value={scope}]"))
            .click(None)
            .await
            .with_context(|| format!("choose scope {scope}"))?;
        self.page
            .locator(format!("{card} button:has-text('Approve')"))
            .click(None)
            .await
            .context("approve the request")
    }

    /// Sends `message` to the open task.
    pub async fn send_message(&self, message: &str) -> Result<()> {
        self.page
            .locator("#message")
            .fill(message, None)
            .await
            .context("fill the message")?;
        self.page
            .locator("button[aria-label='Send']")
            .click(None)
            .await
            .context("send the message")
    }

    /// Opens the closed "Runs" disclosure of the open task.
    pub async fn open_runs(&self) -> Result<()> {
        self.page
            .locator("#runs summary")
            .click(None)
            .await
            .context("open the runs")
    }

    /// Waits until the element `selector` of the open task contains `text`.
    pub async fn expect_region_text(
        &self,
        selector: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<()> {
        expect(self.page.locator(selector))
            .with_timeout(timeout)
            .to_contain_text(text)
            .await
            .with_context(|| format!("wait for `{text}` in {selector}"))
    }

    /// The rendered text of the element `selector` of the open task.
    pub async fn region_text(&self, selector: &str) -> Result<String> {
        self.page
            .locator(selector)
            .inner_text()
            .await
            .with_context(|| format!("text of {selector}"))
    }

    /// Waits until the open task shows `text` (a transcript line, a status).
    pub async fn expect_text(&self, text: &str, timeout: Duration) -> Result<()> {
        expect(self.page.get_by_text(text, false).first())
            .with_timeout(timeout)
            .to_be_visible()
            .await
            .with_context(|| format!("wait for `{text}` in the UI"))
    }
}
