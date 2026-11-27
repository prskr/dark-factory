//! The composer's memory of the last project (specs/task-ui, "Composer remembers project and
//! assignee") in a real browser (`just e2e-local`). Needs the environment of `E2eEnv`, so the
//! tests are ignored by default.

use anyhow::{Context, Result};
use df_e2e::browser::{Session, button};
use df_e2e::data;
use df_e2e::fixture;
use playwright_rs::expect;

/// task-ui, "Next task in the same project": the composer preselects the project and the
/// assignee of the last task when no project filter chose one. The task is made for a persona
/// that is not the default one (the first of the list).
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_composer_preselects_the_project_and_assignee_of_the_last_created_task() -> Result<()> {
    fixture::run(
        "the_composer_preselects_the_project_and_assignee_of_the_last_created_task",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            let display_name = format!("Remembered Bot {}", world.suffix);
            let handle = world.unique("remembered");
            session
                .create_persona(
                    env,
                    &display_name,
                    &handle,
                    &format!("{handle}@example.org"),
                )
                .await?;
            session
                .create_task_for_persona(
                    env,
                    data::NETWORK_PROJECT_KEY,
                    "Remember the project and the assignee.",
                    None,
                    &display_name,
                )
                .await?;

            open_composer(session, &format!("{}/tasks", env.base_url)).await?;

            expect_chosen_project(session, data::NETWORK_PROJECT_KEY).await?;
            expect(
                session
                    .page
                    .locator("#composer select[name='persona'] option:checked"),
            )
            .to_contain_text(&display_name)
            .await
            .context("the composer shows the remembered assignee")
        },
    )
    .await
}

/// task-ui, "Storage blocked": `localStorage` throws on every access. The composer opens with its
/// defaults and submitting it creates the task.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_composer_works_with_defaults_when_browser_storage_is_blocked() -> Result<()> {
    fixture::run(
        "the_composer_works_with_defaults_when_browser_storage_is_blocked",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            session
                .page
                .add_init_script(
                    "Object.defineProperty(window, 'localStorage', { get() { throw new DOMException('blocked', 'SecurityError') } })",
                )
                .await?;

            session.page.clear_console_messages();
            session.page.clear_page_errors();

            open_composer(session, &format!("{}/tasks", env.base_url)).await?;
            expect_default_project(session).await?;
            expect_default_assignee(session).await?;

            // Submitting creates the task (the helper opens its own composer and submits it).
            let key = session
                .create_task(
                    env,
                    data::NETWORK_PROJECT_KEY,
                    "Create with blocked storage.",
                    None,
                )
                .await?;
            assert!(key.starts_with(data::NETWORK_PROJECT_KEY), "{key}");

            // The composer handles the blocked storage itself: nothing was logged as an error.
            // Alpine logs an expression that throws (the submit handler) as a warning first and
            // rethrows it later, which may come too late for the page error.
            let log = session.console_log();
            assert!(
                !log.contains("[error]")
                    && !log.contains("[pageerror]")
                    && !log.contains("Alpine Expression Error"),
                "errors with blocked storage:\n{log}"
            );
            Ok(())
        },
    )
    .await
}

/// task-ui, "Explicit choice wins": with the task list filtered to another project, the composer
/// shows that project although another one is remembered.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_composer_prefers_the_filtered_project_to_the_remembered_one() -> Result<()> {
    fixture::run(
        "the_composer_prefers_the_filtered_project_to_the_remembered_one",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            session
                .create_task(
                    env,
                    data::NETWORK_PROJECT_KEY,
                    "Remember the project.",
                    None,
                )
                .await?;

            let filtered = format!("{}/tasks?project={}", env.base_url, data::PROJECT_KEY);
            open_composer(session, &filtered).await?;

            expect_chosen_project(session, data::PROJECT_KEY).await
        },
    )
    .await
}

/// task-ui, "Remembered project removed": a remembered project that is not offered any more
/// leaves the composer at its default project.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_composer_ignores_a_remembered_project_that_is_not_offered() -> Result<()> {
    fixture::run(
        "the_composer_ignores_a_remembered_project_that_is_not_offered",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            session
                .page
                .goto(&format!("{}/tasks", env.base_url), None)
                .await?;
            session
                .evaluate(
                    "remember a project that does not exist",
                    "localStorage.setItem('df.composer', JSON.stringify({ project: 'gone' }))",
                )
                .await?;

            open_composer(session, &format!("{}/tasks", env.base_url)).await?;

            expect_default_project(session).await
        },
    )
    .await
}

/// Opens the composer from the list page at `url`.
async fn open_composer(session: &Session, url: &str) -> Result<()> {
    session.page.goto(url, None).await?;
    // `.first()`: the rail and the empty detail panel each have a visible "New task" button on a
    // desktop; both open the same composer.
    button(&session.page, "New task")
        .first()
        .click(None)
        .await
        .context("open the composer")?;
    expect(session.page.locator("#composer select[name='project']"))
        .to_be_visible()
        .await?;
    Ok(())
}

/// Waits until the project select of the composer shows the project `key`.
async fn expect_chosen_project(session: &Session, key: &str) -> Result<()> {
    expect(
        session
            .page
            .locator("#composer select[name='project'] option:checked"),
    )
    .to_contain_text(&format!("{key} - "))
    .await
    .with_context(|| format!("the composer shows project {key}"))
}

/// Waits until the composer shows the first project of its list, its default.
async fn expect_default_project(session: &Session) -> Result<()> {
    expect_first_option_chosen(session, "project").await
}

/// Waits until the composer shows the first persona of its list, its default.
async fn expect_default_assignee(session: &Session) -> Result<()> {
    expect_first_option_chosen(session, "persona").await
}

async fn expect_first_option_chosen(session: &Session, select: &str) -> Result<()> {
    let options = format!("#composer select[name='{select}'] option");
    // `.first()`: the default is the first option of the list.
    let first = session.page.locator(&options).first().inner_text().await?;
    expect(session.page.locator(format!("{options}:checked")))
        .to_have_text(first.trim())
        .await
        .with_context(|| format!("the composer shows its default {select}"))
}
