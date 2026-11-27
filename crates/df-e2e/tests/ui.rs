//! Client-side behaviour of the web UI in a real browser (`just e2e-local`): the settings area,
//! the project and model provider settings pages and their tabs, the task list sort menu, tab rows
//! on phones, task swaps without console errors and the network traffic page. Needs the
//! environment of `E2eEnv`, so the tests are ignored by default.

#![expect(
    clippy::indexing_slicing,
    reason = "the bodies of the tests are functions of their own, which clippy.toml does not exempt like the #[test] functions"
)]

use std::sync::Arc;

use anyhow::{Context, Result};
use df_e2e::browser::{Session, assert_no_sideways_scroll, button, button_in};
use df_e2e::fixture::{self, World};
use df_e2e::wait::{RECONNECT_TIMEOUT, RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use playwright_rs::protocol::{
    AriaRole, ContinueOptions, DragToOptions, GetByRoleOptions, Locator, Position,
};
use playwright_rs::{Viewport, expect, expect_page};

/// Wide enough for the graph to grow next to the 520 px events table.
const WIDE: Viewport = Viewport {
    width: 1440,
    height: 900,
};
/// Rows of the events table of the traffic page.
const ROWS: &str = "#flows tbody tr";
/// Divider between the graph and the events table of the traffic page.
const DIVIDER: &str = "[role=separator]";
/// Width change of an arrow key on the divider (`divKey` in `scripts/egress-log.ts`).
const DIVIDER_STEP: u32 = 32;

/// Width of the navigation rail from 768 px (`w-[4.5rem]`).
const RAIL_WIDTH: f64 = 72.0;

/// The sort menu item named `name`.
fn menu_item(session: &Session, name: &str) -> Locator {
    session.page.get_by_role(
        AriaRole::Menuitemradio,
        Some(GetByRoleOptions::default().name(name).exact(true)),
    )
}

/// The open menu lies fully right of `min_left` and inside the viewport.
async fn assert_menu_inside_viewport(session: &Session, min_left: f64) -> Result<()> {
    let menu = session.page.locator("[role=menu]:visible");
    expect(menu.clone()).to_be_visible().await?;
    let menu = menu.bounding_box().await?.context("box of the open menu")?;
    let viewport: f64 = session
        .page
        .evaluate_value("window.innerWidth")
        .await?
        .parse()?;
    assert!(
        menu.x >= min_left - 0.5 && menu.x + menu.width <= viewport + 0.5,
        "menu {menu:?} is not inside {min_left}..{viewport}"
    );
    Ok(())
}

/// The row of `older`, matched only when it comes after the row of `newer`. Tests in parallel add
/// tasks to the same project, so the test's own rows need not be the first ones.
fn newer_listed_before_older(session: &Session, newer: &str, older: &str) -> Locator {
    session
        .page
        .locator(format!("#task-rows > #row-{newer} ~ #row-{older}"))
}

/// Opens the settings page of project `key` from its name link in the projects page (the desktop
/// row or the phone card, whichever is visible).
async fn open_project_page(session: &Session, env: &E2eEnv, key: &str) -> Result<()> {
    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await?;
    // `.first()`: the name link exists in the desktop row and in the phone card.
    session
        .page
        .locator(format!("a[href='/projects/{key}']:visible"))
        .first()
        .click(None)
        .await
        .with_context(|| format!("open the settings page of project {key}"))?;
    expect(session.page.locator("[role=tablist]"))
        .to_be_visible()
        .await
        .context("tab row of the project settings page")?;
    expect_page(&session.page)
        .to_have_url_regex(&format!("/projects/{key}$"))
        .await
        .context("URL of the project settings page")
}

/// web-ui-shell and configuration-management, "Settings area": Projects is the first section of
/// the settings area, reached through Settings in the rail on a desktop and through the bottom bar
/// on a phone.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn projects_are_reached_through_settings_on_desktop_and_the_bottom_bar_on_phones()
-> Result<()> {
    fixture::run(
        "projects_are_reached_through_settings_on_desktop_and_the_bottom_bar_on_phones",
        settings_navigation_body,
    )
    .await
}

async fn settings_navigation_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);

    session.page.set_viewport_size(WIDE).await?;
    session
        .page
        .goto(&format!("{}/tasks", env.base_url), None)
        .await?;
    expect(
        session
            .page
            .locator("#desktop-menu [aria-label='Projects']"),
    )
    .to_have_count(0)
    .await
    .context("the rail has no Projects link")?;
    session
        .page
        .locator("aside [aria-label='Settings']")
        .click(None)
        .await
        .context("open Settings in the rail")?;
    expect_page(&session.page)
        .to_have_url_regex("/projects$")
        .await
        .context("Settings opens the projects page")?;
    expect(session.page.locator("#settings-menu a[aria-current=page]"))
        .to_contain_text("Projects")
        .await
        .context("the settings sidebar marks Projects")?;

    session
        .page
        .set_viewport_size(Viewport {
            width: 390,
            height: 800,
        })
        .await?;
    session
        .page
        .goto(&format!("{}/tasks", env.base_url), None)
        .await?;
    session
        .page
        .locator("#mobile-menu a[href='/projects']")
        .click(None)
        .await
        .context("open Projects in the bottom bar")?;
    expect_page(&session.page)
        .to_have_url_regex("/projects$")
        .await
        .context("the bottom bar opens the projects page")
}

