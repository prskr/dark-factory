//! The browser of the suite: a Playwright server in the cluster, and the login at the identity
//! provider.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use playwright_rs::protocol::{
    AriaRole, BrowserContext, BrowserContextOptions, FetchOptions, GetByRoleOptions, Locator, Page,
    StorageState,
};
use playwright_rs::{Playwright, expect, expect_page};

use crate::env::E2eEnv;
use crate::wait::{UI_TIMEOUT, eventually};

/// How long taking the screenshot of a failed test may take.
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the teardown of a test waits for one cancellation through the UI.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a page waits for a card that already exists in the database.
const CARD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the login waits to land on the UI. The first logins after the cluster starts take
/// longer than the default 5 s (Dex password check, code exchange, first user upsert).
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long starting the driver, connecting to the Playwright server and opening a page may take.
/// Without a bound a server that stops answering (for example a restarted pod) blocks a test
/// forever instead of failing it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// The login the tests of this process share. Every `#[tokio::test]` has a runtime of its own, and
/// tokio's mutex does not depend on one.
static SHARED_LOGIN: tokio::sync::Mutex<SharedLogin> = tokio::sync::Mutex::const_new(SharedLogin {
    state: None,
    generation: 0,
});

/// The storage state after the last login at Dex, and how many logins stored one.
struct SharedLogin {
    state: Option<StorageState>,
    generation: u64,
}

/// The emulated device of [`Session::start_logged_in_tablet`].
#[derive(Debug, Clone, Copy)]
struct Tablet {
    width: u32,
    height: u32,
}

/// A task creation that was submitted without the UI opening the task.
#[derive(Debug, Clone)]
pub struct UnlandedTask {
    pub project: String,
    /// The description as submitted, with the suffix of the test: no other test submits it.
    pub description: String,
    /// Just before the submit.
    pub submitted_at: SystemTime,
}

/// A page of the remote browser in a context of its own. Keeps the local Playwright driver alive.
/// Every task the session creates through the composer is recorded, so the fixture can cancel what
/// a test leaves behind.
pub struct Session {
    _playwright: Playwright,
    context: BrowserContext,
    pub page: Page,
    created: Mutex<Vec<String>>,
    /// Creations that were submitted but whose page never opened: the task may exist, the fixture
    /// looks it up.
    unlanded: Mutex<Vec<UnlandedTask>>,
    cookie: Mutex<Option<String>>,
    /// Appended to every description the session submits, see [`Session::set_description_suffix`].
    description_suffix: OnceLock<String>,
}

impl Session {
    /// Connects to the Playwright server and opens a page in a new context.
    pub async fn start(env: &E2eEnv) -> Result<Self> {
        Self::open(env, None, None).await
    }

    /// Like [`Session::start`] followed by [`Session::login`], but the Dex form is submitted at
    /// most once per process while the session stays valid: the first call logs in and keeps the
    /// storage state (cookies) of its context, later calls open their own context with it. A
    /// context that still lands on the login form (the session expired or was removed) logs in
    /// again and replaces the stored state. Tests that start together wait for that one login.
    pub async fn start_logged_in(env: &E2eEnv) -> Result<Self> {
        Self::start_logged_in_on(env, None).await
    }

    /// [`Session::start_logged_in`] on a tablet with only a touch screen (`hasTouch`, `isMobile`)
    /// of `width` x `height` pixels: a second viewer of the same user. The caller closes its page.
    pub async fn start_logged_in_tablet(env: &E2eEnv, width: u32, height: u32) -> Result<Self> {
        Self::start_logged_in_on(env, Some(Tablet { width, height })).await
    }

    async fn start_logged_in_on(env: &E2eEnv, tablet: Option<Tablet>) -> Result<Self> {
        let (stored, generation) = {
            let shared = SHARED_LOGIN.lock().await;
            (shared.state.clone(), shared.generation)
        };
        let has_stored_state = stored.is_some();
        let mut session = Box::pin(Self::open(env, stored, tablet)).await?;
        if has_stored_state && session.try_stored_login(env).await? {
            return Ok(session);
        }
        // Held across the login on purpose (the one exception to the rule against holding a lock
        // over an await, CODING_STANDARDS.md "Concurrency"): a test that starts meanwhile needs
        // exactly its result. Every step under the lock is bounded on the client by `within`/`within_for`
        // (`CONNECT_TIMEOUT`, `UI_TIMEOUT`) or, for the landing, by `LOGIN_TIMEOUT`. A stuck login
        // fails the test that runs it and releases the lock.
        let mut shared = SHARED_LOGIN.lock().await;
        if shared.generation != generation {
            let newer = Box::pin(Self::open(env, shared.state.clone(), tablet)).await?;
            if newer.try_stored_login(env).await? {
                return Ok(newer);
            }
            session = newer;
        }
        if has_stored_state {
            report(
                "login",
                "the stored login is no longer accepted, logging in again",
            );
        }
        session.login(env).await?;
        shared.state = Some(
            within(
                "read the storage state of the login",
                session.context.storage_state(None),
            )
            .await?,
        );
        shared.generation += 1;
        Ok(session)
    }

