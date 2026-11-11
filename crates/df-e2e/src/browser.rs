//! The browser of the suite: a Playwright server in the cluster, and the login at the identity
//! provider.

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use playwright_rs::protocol::Page;
use playwright_rs::{Playwright, expect, expect_page};

use crate::env::E2eEnv;

/// How long a page waits for a card that already exists in the database.
const CARD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the login waits to land on the UI. The first logins after the cluster starts take
/// longer than the default 5 s (Dex password check, code exchange, first user upsert).
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long starting the driver, connecting to the Playwright server and opening a page may take.
/// Without a bound a server that stops answering (for example a restarted pod) blocks a test
/// forever instead of failing it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// A page of the remote browser. Keeps the local Playwright driver alive.
pub struct Session {
    _playwright: Playwright,
    pub page: Page,
}

impl Session {
    /// Connects to the Playwright server and opens a page.
    pub async fn start(env: &E2eEnv) -> Result<Self> {
        let playwright = within(
            "start the local Playwright driver (`just e2e-local` installs it)",
            Playwright::launch(),
        )
        .await?;
        let browser = within(
            &format!("connect to {}", env.playwright_ws),
            playwright.chromium().connect(&env.playwright_ws, None),
        )
        .await?;
        let page = within("open a page", browser.new_page()).await?;
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
            .with_timeout(LOGIN_TIMEOUT)
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
    /// `instance`) once its card shows `summary`; `None` for a type with a single scope (no
    /// choice on the card). Returns once the card no longer offers the approval, so a following
    /// navigation cannot cancel the request. The decided card leaves the live inbox, so callers
    /// check the outcome elsewhere.
    pub async fn approve_request(
        &self,
        env: &E2eEnv,
        id: &str,
        summary: &str,
        scope: Option<&str>,
    ) -> Result<()> {
        self.page
            .goto(&format!("{}/requests", env.base_url), None)
            .await
            .context("open the requests inbox")?;
        let card = format!("#request-{id}");
        self.expect_region_text(&card, summary, CARD_TIMEOUT)
            .await?;
        if let Some(scope) = scope {
            self.page
                .locator(format!("{card} input[name=scope][value={scope}]"))
                .click(None)
                .await
                .with_context(|| format!("choose scope {scope}"))?;
        }
        let approve = self
            .page
            .locator(format!("{card} button:has-text('Approve')"));
        approve.click(None).await.context("approve the request")?;
        expect(approve)
            .with_timeout(CARD_TIMEOUT)
            .to_be_hidden()
            .await
            .context("wait for the approval to be recorded")
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

    /// Sends `message` to the open task with Ctrl+Enter in the message box, pressed twice in a
    /// row: the second press must not send it again.
    pub async fn send_message_with_shortcut(&self, message: &str) -> Result<()> {
        let input = self.page.locator("#message");
        input
            .fill(message, None)
            .await
            .context("fill the message")?;
        for _ in 0..2 {
            input
                .press("Control+Enter", None)
                .await
                .context("press Ctrl+Enter in the message box")?;
        }
        Ok(())
    }

    /// Interrupts the running turn of the open task with the "Interrupt" button of the message
    /// box, which shows while the box is empty.
    pub async fn interrupt_turn(&self) -> Result<()> {
        self.page
            .locator("#composer-action button[aria-label='Interrupt']")
            .click(None)
            .await
            .context("interrupt the turn")
    }

    /// Stops the active run of the open task with the "Stop run" button of its run card.
    pub async fn stop_run(&self) -> Result<()> {
        self.open_runs().await?;
        self.page
            .locator("#runs button:has-text('Stop run')")
            .first()
            .click(None)
            .await
            .context("stop the run")
    }

    /// Cancels the open task: Status menu, "Cancel task…", then the confirmation.
    pub async fn cancel_task(&self) -> Result<()> {
        self.page
            .locator("#task-details button[aria-haspopup=true]")
            .click(None)
            .await
            .context("open the status menu")?;
        self.page
            .locator("#task-details button:has-text('Cancel task…')")
            .click(None)
            .await
            .context("choose Cancel task…")?;
        self.page
            .locator("dialog[open] button:has-text('Cancel task')")
            .click(None)
            .await
            .context("confirm the cancellation")
    }

    /// Opens the closed "Runs" disclosure of the open task.
    pub async fn open_runs(&self) -> Result<()> {
        self.page
            .locator("#runs summary")
            .click(None)
            .await
            .context("open the runs")
    }

    /// Opens the closed "Dependencies" disclosure of the open task (it is open only while the
    /// task awaits its dependencies).
    pub async fn open_dependencies(&self) -> Result<()> {
        self.page
            .locator("#dependencies summary")
            .click(None)
            .await
            .context("open the dependencies")
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

/// Runs `step` of the browser setup with [`CONNECT_TIMEOUT`], so that a server that does not
/// answer fails the test with `what` instead of blocking it.
async fn within<T, E>(what: &str, step: impl Future<Output = Result<T, E>>) -> Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    within_for(CONNECT_TIMEOUT, what, step).await
}

async fn within_for<T, E>(
    limit: Duration,
    what: &str,
    step: impl Future<Output = Result<T, E>>,
) -> Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    tokio::time::timeout(limit, step)
        .await
        .map_err(|_elapsed| anyhow!("{what}: no answer within {limit:?}"))?
        .with_context(|| what.to_owned())
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use super::{within, within_for};

    #[tokio::test]
    async fn a_setup_step_that_never_answers_fails_with_its_description() {
        let step = pending::<Result<(), std::io::Error>>();

        let error = within_for(
            Duration::from_millis(10),
            "connect to ws://playwright",
            step,
        )
        .await
        .expect_err("the step times out");

        assert!(
            error
                .to_string()
                .starts_with("connect to ws://playwright: no answer within"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_failing_setup_step_keeps_its_error_with_the_description() {
        let step = async { Err::<(), _>(std::io::Error::other("refused")) };

        let error = within("open a page", step)
            .await
            .expect_err("the step fails");

        assert_eq!(format!("{error:#}"), "open a page: refused");
    }
}