/// Neither the project table nor the page scrolls sideways, and the Done cell of every row, the
/// one of `row` included, lies inside the table and the viewport.
async fn assert_project_table_fits(session: &Session, row: &str) -> Result<()> {
    let overflows = session
        .page
        .evaluate_value(
            "(c => c.scrollWidth > c.clientWidth)(document.querySelector('[data-table]'))",
        )
        .await?;
    assert_eq!(overflows, "false", "the project table scrolls sideways");
    assert_no_sideways_scroll(&session.page, "at 1350 px").await?;
    let all_done_inside = session
        .page
        .evaluate_value(
            "[...document.querySelectorAll('[data-table] tbody tr')].every(r => r.lastElementChild.getBoundingClientRect().right <= window.innerWidth)",
        )
        .await?;
    assert_eq!(
        all_done_inside, "true",
        "a Done cell lies outside the viewport"
    );
    let card_box = session
        .page
        .locator("[data-table]")
        .bounding_box()
        .await?
        .context("box of the table")?;
    let done = session
        .page
        .locator(format!("{row} td:last-child"))
        .bounding_box()
        .await?
        .context("box of the Done cell")?;
    assert!(
        done.x + done.width <= card_box.x + card_box.width + 0.5,
        "the Done cell {done:?} is outside the table {card_box:?}"
    );
    Ok(())
}

/// configuration-management, "Two-line settings list rows": at 1350 px the project table fits
/// without sideways scrolling even for a very long repository URL, a click on the URL opens the
/// project and a count keeps its own link.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_project_row_opens_the_project_and_a_long_url_does_not_widen_the_table() -> Result<()> {
    fixture::run(
        "the_project_row_opens_the_project_and_a_long_url_does_not_widen_the_table",
        project_row_body,
    )
    .await
}

async fn project_row_body(world: Arc<World>) -> Result<()> {
    // Unique per run: the test must pass again against the same instance.
    let key = format!("LU{}", world.suffix.to_uppercase());
    let key = key.as_str();
    let (env, session) = (&world.env, &world.session);
    session
        .page
        .set_viewport_size(Viewport {
            width: 1350,
            height: 900,
        })
        .await?;
    let url = format!("https://git.example.org/{}.git", "long-path/".repeat(25));
    assert!(url.len() > 200);
    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await?;
    // CSS and `.first()`: the button has no accessible name of its own and exists once per layout.
    session
        .page
        .locator("button[hx-get='/projects/new']:visible")
        .first()
        .click(None)
        .await
        .context("open the new project form")?;
    for (field, value) in [
        ("#key", key),
        ("#name", "Long URL"),
        ("#repo_url", url.as_str()),
        ("#image", data::PROJECT_IMAGE),
    ] {
        session.page.locator(field).fill(value, None).await?;
    }
    button(&session.page, "Create project")
        .click(None)
        .await
        .context("create the project")?;
    expect_page(&session.page)
        .to_have_url_regex(&format!("/projects/{key}$"))
        .await?;
    // A task gives the row a count that is a link, whichever status the task has reached. The
    // project's harness image is not in the cluster, so the task is cancelled right away: its
    // sandbox would never start and would hold a pod of the quota. The test checks the cancelled
    // status itself, so it cancels here and not only in the teardown.
    session
        .create_task(env, key, "Count in the project list.", None)
        .await?;
    session.cancel_task().await?;
    expect(session.page.locator("#task-details"))
        .to_contain_text("cancelled")
        .await
        .context("the task is cancelled")?;

    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await?;
    let row = format!("tr:has(a[href='/projects/{key}'])");
    assert_project_table_fits(session, &row).await?;

    // The count link sits above the row's cover. `.first()`: the row has a count link per status.
    session
        .page
        .locator(format!("{row} a[href^='/tasks?project={key}&status=']"))
        .first()
        .click(None)
        .await
        .context("click a count")?;
    expect_page(&session.page)
        .to_have_url_regex(&format!("/tasks\\?project={key}&status="))
        .await
        .context("a count opens the filtered task list")?;

    // A click on the repository URL goes through the cover link.
    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await?;
    let repo = session
        .page
        .locator(format!("{row} span.truncate"))
        .bounding_box()
        .await?
        .context("box of the repository URL")?;
    session
        .page
        .mouse()
        .click(repo.x + 20.0, repo.y + repo.height / 2.0, None)
        .await?;
    expect_page(&session.page)
        .to_have_url_regex(&format!("/projects/{key}$"))
        .await
        .context("a click on the repository URL opens the project")
}

/// configuration-management, "Click into a credential reference row": a click on the details
/// line, outside the name, opens the edit dialog of that reference over the list.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_click_on_the_details_of_a_credential_row_opens_its_edit_dialog() -> Result<()> {
    fixture::run(
        "a_click_on_the_details_of_a_credential_row_opens_its_edit_dialog",
        credential_row_body,
    )
    .await
}

async fn credential_row_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(WIDE).await?;
    session
        .page
        .goto(&format!("{}/credentials", env.base_url), None)
        .await?;
    let details = session
        .page
        .locator(format!(
            "tr:has(button[aria-label='Edit credential reference {}']) span.font-mono",
            data::CREDENTIAL_NAME
        ))
        .bounding_box()
        .await?
        .context("box of the secret and key line")?;
    session
        .page
        .mouse()
        .click(details.x + 5.0, details.y + details.height / 2.0, None)
        .await?;
    expect(session.page.locator("#composer-slot input[name=name]"))
        .to_have_value(data::CREDENTIAL_NAME)
        .await
        .context("the edit dialog of the credential reference opens")?;
    expect_page(&session.page)
        .to_have_url_regex("/credentials$")
        .await
        .context("the list stays open under the dialog")
}