    async fn open(
        env: &E2eEnv,
        state: Option<StorageState>,
        tablet: Option<Tablet>,
    ) -> Result<Self> {
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
        let context = if state.is_none() && tablet.is_none() {
            within("open a browser context", browser.new_context()).await?
        } else {
            let mut options = BrowserContextOptions::builder();
            if let Some(state) = state {
                options = options.storage_state(state);
            }
            if let Some(Tablet { width, height }) = tablet {
                options = options
                    .viewport(playwright_rs::Viewport { width, height })
                    .has_touch(true)
                    .is_mobile(true);
            }
            within(
                "open a browser context with the stored login or the emulated device",
                browser.new_context_with_options(options.build()),
            )
            .await?
        };
        let page = within("open a page", context.new_page()).await?;
        Ok(Self {
            _playwright: playwright,
            context,
            page,
            created: Mutex::new(Vec::new()),
            unlanded: Mutex::new(Vec::new()),
            cookie: Mutex::new(None),
            description_suffix: OnceLock::new(),
        })
    }

    /// Opens the UI and logs in at Dex; returns when the board is shown.
    pub async fn login(&self, env: &E2eEnv) -> Result<()> {
        report("login", "submitting the Dex login form");
        within_for(
            UI_TIMEOUT,
            "open the UI",
            self.page.goto(&env.base_url, None),
        )
        .await?;
        within_for(
            UI_TIMEOUT,
            "fill the user",
            self.page.locator("input[name=login]").fill(&env.user, None),
        )
        .await?;
        within_for(
            UI_TIMEOUT,
            "fill the password",
            self.page
                .locator("input[name=password]")
                .fill(&env.password, None),
        )
        .await?;
        // Dex's form has a single submit button; its label is Dex's, not the application's.
        within_for(
            UI_TIMEOUT,
            "submit the login",
            self.page.locator("button[type=submit]").click(None),
        )
        .await?;
        expect_page(&self.page)
            .with_timeout(LOGIN_TIMEOUT)
            .to_have_url_regex(&format!("^{}", regex::escape(&env.base_url)))
            .await
            .context("land on the UI after the login")?;
        self.remember_cookie().await
    }

    /// Kept for the fallback of the teardown, which must work when the browser does not answer.
    async fn remember_cookie(&self) -> Result<()> {
        let cookie = self.read_cookie_header().await?;
        *self.cookie.lock().unwrap_or_else(PoisonError::into_inner) = Some(cookie);
        Ok(())
    }

    /// Opens the UI with the stored login and reports whether it is accepted; if so, keeps the
    /// cookie header of the session for the teardown.
    async fn try_stored_login(&self, env: &E2eEnv) -> Result<bool> {
        if !self.opens_the_ui(env).await? {
            return Ok(false);
        }
        self.remember_cookie().await?;
        Ok(true)
    }

    /// Opens the UI and reports whether it answered instead of redirecting to the login form.
    async fn opens_the_ui(&self, env: &E2eEnv) -> Result<bool> {
        let response = within_for(
            UI_TIMEOUT,
            "open the UI with the stored login",
            self.page.goto(&env.base_url, None),
        )
        .await?;
        check_ui_status(response.map(|r| r.status()))?;
        Ok(lands_on_the_ui(&self.page.url(), &env.base_url))
    }