/// configuration-management, "Delete inside a row": the delete button asks for confirmation and
/// does not open the provider's settings page.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_delete_button_of_a_provider_row_confirms_and_does_not_open_the_page() -> Result<()> {
    fixture::run(
        "the_delete_button_of_a_provider_row_confirms_and_does_not_open_the_page",
        provider_delete_body,
    )
    .await
}

async fn provider_delete_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(WIDE).await?;
    let (messages, mut seen) = tokio::sync::mpsc::unbounded_channel::<String>();
    session
        .page
        .on_dialog(move |dialog| {
            let messages = messages.clone();
            async move {
                // The receiver only closes when the test is over.
                messages.send(dialog.message().to_owned()).ok();
                dialog.dismiss().await
            }
        })
        .await?;
    session
        .page
        .goto(&format!("{}/model-providers", env.base_url), None)
        .await?;
    let name = data::SETTINGS_PROVIDER_NAME;
    button(&session.page, &format!("Delete model provider {name}"))
        .click(None)
        .await
        .context("click delete")?;
    let message = tokio::time::timeout(std::time::Duration::from_secs(10), seen.recv())
        .await
        .context("the confirmation appears")?
        .context("the dialog handler is alive")?;
    assert_eq!(message, format!("Delete model provider {name}?"));
    expect_page(&session.page)
        .to_have_url_regex("/model-providers$")
        .await
        .context("the settings page did not open")?;
    expect(
        session
            .page
            .locator(format!("a[href^='/model-providers/']:has-text('{name}')")),
    )
    .to_have_count(1)
    .await
    .context("the provider is still listed")
}

/// configuration-management, "Saving the project settings page" and "Project settings tab in the
/// URL": a save stays on the page and says so, the tab is part of the URL and a URL opens its tab.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn saving_a_project_stays_on_its_page_and_tabs_follow_the_url() -> Result<()> {
    fixture::run(
        "saving_a_project_stays_on_its_page_and_tabs_follow_the_url",
        project_save_body,
    )
    .await
}

async fn project_save_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(WIDE).await?;
    open_project_page(session, env, data::PROJECT_KEY).await?;
    let page_url = session.page.url();

    button(&session.page, "Save project")
        .click(None)
        .await
        .context("save the project")?;
    expect(session.page.locator("#save-status"))
        .to_contain_text("Project saved")
        .await
        .context("the save is confirmed")?;
    assert_eq!(session.page.url(), page_url, "the page stays open");

    session.page.locator("#tab-security").click(None).await?;
    expect_page(&session.page)
        .to_have_url_regex("\\?tab=security$")
        .await
        .context("choosing a tab updates the URL")?;
    session
        .page
        .goto(
            &format!(
                "{}/projects/{}?tab=integrations",
                env.base_url,
                data::PROJECT_KEY
            ),
            None,
        )
        .await?;
    expect(session.page.locator("#tab-integrations"))
        .to_have_attribute("aria-selected", "true")
        .await
        .context("the URL opens the Integrations tab")
}

/// Opens the settings page of the seeded provider with an HTTPS upstream from its row in the list.
async fn open_provider_page(session: &Session, env: &E2eEnv) -> Result<()> {
    session
        .page
        .goto(&format!("{}/model-providers", env.base_url), None)
        .await?;
    // `.first()`: the provider's link exists in the desktop row and in the phone card.
    session
        .page
        .locator(format!(
            "a[href^='/model-providers/']:has-text('{}'):visible",
            data::SETTINGS_PROVIDER_NAME
        ))
        .first()
        .click(None)
        .await
        .context("open the settings page of the model provider")?;
    expect(session.page.locator("[role=tablist]"))
        .to_be_visible()
        .await
        .context("tab row of the model provider page")?;
    expect_page(&session.page)
        .to_have_url_regex("/model-providers/[0-9a-f-]+$")
        .await
        .context("URL of the model provider page")
}

/// Fills `field` on General, goes to the tab `last` and saves: the form comes back with the error
/// on General although the user was on `last` (design D3 of `ui-model-provider-page`).
async fn save_with_a_general_error(
    session: &Session,
    field: &str,
    bad: &str,
    last: &str,
    save: &str,
) -> Result<()> {
    session.page.locator("#tab-general").click(None).await?;
    session.page.locator(field).fill(bad, None).await?;
    session
        .page
        .locator(format!("#tab-{last}"))
        .click(None)
        .await?;
    expect_page(&session.page)
        .to_have_url_regex(&format!("\\?tab={last}$"))
        .await
        .context("the user is on the last tab")?;
    button(&session.page, save).click(None).await?;
    expect(session.page.locator("#tab-general"))
        .to_have_attribute("aria-selected", "true")
        .await
        .context("General opens with the error")?;
    expect(session.page.locator("[role=alert]:visible"))
        .to_be_visible()
        .await
        .context("the error is shown")?;
    expect_page(&session.page)
        .to_have_url_regex("/[^?]+$")
        .await
        .context("the URL loses ?tab")
}