    /// The keys of the tasks created through [`Session::create_task`] and its variants, oldest
    /// first.
    pub fn created_tasks(&self) -> Vec<String> {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Makes every description this session submits end in ` [suffix]` (once; the fixture sets the
    /// suffix of the test), so no other test submits the same text.
    pub fn set_description_suffix(&self, suffix: &str) {
        self.description_suffix.set(suffix.to_owned()).ok();
    }

    /// The creations whose task page never opened.
    pub fn unlanded_tasks(&self) -> Vec<UnlandedTask> {
        self.unlanded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The `Cookie` header of the session as it was right after [`Session::login`].
    pub fn cookie_header(&self) -> Option<String> {
        self.cookie
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    async fn read_cookie_header(&self) -> Result<String> {
        let cookies = within("read the session cookies", self.context.cookies(None)).await?;
        Ok(cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; "))
    }

    /// Cancels task `key` through the UI in a fresh page of the context, so a page that a failed
    /// test left in a dialog or on another site does not matter.
    pub async fn cancel_task_in_new_page(&self, env: &E2eEnv, key: &str) -> Result<()> {
        let cancel = async {
            let page = self.context.new_page().await.context("open a page")?;
            let cancelled = async {
                page.goto(&format!("{}/tasks/{key}", env.base_url), None)
                    .await
                    .with_context(|| format!("open task {key}"))?;
                cancel_task_on(&page).await?;
                // Closing the page right after the click would cancel the request in flight.
                expect(page.locator("#task-details"))
                    .with_timeout(CANCEL_TIMEOUT)
                    .to_contain_text("cancelled")
                    .await
                    .context("the task page shows the cancellation")
            }
            .await;
            // Best effort: a page that stays open keeps its event streams, and the browser allows
            // few connections per host.
            page.close().await.ok();
            cancelled
        };
        tokio::time::timeout(CANCEL_TIMEOUT, cancel)
            .await
            .map_err(|_elapsed| anyhow!("cancel {key}: no answer within {CANCEL_TIMEOUT:?}"))?
    }

    /// Closes the page of the session, which ends its event streams. The browser allows few
    /// connections per host (HTTP/1.1), and every open task page holds several streams: a second
    /// page next to it would wait for a connection, a cancellation included.
    pub async fn close_page(&self) {
        // Best effort: the browser may be gone already.
        within_for(CANCEL_TIMEOUT, "close the page", self.page.close())
            .await
            .ok();
    }

    /// Writes what explains a failure into `dir`: `screenshot.png` of the page and `console.txt`.
    /// Every step is bounded and independent: a step that fails is reported in the returned list
    /// and does not keep the other from running.
    ///
    /// There is no Playwright trace: `Tracing::stop` with a path makes the browser server write
    /// the file on its own disk (`Artifact::save_as` of playwright-rs does not stream it to a
    /// remote client), so with the browser in the cluster the test host never gets it.
    pub async fn save_diagnostics(&self, dir: &Path) -> Vec<String> {
        let mut failures = Vec::new();
        if let Err(e) = std::fs::create_dir_all(dir) {
            return vec![format!("create {}: {e}", dir.display())];
        }
        let console = self.console_log();
        if let Err(e) = std::fs::write(dir.join("console.txt"), console) {
            failures.push(format!("console.txt: {e}"));
        }
        let shot = within_for(
            DIAGNOSTICS_TIMEOUT,
            "screenshot",
            self.page
                .screenshot_to_file(&dir.join("screenshot.png"), None),
        )
        .await;
        if let Err(e) = shot {
            failures.push(format!("screenshot.png: {e:#}"));
        }
        failures
    }

    /// The console messages and page errors the page has accumulated (a test that clears them
    /// drops the earlier ones), one per line.
    pub fn console_log(&self) -> String {
        let messages = self
            .page
            .console_messages()
            .into_iter()
            .map(|message| format!("[{}] {}", message.type_(), message.text()));
        let errors = self
            .page
            .page_errors()
            .into_iter()
            .map(|error| format!("[pageerror] {error}"));
        messages.chain(errors).collect::<Vec<_>>().join("\n")
    }

    fn record_created(&self, key: &str) {
        self.created
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(key.to_owned());
    }

    /// Creates a task of project `project_key` for the only persona through the composer, linked
    /// to issue `issue` when given. Returns its key once the UI has opened the task.
    pub async fn create_task(
        &self,
        env: &E2eEnv,
        project_key: &str,
        description: &str,
        issue: Option<u64>,
    ) -> Result<String> {
        self.compose_task(env, project_key, description, issue, None, None)
            .await
    }

    /// [`Session::create_task`] with the persona that the composer lists as `persona` (its
    /// display name) instead of the first one.
    pub async fn create_task_for_persona(
        &self,
        env: &E2eEnv,
        project_key: &str,
        description: &str,
        issue: Option<u64>,
        persona: &str,
    ) -> Result<String> {
        self.compose_task(env, project_key, description, issue, None, Some(persona))
            .await
    }

    /// [`Session::create_task`] on the existing branch `branch` (the composer's options).
    pub async fn create_task_on_branch(
        &self,
        env: &E2eEnv,
        project_key: &str,
        description: &str,
        branch: &str,
    ) -> Result<String> {
        self.compose_task(env, project_key, description, None, Some(branch), None)
            .await
    }

    async fn compose_task(
        &self,
        env: &E2eEnv,
        project_key: &str,
        description: &str,
        issue: Option<u64>,
        branch: Option<&str>,
        persona: Option<&str>,
    ) -> Result<String> {
        // Every submission passes here: the suffix makes the description unique to the test, which
        // ties a task to its creation (the fake agent scripts match markers with `contains`).
        let description = &submitted_description(description, self.description_suffix.get());
        self.page
            .goto(
                &format!("{}/tasks?project={project_key}", env.base_url),
                None,
            )
            .await
            .context("open the task list")?;
        // `.first()`: the list has a composer button per layout (desktop and phone), without an
        // accessible name of its own.
        self.page
            .locator("button[hx-get^='/tasks/compose']:visible")
            .first()
            .click(None)
            .await
            .context("open the composer")?;
        if let Some(persona) = persona {
            self.page
                .locator("select[name='persona']")
                .first()
                .select_option(
                    playwright_rs::protocol::SelectOption::Label(persona.to_owned()),
                    None,
                )
                .await
                .with_context(|| format!("choose the persona {persona}"))?;
        }
        if issue.is_some() || branch.is_some() {
            button(&self.page, "Task options")
                .click(None)
                .await
                .context("open the task options")?;
        }
        if let Some(issue) = issue {
            self.page
                .locator("#issue_number")
                .fill(&issue.to_string(), None)
                .await
                .context("fill the issue number")?;
        }
        if let Some(branch) = branch {
            self.page
                .locator("#branch")
                .fill(branch, None)
                .await
                .context("fill the existing branch")?;
        }
        self.page
            .locator("#description")
            .fill(description, None)
            .await
            .context("fill the description")?;
        let submitted_at = SystemTime::now();
        button(&self.page, "Create task")
            .click(None)
            .await
            .context("create the task")?;
        // From here on the task may exist although the page never shows it.
        let landed = self.landed_task(project_key).await;
        if landed.is_err() {
            self.unlanded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(UnlandedTask {
                    project: project_key.to_owned(),
                    description: description.to_owned(),
                    submitted_at,
                });
        }
        landed
    }

    /// Waits for the UI to open the new task of `project_key` and records its key.
    async fn landed_task(&self, project_key: &str) -> Result<String> {
        expect_page(&self.page)
            .to_have_url_regex(&format!("/tasks/{project_key}-[0-9]+$"))
            .await
            .context("land on the new task")?;
        let url = self.page.url();
        let key = url
            .rsplit('/')
            .next()
            .map(str::to_owned)
            .with_context(|| format!("task key in {url}"))?;
        self.record_created(&key);
        Ok(key)
    }

    /// Starts a chat of project `project_key` with the only persona and `message` through the new
    /// chat dialog. Returns the key of the chat once the UI has opened it. The message gets the
    /// suffix of the test like a task description does. The chat is recorded, so the fixture
    /// cancels the tasks it creates.
    pub async fn create_chat(
        &self,
        env: &E2eEnv,
        project_key: &str,
        message: &str,
    ) -> Result<String> {
        let message = &submitted_description(message, self.description_suffix.get());
        self.page
            .goto(
                &format!("{}/chats?project={project_key}", env.base_url),
                None,
            )
            .await
            .context("open the chat list")?;
        // `.first()`: the page has a "New chat" button for each layout (header and phone).
        self.page
            .locator("button[hx-get^='/chats/compose']:visible")
            .first()
            .click(None)
            .await
            .context("open the new chat dialog")?;
        self.page
            .locator("#composer #message")
            .fill(message, None)
            .await
            .context("fill the first message")?;
        button(&self.page, "Start chat")
            .click(None)
            .await
            .context("start the chat")?;
        expect_page(&self.page)
            .to_have_url_regex(&format!("/chats/{project_key}-[0-9]+-CHAT$"))
            .await
            .context("land on the new chat")?;
        let url = self.page.url();
        let key = url
            .rsplit('/')
            .next()
            .map(str::to_owned)
            .with_context(|| format!("chat key in {url}"))?;
        self.record_created(&key);
        Ok(key)
    }

    /// Opens the page of chat `key`.
    pub async fn open_chat(&self, env: &E2eEnv, key: &str) -> Result<()> {
        self.page
            .goto(&format!("{}/chats/{key}", env.base_url), None)
            .await
            .with_context(|| format!("open chat {key}"))?;
        Ok(())
    }

    /// Archives the open chat with the "Archive" action of its overview and waits for the chat
    /// list the UI goes back to.
    pub async fn archive_chat(&self) -> Result<()> {
        button(&self.page, "Archive")
            .click(None)
            .await
            .context("archive the chat")?;
        expect_page(&self.page)
            .to_have_url_regex(r"/chats\?project=")
            .await
            .context("go back to the chat list")
    }

    /// Unarchives the open (archived) chat with the "Unarchive" action of its overview.
    pub async fn unarchive_chat(&self) -> Result<()> {
        // `.first()`: the notice in place of the message box has the action too.
        self.page
            .get_by_role(
                AriaRole::Button,
                Some(GetByRoleOptions::default().name("Unarchive").exact(true)),
            )
            .first()
            .click(None)
            .await
            .context("unarchive the chat")
    }

    /// Cuts (`true`) or restores the network of the session's context, which drops the live
    /// connections of its pages: the way a test loses and regains the live updates.
    pub async fn set_offline(&self, offline: bool) -> Result<()> {
        within_for(
            UI_TIMEOUT,
            &format!("set the context offline = {offline}"),
            self.context.set_offline(offline),
        )
        .await
    }

    /// Evaluates `script` on the open page within [`UI_TIMEOUT`]; `what` names the step.
    pub async fn evaluate(&self, what: &str, script: &str) -> Result<String> {
        within_for(UI_TIMEOUT, what, self.page.evaluate_value(script)).await
    }

    /// The card of request `id` on the open page.
    pub fn request_card(&self, id: df_core::RequestId) -> Locator {
        self.page.locator(format!("#request-{id}"))
    }

    /// Opens the requests inbox and waits for the card of request `id`.
    pub async fn open_inbox_card(&self, env: &E2eEnv, id: df_core::RequestId) -> Result<Locator> {
        within_for(
            CARD_TIMEOUT,
            "open the requests inbox",
            self.page.goto(&format!("{}/requests", env.base_url), None),
        )
        .await?;
        let card = self.request_card(id);
        expect(card.clone())
            .with_timeout(CARD_TIMEOUT)
            .to_be_visible()
            .await
            .with_context(|| format!("wait for the card of {id} in the inbox"))?;
        Ok(card)
    }

    /// Whether the open page requested the highlighting editor bundle.
    pub async fn requested_editor_bundle(&self) -> Result<bool> {
        let script =
            "performance.getEntriesByType('resource').some(e => e.name.includes('editor.js'))";
        let answer = self.evaluate("check for the editor bundle", script).await?;
        Ok(answer.trim_matches('"') == "true")
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
        let approve = self.page.locator(&card).get_by_role(
            AriaRole::Button,
            Some(GetByRoleOptions::default().name("Approve")),
        );
        approve.click(None).await.context("approve the request")?;
        expect(approve)
            .with_timeout(CARD_TIMEOUT)
            .to_be_hidden()
            .await
            .context("wait for the approval to be recorded")
    }

    /// Opens the settings page of the persona `display_name` from the list, on the tab `tab`
    /// (`General`, `System prompt` or `Commits`: the name of the tab).
    pub async fn open_persona_page(
        &self,
        env: &E2eEnv,
        display_name: &str,
        tab: &str,
    ) -> Result<()> {
        let page = &self.page;
        page.goto(&format!("{}/personas", env.base_url), None)
            .await
            .context("open the personas")?;
        // `.first()`: the list has the link once per layout (desktop and phone).
        page.get_by_role(
            AriaRole::Link,
            Some(
                GetByRoleOptions::default()
                    .name(format!("Edit persona {display_name}"))
                    .exact(true),
            ),
        )
        .first()
        .click(None)
        .await
        .context("edit the persona")?;
        expect_page(page)
            .to_have_url_regex(r"/personas/[0-9a-f-]+$")
            .await
            .context("the settings page of the persona opens")?;
        page.get_by_role(
            AriaRole::Tab,
            Some(GetByRoleOptions::default().name(tab).exact(true)),
        )
        .click(None)
        .await
        .with_context(|| format!("open the tab {tab}"))
    }

    /// Opens the "New persona" modal of the persona list.
    pub async fn open_new_persona_modal(&self, env: &E2eEnv) -> Result<()> {
        let page = &self.page;
        page.goto(&format!("{}/personas", env.base_url), None)
            .await
            .context("open the personas")?;
        // Role and name only match the visible button: the phone's floating button is "New task".
        button(page, "New persona")
            .click(None)
            .await
            .context("open the New persona modal")
    }

    /// Creates the persona with `display_name`, `handle` and `commit_email`, the seeded model
    /// provider and model in the "New persona" modal; returns when the settings page of the new
    /// persona is open on its System prompt tab.
    pub async fn create_persona(
        &self,
        env: &E2eEnv,
        display_name: &str,
        handle: &str,
        commit_email: &str,
    ) -> Result<()> {
        let page = &self.page;
        self.open_new_persona_modal(env).await?;
        page.locator("#display_name")
            .fill(display_name, None)
            .await?;
        page.locator("#handle").fill(handle, None).await?;
        page.locator("#commit_email")
            .fill(commit_email, None)
            .await?;
        page.locator("#model_provider_id")
            .select_option(
                playwright_rs::protocol::SelectOption::Label(
                    crate::data::MODEL_PROVIDER_NAME.to_string(),
                ),
                None,
            )
            .await?;
        page.locator("#model")
            .select_option(
                playwright_rs::protocol::SelectOption::Label(format!(
                    "{} (Anthropic Messages)",
                    crate::data::PERSONA_MODEL
                )),
                None,
            )
            .await?;
        button(page, "Create persona")
            .click(None)
            .await
            .context("create the persona")?;
        expect_page(page)
            .to_have_url_regex(r"/personas/[0-9a-f-]+\?tab=system-prompt$")
            .await
            .context("the new persona opens on its System prompt tab")
    }

    /// Types `text` into the System prompt field of the open persona page, in the highlighting
    /// editor: the build of the suite has the editor bundle, which takes the textarea's place from
    /// 768 px. Waits for the editor, which attaches once the bundle has loaded.
    pub async fn write_system_prompt(&self, text: &str) -> Result<()> {
        let page = &self.page;
        let editor = self.system_prompt_editor();
        expect(editor.clone())
            .with_timeout(UI_TIMEOUT)
            .to_be_visible()
            .await
            .context("the highlighting editor of the System prompt")?;
        editor.click(None).await?;
        page.keyboard()
            .type_text(text, None)
            .await
            .context("type the system prompt")
    }

    /// The highlighting editor of the System prompt: a text box named by the field's label.
    /// (Without an editor the textarea has the same role and name.)
    pub fn system_prompt_editor(&self) -> Locator {
        self.page.get_by_role(
            AriaRole::Textbox,
            Some(
                GetByRoleOptions::default()
                    .name("System prompt")
                    .exact(true),
            ),
        )
    }

    /// Saves the persona page with "Save persona" and waits for the status "Persona saved", which
    /// is a live region, so screen readers announce it.
    pub async fn save_persona(&self) -> Result<()> {
        let page = &self.page;
        // `.first()`: the page has the button in its header and again for phones.
        button(page, "Save persona")
            .first()
            .click(None)
            .await
            .context("save the persona")?;
        let status = page.locator("#save-status");
        expect(status.clone())
            .with_timeout(UI_TIMEOUT)
            .to_contain_text("Persona saved")
            .await
            .context("the page says the persona is saved")?;
        expect(status)
            .to_have_attribute("aria-live", "polite")
            .await
            .context("the status is announced")
    }

    /// Signs `token` with the key of the persona `display_name` through "Verify key on Forgejo" on
    /// the Commits tab of its settings page; returns the armored signature the dialog shows.
    pub async fn sign_forgejo_token(
        &self,
        env: &E2eEnv,
        display_name: &str,
        domain: &str,
        token: &str,
    ) -> Result<String> {
        let page = &self.page;
        self.open_persona_page(env, display_name, "Commits").await?;
        button(page, "Verify key on Forgejo")
            .click(None)
            .await
            .context("open the verify dialog")?;
        page.locator("#forge-token")
            .fill(token, None)
            .await
            .context("enter the token")?;
        page.locator("#forge-domain")
            .fill(domain, None)
            .await
            .context("enter the Forgejo domain")?;
        button(page, "Sign token")
            .click(None)
            .await
            .context("sign the token")?;
        expect(page.locator("#forge-signature"))
            .with_timeout(UI_TIMEOUT)
            .to_be_visible()
            .await
            .context("the dialog shows the signature")?;
        page.locator("#forge-signature")
            .input_value(None)
            .await
            .context("read the signature")
    }

    /// Signs in to Forgejo's web UI (`web_url`) as `user` in this session's browser.
    pub async fn forgejo_login(&self, web_url: &str, user: &str, password: &str) -> Result<()> {
        let page = &self.page;
        page.goto(&format!("{web_url}/user/login"), None)
            .await
            .context("open the Forgejo sign-in")?;
        page.locator("#user_name")
            .fill(user, None)
            .await
            .context("enter the Forgejo user")?;
        page.locator("#password")
            .fill(password, None)
            .await
            .context("enter the Forgejo password")?;
        page.locator("form[action='/user/login'] button.primary")
            .click(None)
            .await
            .context("sign in to Forgejo")?;
        expect(page.locator("#navbar"))
            .with_timeout(UI_TIMEOUT)
            .to_be_visible()
            .await
            .context("Forgejo signed in")
    }

    /// Hands `signature` (over the token Forgejo shows for the key) to the verification form of
    /// the key with `fingerprint`, as the signed-in Forgejo user; waits for Forgejo's success
    /// message.
    pub async fn forgejo_verify_key(
        &self,
        web_url: &str,
        fingerprint: &str,
        signature: &str,
    ) -> Result<()> {
        let page = &self.page;
        let url = reqwest::Url::parse_with_params(
            &format!("{web_url}/user/settings/keys"),
            &[("verify_ssh", fingerprint)],
        )
        .context("the verification URL")?;
        page.goto(url.as_str(), None)
            .await
            .context("open the key verification")?;
        page.locator("#ssh-key-signature")
            .fill(signature, None)
            .await
            .context("enter the signature")?;
        page.locator("form:has(#ssh-key-signature) button.primary")
            .click(None)
            .await
            .context("submit the signature")?;
        expect(page.locator(".flash-success"))
            .with_timeout(UI_TIMEOUT)
            .to_be_visible()
            .await
            .context("Forgejo reports the key as verified")
    }

    /// Sends `message` to the open task.
    pub async fn send_message(&self, message: &str) -> Result<()> {
        self.page
            .locator("#message")
            .fill(message, None)
            .await
            .context("fill the message")?;
        button(&self.page, "Send")
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
        button(&self.page, "Interrupt")
            .click(None)
            .await
            .context("interrupt the turn")
    }

    /// Stops the active run of the open task with the "Stop run" button of its run card.
    pub async fn stop_run(&self) -> Result<()> {
        self.open_runs().await?;
        // `.first()`: the run cards of earlier runs have the button too; the active one is first.
        self.page
            .locator("#runs")
            .get_by_role(
                AriaRole::Button,
                Some(GetByRoleOptions::default().name("Stop run")),
            )
            .first()
            .click(None)
            .await
            .context("stop the run")
    }

    /// Cancels the open task: Status menu, "Cancel task…", then the confirmation.
    pub async fn cancel_task(&self) -> Result<()> {
        cancel_task_on(&self.page).await
    }

    /// Opens the closed "Runs" disclosure of the open task.
    pub async fn open_runs(&self) -> Result<()> {
        self.page
            .locator("#runs summary")
            .click(None)
            .await
            .context("open the runs")
    }

    /// Selects the "Subtasks" tab of the details column of the open task.
    pub async fn open_subtasks_tab(&self) -> Result<()> {
        self.page
            .locator("#tab-subtasks")
            .click(None)
            .await
            .context("open the subtasks tab")
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

    /// Asserts that the open task's transcript ends at its last item: the page does not scroll,
    /// the transcript is scrolled to its end, the last item (a request card, for example) is fully
    /// in view and nothing blank follows it. Retries for `UI_TIMEOUT`, because the view settles
    /// after the stream's first update and after fonts load.
    pub async fn expect_transcript_ends_at_last_item(&self) -> Result<()> {
        let script = r"(() => {
            const page = document.scrollingElement;
            const scroll = document.querySelector('#transcript-scroll');
            const last = document.querySelector('#transcript').lastElementChild;
            if (!scroll || !last) return 'no transcript';
            const box = last.getBoundingClientRect();
            const view = scroll.getBoundingClientRect();
            const padding = parseFloat(getComputedStyle(scroll).paddingBottom);
            const blank = view.bottom - padding - box.bottom;
            if (page.scrollHeight > page.clientHeight || page.scrollTop !== 0) {
                // Name the lowest elements, which are what makes the page taller than the window.
                const lowest = [...document.body.querySelectorAll('*')]
                    .map((e) => [e.getBoundingClientRect().bottom, e.tagName + (e.id ? '#' + e.id : '') + '.' + String(e.className).slice(0, 50) + ' < ' + (e.parentElement ? e.parentElement.tagName + (e.parentElement.id ? '#' + e.parentElement.id : '') : '')])
                    .filter((x) => x[0] > window.innerHeight && x[0] <= page.scrollHeight + 2)
                    .sort((a, b) => b[0] - a[0]).slice(0, 5).map((x) => Math.round(x[0]) + ' ' + x[1]);
                return 'the page scrolls: ' + page.scrollTop + ' of ' + (page.scrollHeight - page.clientHeight) + '; lowest: ' + lowest.join(' | ');
            }
            if (box.top < view.top || box.bottom > view.bottom) return 'the last item is not in view: ' + box.top + '..' + box.bottom + ' in ' + view.top + '..' + view.bottom;
            if (Math.abs(blank) > 2) return 'blank space of ' + blank + 'px follows the last item';
            return 'ok';
        })()";
        eventually(
            "the transcript ends at its last item",
            UI_TIMEOUT,
            || async {
                let answer = within_for(
                    UI_TIMEOUT,
                    "measure the transcript",
                    self.page.evaluate_value(script),
                )
                .await?;
                let answer = answer.trim_matches('"');
                if answer == "ok" {
                    return Ok(Some(()));
                }
                anyhow::bail!("{answer}")
            },
        )
        .await
    }

    /// Waits until the open task shows `text` (a transcript line, a status).
    pub async fn expect_text(&self, text: &str, timeout: Duration) -> Result<()> {
        // `.first()`: the text may show in the transcript and again in a summary or the list.
        expect(self.page.get_by_text(text, false).first())
            .with_timeout(timeout)
            .to_be_visible()
            .await
            .with_context(|| format!("wait for `{text}` in the UI"))
    }
}

/// Cancels task `key` with the form post the UI sends (`POST /tasks/{key}/cancel`, which needs the
/// session cookie and the header htmx sets, no token), through a browser connection of its own:
/// the control plane is reachable from the browser in the cluster, not from the test host. For the
/// teardown when the page of the test cannot cancel. A task that cannot be cancelled any more
/// (HTTP 409) is already final.
pub async fn cancel_task_over_new_connection(env: &E2eEnv, cookie: &str, key: &str) -> Result<()> {
    let playwright = within("start the local Playwright driver", Playwright::launch()).await?;
    let browser = within(
        &format!("connect to {}", env.playwright_ws),
        playwright.chromium().connect(&env.playwright_ws, None),
    )
    .await?;
    let context = within("open a browser context", browser.new_context()).await?;
    let request = within("get the request context", context.request()).await?;
    let headers = HashMap::from([
        ("Cookie".to_owned(), cookie.to_owned()),
        ("HX-Request".to_owned(), "true".to_owned()),
    ]);
    let url = format!("{}/tasks/{key}/cancel", env.base_url);
    let response = within_for(
        CANCEL_TIMEOUT,
        &format!("POST {url}"),
        request.post(&url, FetchOptions::builder().headers(headers).build()),
    )
    .await?;
    match response.status() {
        200 | 409 => Ok(()),
        status => Err(anyhow!("POST {url}: HTTP {status}")),
    }
}

/// The button of `page` with the accessible name `name` (exactly).
pub fn button(page: &Page, name: &str) -> Locator {
    page.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name(name).exact(true)),
    )
}

/// The radio button or checkbox (`role`) of the option labelled `label` inside `scope`.
pub fn option_in(scope: &Locator, role: AriaRole, label: &str) -> Locator {
    scope.get_by_role(role, Some(GetByRoleOptions::default().name(label)))
}

/// Presses "Answer" on the question card `card`.
pub async fn press_answer(card: &Locator) -> Result<()> {
    button_in(card, "Answer")
        .click(None)
        .await
        .context("press Answer")
}

/// Waits until `scope` contains `text`.
pub async fn expect_text_in(scope: &Locator, text: &str, timeout: Duration) -> Result<()> {
    expect(scope.clone())
        .with_timeout(timeout)
        .to_contain_text(text)
        .await
        .with_context(|| format!("wait for `{text}`"))
}

/// The button inside `scope` with the accessible name `name` (exactly).
pub fn button_in(scope: &Locator, name: &str) -> Locator {
    scope.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name(name).exact(true)),
    )
}