/// model-providers, "Model provider settings page" and configuration-management, "Saving the
/// project settings page": a model provider is edited on its page, a save stays on it, and after
/// a failed save the tab with the error is open and named by the URL, on both settings pages.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_model_provider_page_saves_in_place_and_shows_the_tab_with_the_error() -> Result<()> {
    fixture::run(
        "the_model_provider_page_saves_in_place_and_shows_the_tab_with_the_error",
        provider_save_body,
    )
    .await
}

async fn provider_save_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    // Unique per run, and removed again below: a model left behind would shift the row of the
    // next run's new model.
    let model = world.unique("e2e-extra-model");
    session.page.set_viewport_size(WIDE).await?;
    open_provider_page(session, env).await?;
    let page_url = session.page.url();

    session.page.locator("#tab-models").click(None).await?;
    expect_page(&session.page)
        .to_have_url_regex("\\?tab=models$")
        .await
        .context("choosing Models updates the URL")?;
    let rows = session.page.locator("input[aria-label='Model ID']");
    let before = rows.count().await?;
    button(&session.page, "+ Add model").click(None).await?;
    // The form is rendered again with the new row; filling before that would write into the
    // old last row. `.last()`: the new row comes after the models of the provider.
    expect(rows.clone())
        .to_have_count(before + 1)
        .await
        .context("the form has a row for the new model")?;
    rows.last().fill(&model, None).await?;
    save_model_provider(session).await?;
    expect(session.page.locator(format!("input[value='{model}']")))
        .to_be_visible()
        .await
        .context("the added model is in the form after the save")?;
    assert_eq!(
        session.page.url(),
        format!("{page_url}?tab=models"),
        "the page stays open on the Models tab"
    );

    // Back to the seeded state: remove the model and save.
    button(&session.page, &format!("Remove model {model}"))
        .click(None)
        .await
        .context("remove the added model")?;
    save_model_provider(session).await?;
    expect(session.page.locator(format!("input[value='{model}']")))
        .to_have_count(0)
        .await
        .context("the added model is gone after the second save")?;

    save_with_a_general_error(
        session,
        "#url_anthropic_messages",
        "http://insecure.example.org",
        "models",
        "Save model provider",
    )
    .await
}

/// Saves the model provider form and waits for the confirmation. The status is cleared first: it
/// lies outside the form and still shows the confirmation of an earlier save.
async fn save_model_provider(session: &Session) -> Result<()> {
    session
        .page
        .evaluate_value("document.getElementById('save-status').textContent = ''")
        .await
        .context("clear the save status")?;
    button(&session.page, "Save model provider")
        .click(None)
        .await?;
    expect(session.page.locator("#save-status"))
        .to_contain_text("Model provider saved")
        .await
        .context("the save is confirmed")
}

/// configuration-management, "Saving the project settings page": the same for the project page
/// (TD-124): on `?tab=security` a save with an invalid repository URL opens General.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_failed_project_save_opens_the_general_tab_and_the_url_follows() -> Result<()> {
    fixture::run(
        "a_failed_project_save_opens_the_general_tab_and_the_url_follows",
        failed_project_save_body,
    )
    .await
}

async fn failed_project_save_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(WIDE).await?;
    open_project_page(session, env, data::PROJECT_KEY).await?;

    save_with_a_general_error(
        session,
        "#repo_url",
        "ssh://git.example.org/o/r.git",
        "security",
        "Save project",
    )
    .await
}

/// task-ui, "Task list sort": the sort menu changes the order and the URL.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_sort_menu_changes_the_order_and_the_url() -> Result<()> {
    fixture::run(
        "the_sort_menu_changes_the_order_and_the_url",
        sort_menu_body,
    )
    .await
}

async fn sort_menu_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(WIDE).await?;
    let mut keys = Vec::new();
    for name in ["older", "newer"] {
        let description = format!("Sort the list, {name}.");
        keys.push(
            session
                .create_task(env, data::NETWORK_PROJECT_KEY, &description, None)
                .await?,
        );
    }
    let (older, newer) = (&keys[0], &keys[1]);
    session
        .page
        .goto(
            &format!(
                "{}/tasks?status=all&project={}",
                env.base_url,
                data::NETWORK_PROJECT_KEY
            ),
            None,
        )
        .await?;

    button(&session.page, "Recently updated")
        .click(None)
        .await
        .context("open the sort menu")?;
    // Right of the 72 px rail, so the first option's text is not cut off.
    assert_menu_inside_viewport(session, RAIL_WIDTH).await?;
    menu_item(session, "Newest created")
        .click(None)
        .await
        .context("choose Newest created")?;
    expect_page(&session.page)
        .to_have_url_regex("sort=created")
        .await
        .context("the sort is part of the URL")?;
    expect(button(&session.page, "Newest created"))
        .to_be_visible()
        .await
        .context("the button names the current sort")?;
    expect(newer_listed_before_older(session, newer, older))
        .to_be_visible()
        .await
        .context("the newer task is listed before the older one")?;
    Ok(())
}

/// configuration-management, "Form tabs on phones": the chosen last tab ("Integrations", for an
/// admin) lies inside the tab row of the project settings page and the page does not scroll
/// sideways. The three tabs of an admin need 266 px, so the row overflows below 298 px; at 280 px
/// it does (248 px wide), and the tab has to scroll into view. The click is dispatched, not
/// performed: Playwright would scroll the tab into view itself before a real click.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_last_tab_of_the_project_page_stays_in_view_on_phones() -> Result<()> {
    fixture::run(
        "the_last_tab_of_the_project_page_stays_in_view_on_phones",
        last_tab_body,
    )
    .await
}