/// Fails when `page` scrolls sideways: the document is wider than the viewport. `when` names the
/// situation in the failure message (`at 390 px`).
pub async fn assert_no_sideways_scroll(page: &Page, when: &str) -> Result<()> {
    let overflows = within_for(
        UI_TIMEOUT,
        "read the page scroll width",
        page.evaluate_value(
            "document.documentElement.scrollWidth > document.documentElement.clientWidth",
        ),
    )
    .await?;
    if overflows != "false" {
        return Err(anyhow!("the page scrolls sideways {when}"));
    }
    Ok(())
}

/// Cancels the task open on `page`: Status menu, "Cancel task…", then the confirmation.
async fn cancel_task_on(page: &Page) -> Result<()> {
    // CSS: the accessible name of the status menu button is the current status, which varies.
    page.locator("#task-details button[aria-haspopup=true]")
        .click(None)
        .await
        .context("open the status menu")?;
    button(page, "Cancel task…")
        .click(None)
        .await
        .context("choose Cancel task…")?;
    page.get_by_role(AriaRole::Dialog, None)
        .get_by_role(
            AriaRole::Button,
            Some(GetByRoleOptions::default().name("Cancel task").exact(true)),
        )
        .click(None)
        .await
        .context("confirm the cancellation")
}

/// The text a task creation submits for `description`: with the suffix of the test, ` [<suffix>]`,
/// when there is one.
pub fn submitted_description(description: &str, suffix: Option<&String>) -> String {
    match suffix {
        Some(suffix) => format!("{description} [{suffix}]"),
        None => description.to_owned(),
    }
}