async fn last_tab_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    for width in [390, 280] {
        session
            .page
            .set_viewport_size(Viewport { width, height: 800 })
            .await?;
        open_project_page(session, env, data::PROJECT_KEY).await?;
        if width == 280 {
            let overflows = session
                .page
                .evaluate_value(
                    "(l => l.scrollWidth > l.clientWidth)(document.querySelector('[role=tablist]'))",
                )
                .await?;
            assert_eq!(overflows, "true", "the tab row fits at 280 px");
        }
        // `.last()`: the test is about the last tab of the row.
        let last = session.page.locator("[role=tab]:visible").last();
        last.dispatch_event("click", None).await?;
        expect(last.clone())
            .to_have_attribute("aria-selected", "true")
            .await?;

        let tab = last.bounding_box().await?.context("box of the last tab")?;
        let list = session
            .page
            .locator("[role=tablist]")
            .bounding_box()
            .await?
            .context("box of the tab row")?;
        // Half a pixel for subpixel layout.
        assert!(
            tab.x >= list.x - 0.5 && tab.x + tab.width <= list.x + list.width + 0.5,
            "{width} px: tab {tab:?} is outside the tab row {list:?}"
        );
        let overflows = session
            .page
            .evaluate_value("document.documentElement.scrollWidth > window.innerWidth")
            .await?;
        assert_eq!(overflows, "false", "{width} px: the page scrolls sideways");
    }
    Ok(())
}

/// Opening tasks from the list swaps `#ws-detail` and with it the task's event stream. The swap
/// must leave no error in the console: `hx-sse` used to reject an `AbortError` nobody handled
/// when htmx cleaned up the old stream. The swapped details must keep the attributes Alpine
/// binds (`data-tab` decides which details panel shows).
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab() -> Result<()> {
    fixture::run(
        "opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab",
        swap_tasks_body,
    )
    .await
}

async fn swap_tasks_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    let mut keys = Vec::new();
    for name in ["first", "second"] {
        let description = format!("Swap between tasks, {name}.");
        keys.push(
            session
                .create_task(env, data::NETWORK_PROJECT_KEY, &description, None)
                .await?,
        );
    }
    let (first, second) = (&keys[0], &keys[1]);
    session
        .page
        .goto(
            &format!(
                "{}/tasks/{first}?project={}",
                env.base_url,
                data::NETWORK_PROJECT_KEY
            ),
            None,
        )
        .await?;
    session.expect_text(data::NETWORK_NEED, RUN_TIMEOUT).await?;
    session.page.clear_console_messages();
    session.page.clear_page_errors();

    for key in [second, first, second] {
        session
            .page
            .locator(format!("#row-{key} a"))
            .click(None)
            .await
            .with_context(|| format!("open {key} from the list"))?;
        expect(session.page.locator("#ws-detail"))
            .to_contain_text(key)
            .await
            .with_context(|| format!("{key} swapped in"))?;
        // htmx removes `htmx-added` once the swap settled; a settle step used to reset the
        // swapped elements to the server's attributes, dropping what Alpine bound.
        expect(session.page.locator(".htmx-added"))
            .to_have_count(0)
            .await
            .with_context(|| format!("{key} settled"))?;
        expect(session.page.locator("#ws-details"))
            .to_have_attribute("data-tab", "overview")
            .await
            .with_context(|| format!("{key} keeps the open details tab"))?;
    }
    // A round trip after the last swap: a rejection of its cleanup has been reported by then.
    session.page.evaluate_value("1").await?;

    let errors: Vec<String> = session
        .page
        .console_messages()
        .iter()
        .filter(|message| message.type_() == "error")
        .map(|message| message.text().to_owned())
        .chain(session.page.page_errors())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    Ok(())
}

/// Creates two tasks of the network project and opens the first one with the list beside it.
/// Returns their keys.
async fn two_tasks_with_the_first_open(world: &World) -> Result<(String, String)> {
    let (env, session) = (&world.env, &world.session);
    let first = session
        .create_task(env, data::NETWORK_PROJECT_KEY, "Focus, first.", None)
        .await?;
    let second = session
        .create_task(env, data::NETWORK_PROJECT_KEY, "Focus, second.", None)
        .await?;
    session
        .page
        .goto(
            &format!(
                "{}/tasks/{first}?project={}",
                env.base_url,
                data::NETWORK_PROJECT_KEY
            ),
            None,
        )
        .await?;
    expect(session.page.locator("#task-head"))
        .to_contain_text(&first)
        .await?;
    Ok((first, second))
}

/// task-ui, "Task selected from the list": a keyboard activation of a row puts focus on the head
/// of that task, with a visible focus outline.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn selecting_a_task_with_the_keyboard_focuses_its_head_with_a_visible_outline() -> Result<()>
{
    fixture::run(
        "selecting_a_task_with_the_keyboard_focuses_its_head_with_a_visible_outline",
        |world| async move {
            let session = &world.session;
            let (first, second) = two_tasks_with_the_first_open(&world).await?;

            let row = session.page.locator(format!("#row-{second} a"));
            row.focus().await?;
            row.press("Enter", None).await?;

            expect(session.page.locator("#task-head"))
                .to_contain_text(&second)
                .await
                .with_context(|| format!("{second} replaces {first}"))?;
            expect(session.page.locator("#task-head"))
                .to_be_focused()
                .await
                .context("focus is on the head of the opened task")?;
            let outline = session
                .evaluate(
                    "read the outline of the focused head",
                    "getComputedStyle(document.activeElement).outlineStyle",
                )
                .await?;
            assert!(
                !outline.contains("none"),
                "visible focus outline: {outline}"
            );
            Ok(())
        },
    )
    .await
}

/// task-ui, "Selecting a task fails" and "Failed selection keeps focus": the load of the selected
/// task is answered by the server with an error (the request is sent to a task that does not
/// exist). A toast appears, the open task stays shown, the URL is the same and focus stays on
/// the row.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_failed_selection_shows_a_toast_and_keeps_the_open_task_the_url_and_the_focus()
-> Result<()> {
    fixture::run(
        "a_failed_selection_shows_a_toast_and_keeps_the_open_task_the_url_and_the_focus",
        |world| async move {
            let session = &world.session;
            let (first, second) = two_tasks_with_the_first_open(&world).await?;
            // A real 404 rather than the 500 of the scenario: it takes the same `error_answers`
            // path, and the unit tests of `df-web` (`htmx/tests.rs`) cover the 500.
            let missing = format!(
                "{}/tasks/{}-404",
                world.env.base_url,
                data::NETWORK_PROJECT_KEY
            );
            session
                .page
                .route(&format!("**/tasks/{second}?*"), move |route| {
                    let missing = missing.clone();
                    async move {
                        route
                            .continue_(Some(ContinueOptions::builder().url(missing).build()))
                            .await
                    }
                })
                .await?;
            let url = session.page.url();

            let next = session.page.locator(format!("#row-{second} a"));
            next.focus().await?;
            next.press("Enter", None).await?;

            expect(session.page.locator("#toasts"))
                .to_contain_text("404 Not found")
                .await
                .context("the failed selection shows a toast")?;
            assert_eq!(session.page.url(), url, "the URL stays");
            expect(session.page.locator("#ws-detail"))
                .to_contain_text(&first)
                .await
                .context("the open task stays shown")?;
            expect(next)
                .to_be_focused()
                .await
                .context("focus stays on the row")?;
            Ok(())
        },
    )
    .await
}

/// task-ui, "Live update does not move focus": another viewer renames the task, which replaces
/// the task head on this page while the cursor is in the message input.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_live_update_of_the_task_head_leaves_the_focus_in_the_message_input() -> Result<()> {
    fixture::run(
        "a_live_update_of_the_task_head_leaves_the_focus_in_the_message_input",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            let (first, _second) = two_tasks_with_the_first_open(&world).await?;
            let input = session.page.locator("#message");
            input.focus().await?;
            expect(input.clone()).to_be_focused().await?;

            let renamer = Session::start_logged_in(env).await?;
            renamer.open_task(env, &first).await?;
            let title = world.unique("Renamed while typing");
            renamer
                .page
                .get_by_role(
                    AriaRole::Button,
                    Some(GetByRoleOptions::default().name("Rename task")),
                )
                .click(None)
                .await?;
            renamer.page.locator("#title").fill(&title, None).await?;
            button_in(&renamer.page.locator("#task-head"), "Save")
                .click(None)
                .await?;

            expect(session.page.locator("#task-head"))
                .to_contain_text(&title)
                .await
                .context("the live update replaced the head")?;
            expect(input)
                .to_be_focused()
                .await
                .context("focus stays in the message input")?;
            renamer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// web-ui-shell, "Live updates reconnect": the live connection of the task page drops and
/// returns; no toast appears.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_dropped_and_restored_live_connection_shows_no_toast() -> Result<()> {
    fixture::run(
        "a_dropped_and_restored_live_connection_shows_no_toast",
        |world| async move {
            let session = &world.session;
            let (first, _second) = two_tasks_with_the_first_open(&world).await?;

            session.set_offline(true).await?;
            let extra =
                db::raise_text_question(&world.pool, &first, &world.unique("While offline?"))
                    .await?;
            session.set_offline(false).await?;

            // The request raised while offline can only arrive through the reconnect.
            expect(session.request_card(extra))
                .with_timeout(RECONNECT_TIMEOUT)
                .to_be_visible()
                .await?;
            expect(session.page.locator("#toasts > *"))
                .to_have_count(0)
                .await
                .context("no toast after the reconnect")?;
            Ok(())
        },
    )
    .await
}