/// An error page under the UI's origin (for example the 502 "Login unavailable" of `/login` when the
/// identity provider fails) is not a logged-in page, although its URL passes [`lands_on_the_ui`].
fn check_ui_status(status: Option<u16>) -> Result<()> {
    match status {
        Some(status) if status >= 400 => Err(anyhow!("the UI answered with HTTP {status}")),
        _ => Ok(()),
    }
}

/// Whether `url`, where a page ended up after opening the UI, is a page of the UI (at `base_url`)
/// and not the login form of the identity provider.
fn lands_on_the_ui(url: &str, base_url: &str) -> bool {
    url.strip_prefix(base_url)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

/// Writes `<topic>: <line>` to stderr, the test output of cargo (visible with `--nocapture`): a run
/// shows how often the suite logs in and what a teardown had to do.
#[expect(
    clippy::print_stderr,
    reason = "the suite reports to the test output of cargo"
)]
pub(crate) fn report(topic: &str, line: &str) {
    eprintln!("{topic}: {line}");
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

    use super::{check_ui_status, lands_on_the_ui, submitted_description, within, within_for};

    #[test]
    fn an_error_page_of_the_ui_is_not_a_login() {
        assert!(check_ui_status(Some(200)).is_ok());
        assert!(check_ui_status(None).is_ok());
        let error = check_ui_status(Some(502)).unwrap_err();
        assert!(error.to_string().contains("502"), "{error}");
        assert!(check_ui_status(Some(404)).is_err());
    }

    #[test]
    fn a_page_of_the_ui_is_not_the_login_form() {
        let base = "http://127.0.0.1:13001";

        assert!(lands_on_the_ui("http://127.0.0.1:13001", base));
        assert!(lands_on_the_ui("http://127.0.0.1:13001/", base));
        assert!(lands_on_the_ui(
            "http://127.0.0.1:13001/tasks?project=DF",
            base
        ));
        assert!(!lands_on_the_ui(
            "http://dex.dex.svc:5556/dex/auth?client_id=dark-factory",
            base
        ));
    }

    #[test]
    fn another_port_with_the_same_prefix_is_not_the_ui() {
        assert!(!lands_on_the_ui(
            "http://127.0.0.1:130010/",
            "http://127.0.0.1:13001"
        ));
    }

    #[test]
    fn a_submitted_description_ends_in_the_suffix_of_the_test() {
        let suffix = "1a2b3c4d".to_owned();

        assert_eq!(
            submitted_description("Add a greeting file", Some(&suffix)),
            "Add a greeting file [1a2b3c4d]"
        );
    }

    #[test]
    fn a_description_without_a_suffix_is_submitted_as_it_is() {
        assert_eq!(
            submitted_description("Add a greeting file", None),
            "Add a greeting file"
        );
    }

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