/// task-ui, "Toast region on every page": at 390 px an error toast lies above the floating
/// create button and the bottom bar.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn on_a_phone_an_error_toast_sits_above_the_create_button_and_the_bottom_bar() -> Result<()> {
    fixture::run(
        "on_a_phone_an_error_toast_sits_above_the_create_button_and_the_bottom_bar",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            session
                .page
                .set_viewport_size(Viewport {
                    width: 390,
                    height: 800,
                })
                .await?;
            session
                .page
                .goto(&format!("{}/tasks", env.base_url), None)
                .await?;
            // The create button's request goes to a task that does not exist: the server's 404.
            let missing = format!("{}/tasks/{}-404", env.base_url, data::PROJECT_KEY);
            session
                .page
                .route("**/tasks/compose*", move |route| {
                    let missing = missing.clone();
                    async move {
                        route
                            .continue_(Some(ContinueOptions::builder().url(missing).build()))
                            .await
                    }
                })
                .await?;
            // By role and name only the visible button matches: the rail's is hidden on a phone.
            let create = button(&session.page, "New task");
            create.click(None).await?;
            let toast = session.page.locator("#toasts > *");
            expect(toast.clone()).to_be_visible().await?;

            let toast = toast.bounding_box().await?.context("box of the toast")?;
            let create = create
                .bounding_box()
                .await?
                .context("box of the create button")?;
            let bar = session
                .page
                .locator("#mobile-menu")
                .bounding_box()
                .await?
                .context("box of the bottom bar")?;
            let bottom = toast.y + toast.height;
            assert!(
                bottom <= create.y,
                "toast ends at {bottom}, button starts at {}",
                create.y
            );
            assert!(
                bottom <= bar.y,
                "toast ends at {bottom}, bar starts at {}",
                bar.y
            );
            Ok(())
        },
    )
    .await
}

/// web-ui-shell, "Unreachable server shows a toast": a message post that gets no answer shows
/// the network toast, and the input keeps its text.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_message_post_without_an_answer_shows_the_network_toast_and_keeps_the_text() -> Result<()>
{
    fixture::run(
        "a_message_post_without_an_answer_shows_the_network_toast_and_keeps_the_text",
        |world| async move {
            let (env, session) = (&world.env, &world.session);
            let key = session
                .create_task(env, data::NETWORK_PROJECT_KEY, "Network toast.", None)
                .await?;
            session.open_task(env, &key).await?;
            session
                .page
                .route(
                    "**/messages",
                    |route| async move { route.abort(None).await },
                )
                .await?;

            let text = world.unique("keep this text");
            session.send_message(&text).await?;

            expect(session.page.locator("#toasts"))
                .to_contain_text("Could not reach the server")
                .await
                .context("the network toast")?;
            expect(session.page.locator("#message"))
                .to_have_value(&text)
                .await
                .context("the input keeps its text")?;
            Ok(())
        },
    )
    .await
}

/// task-ui, "Sort on a phone": at 390 px the sort menu is in the title row, choosing an option
/// sorts the list and the page does not scroll sideways.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_sort_menu_on_a_phone_sorts_the_list_without_sideways_scrolling() -> Result<()> {
    fixture::run(
        "the_sort_menu_on_a_phone_sorts_the_list_without_sideways_scrolling",
        phone_sort_menu_body,
    )
    .await
}

async fn phone_sort_menu_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session
        .page
        .set_viewport_size(Viewport {
            width: 390,
            height: 800,
        })
        .await?;
    let mut keys = Vec::new();
    for name in ["older", "newer"] {
        let description = format!("Sort the list on a phone, {name}.");
        keys.push(
            session
                .create_task(env, data::NETWORK_PROJECT_KEY, &description, None)
                .await?,
        );
    }
    let (older, newer) = (&keys[0], &keys[1]);
    session
        .page
        .goto(
            &format!(
                "{}/tasks?status=all&project={}",
                env.base_url,
                data::NETWORK_PROJECT_KEY
            ),
            None,
        )
        .await?;

    // The phone shows the sort button once, in the title row.
    button(&session.page, "Recently updated")
        .click(None)
        .await
        .context("open the sort menu in the title row")?;
    assert_menu_inside_viewport(session, 0.0).await?;
    menu_item(session, "Newest created")
        .click(None)
        .await
        .context("choose Newest created")?;
    expect_page(&session.page)
        .to_have_url_regex("sort=created")
        .await
        .context("the sort is part of the URL")?;
    expect(newer_listed_before_older(session, newer, older))
        .to_be_visible()
        .await
        .context("the newer task is listed before the older one")?;
    assert_no_sideways_scroll(&session.page, "at 390 px").await
}

/// egress-log: pause and resume, the divider (keyboard, reset, drag, remembered width) and the
/// details panel, on the live events of a task whose fetch the proxy keeps denying.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn traffic_page_pauses_resizes_and_opens_details() -> Result<()> {
    fixture::run(
        "traffic_page_pauses_resizes_and_opens_details",
        traffic_page_body,
    )
    .await
}

async fn traffic_page_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    // The script retries the denied fetch once a second until the request is decided.
    let key = session
        .create_task(env, data::NETWORK_PROJECT_KEY, "Live traffic.", None)
        .await?;
    session.expect_text(data::NETWORK_NEED, RUN_TIMEOUT).await?;
    let url = format!("{}/egress/log?task={key}", env.base_url);
    session.page.set_viewport_size(WIDE).await?;
    // Reloaded until the backlog holds the first denied fetch.
    eventually("the first event of the task", RUN_TIMEOUT, || async {
        session.page.goto(&url, None).await?;
        Ok((rows(session).await? > 0).then_some(()))
    })
    .await?;

    Box::pin(pause_and_resume(session, env, &url)).await?;
    divider(session).await?;
    details(session).await?;
    Ok(())
}

async fn rows(session: &Session) -> Result<usize> {
    Ok(session.page.locator(ROWS).count().await?)
}

/// Waits until the events table has at least `count` rows; returns the number it has then.
async fn wait_for_rows(session: &Session, count: usize) -> Result<usize> {
    eventually(&format!("at least {count} rows"), RUN_TIMEOUT, || async {
        let rows = rows(session).await?;
        Ok((rows >= count).then_some(rows))
    })
    .await
}

/// Pause aborts the stream and the page stops changing while new events arrive; Resume brings
/// them in. A second viewer sees the new events in the buffered backlog of a fresh page load, so
/// the check does not rest on a live stream.
async fn pause_and_resume(session: &Session, env: &E2eEnv, url: &str) -> Result<()> {
    // The rows come with the page; the stream connects after it.
    eventually("the stream of the traffic page", RUN_TIMEOUT, || async {
        let connected = session
            .page
            .evaluate_value("!!document.querySelector('#sse-slot > *')?._htmx?.sse")
            .await?;
        Ok((connected == "true").then_some(()))
    })
    .await?;
    // The extension's handle on the request, kept to check that Pause aborted it.
    session
        .page
        .evaluate_value(
            "(window.pausedStream = document.querySelector('#sse-slot > *')._htmx.sse.abortController, 1)",
        )
        .await
        .context("the connected stream of the traffic page")?;
    let badge = session.page.locator("header [role=status]");
    button(&session.page, "Pause").click(None).await?;
    expect(badge.clone()).to_contain_text("Paused").await?;
    let aborted = session
        .page
        .evaluate_value("window.pausedStream.signal.aborted")
        .await?;
    assert_eq!(aborted, "true", "Pause aborts the stream's request");
    let streams = session
        .page
        .evaluate_value("document.getElementById('sse-slot').children.length")
        .await?;
    assert_eq!(streams, "0", "Pause removes the stream element");
    let paused = rows(session).await?;

    // The denied retries go on (up to 240 s after the task started, far longer than this takes).
    let watcher = Session::start_logged_in(env).await?;
    let buffered = eventually("an event after the pause", RUN_TIMEOUT, || async {
        watcher.page.goto(url, None).await?;
        let count = rows(&watcher).await?;
        Ok((count > paused).then_some(count))
    })
    .await?;
    assert_eq!(rows(session).await?, paused, "rows changed while paused");

    button(&session.page, "Resume").click(None).await?;
    expect(badge).to_contain_text("Live").await?;
    wait_for_rows(session, buffered).await?;
    Ok(())
}

async fn divider_value(session: &Session, attribute: &str) -> Result<String> {
    session
        .page
        .locator(DIVIDER)
        .get_attribute(attribute)
        .await?
        .with_context(|| format!("{attribute} of the divider"))
}

/// Arrow key, double-click reset, drag up to the limit, and the width kept across a reload.
async fn divider(session: &Session) -> Result<()> {
    let divider = session.page.locator(DIVIDER);
    let initial: u32 = divider_value(session, "aria-valuenow").await?.parse()?;

    divider.focus().await?;
    divider.press("ArrowRight", None).await?;
    expect(divider.clone())
        .to_have_attribute("aria-valuenow", &(initial + DIVIDER_STEP).to_string())
        .await
        .context("Right Arrow grows the graph by one step")?;

    divider.dblclick(None).await?;
    expect(divider.clone())
        .to_have_attribute("aria-valuenow", &initial.to_string())
        .await
        .context("double-click restores the default width")?;

    // Dropped near the right edge of the events panel: the table keeps its minimum width.
    let events = session
        .page
        .locator("#events")
        .bounding_box()
        .await?
        .context("box of the events panel")?;
    divider
        .drag_to(
            &session.page.locator("#events"),
            DragToOptions::builder()
                .target_position(Position {
                    x: events.width - 10.0,
                    y: events.height / 2.0,
                })
                .build(),
        )
        .await
        .context("drag the divider")?;
    let max = divider_value(session, "aria-valuemax").await?;
    expect(divider.clone())
        .to_have_attribute("aria-valuenow", &max)
        .await
        .context("dragging stops at the limit")?;
    assert_ne!(max, initial.to_string(), "the drag did not resize");

    session.page.reload(None).await?;
    let divider = session.page.locator(DIVIDER);
    expect(divider.clone())
        .to_have_attribute("aria-valuenow", &max)
        .await
        .context("the width is remembered across a reload")?;
    // Back to the default: at the limit the table is too narrow for the inline row actions.
    divider.dblclick(None).await?;
    expect(divider)
        .to_have_attribute("aria-valuenow", &initial.to_string())
        .await
        .context("double-click after the reload restores the default width")
}

/// The details of a row open in a panel; Escape closes it and focus returns to the row.
async fn details(session: &Session) -> Result<()> {
    wait_for_rows(session, 1).await?;
    // By label (host and time): a live update re-renders the rows. CSS and `.first()`: the label
    // is not known yet, the rows are newest first.
    let label = session
        .page
        .locator("#flows button[aria-label^='Request details for']:visible")
        .first()
        .get_attribute("aria-label")
        .await?
        .context("label of the newest row's details button")?;
    let opener = button(&session.page, &label);
    opener.click(None).await?;
    let panel = session.page.locator("aside[role=dialog]");
    expect(panel.clone()).to_be_visible().await?;
    expect(session.page.locator("#flow-title"))
        .to_contain_text(data::NETWORK_HOST)
        .await?;

    session.page.keyboard().press("Escape", None).await?;
    expect(panel).to_be_hidden().await?;
    let focused = session
        .page
        .evaluate_value("document.activeElement.getAttribute('aria-label')")
        .await?;
    assert_eq!(focused, label, "focus returns to the row's details button");
    Ok(())
}
