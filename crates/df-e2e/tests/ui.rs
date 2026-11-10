//! Client-side behaviour of the web UI in a real browser (`just e2e-local`): the settings area,
//! the project settings page and its tabs, the task list sort menu, tab rows on phones, task swaps
//! without console errors and the network traffic page. Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data};
use playwright_rs::protocol::{DragToOptions, Position};
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

/// Opens the settings page of project `key` from the edit action of the projects page.
async fn open_project_page(session: &Session, env: &E2eEnv, key: &str) -> Result<()> {
    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await?;
    session
        .page
        .locator(format!("[aria-label='Edit project {key}']:visible"))
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
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;

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

/// configuration-management, "Saving the project settings page" and "Project settings tab in the
/// URL": a save stays on the page and says so, the tab is part of the URL and a URL opens its tab.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn saving_a_project_stays_on_its_page_and_tabs_follow_the_url() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    session.page.set_viewport_size(WIDE).await?;
    open_project_page(&session, &env, data::PROJECT_KEY).await?;
    let page_url = session.page.url();

    session
        .page
        .locator("button:has-text('Save project'):visible")
        .click(None)
        .await
        .context("save the project")?;
    expect(session.page.locator("#save-status"))
        .to_contain_text("Project saved")
        .await
        .context("the save is confirmed")?;
    assert_eq!(session.page.url(), page_url, "the page stays open");

    session.page.locator("#tab-push-checks").click(None).await?;
    expect_page(&session.page)
        .to_have_url_regex("\\?tab=push-checks$")
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

/// task-ui, "Task list sort": the sort menu changes the order and the URL.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_sort_menu_changes_the_order_and_the_url() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    session.page.set_viewport_size(WIDE).await?;
    let mut keys = Vec::new();
    for name in ["older", "newer"] {
        let description = format!("Sort the list, {name}.");
        keys.push(
            session
                .create_task(&env, data::NETWORK_PROJECT_KEY, &description, None)
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

    session
        .page
        .locator("button[aria-label='Recently updated']:visible")
        .click(None)
        .await
        .context("open the sort menu")?;
    // Right of the 72 px rail, so the first option's text is not cut off.
    assert_menu_inside_viewport(&session, RAIL_WIDTH).await?;
    session
        .page
        .locator("[role=menuitemradio]:has-text('Newest created')")
        .click(None)
        .await
        .context("choose Newest created")?;
    expect_page(&session.page)
        .to_have_url_regex("sort=created")
        .await
        .context("the sort is part of the URL")?;
    expect(
        session
            .page
            .locator("button[aria-label='Newest created']:visible"),
    )
    .to_be_visible()
    .await
    .context("the button names the current sort")?;
    let rows = session.page.locator("#task-rows li[id^=row-]");
    expect(rows.first())
        .to_have_attribute("id", &format!("row-{newer}"))
        .await
        .context("the newest task is first")?;
    let position = |key: &str| format!("#task-rows #row-{key}");
    assert!(
        session.page.locator(position(older)).count().await? == 1,
        "the older task is listed"
    );

    for key in &keys {
        session.open_task(&env, key).await?;
        session.cancel_task().await?;
    }
    Ok(())
}

/// configuration-management, "Form tabs on phones": the chosen last tab ("Integrations", for an
/// admin) lies inside the tab row of the project settings page and the page does not scroll
/// sideways. At 300 px the row overflows, so the tab has to scroll into view. The click is
/// dispatched, not performed: Playwright would scroll the tab into view itself before a real click.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_last_tab_of_the_project_page_stays_in_view_on_phones() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    for width in [390, 300] {
        session
            .page
            .set_viewport_size(Viewport { width, height: 800 })
            .await?;
        open_project_page(&session, &env, data::PROJECT_KEY).await?;
        if width == 300 {
            let overflows = session
                .page
                .evaluate_value(
                    "(l => l.scrollWidth > l.clientWidth)(document.querySelector('[role=tablist]'))",
                )
                .await?;
            assert_eq!(overflows, "true", "the tab row fits at 300 px");
        }
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
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let mut keys = Vec::new();
    for name in ["first", "second"] {
        let description = format!("Swap between tasks, {name}.");
        keys.push(
            session
                .create_task(&env, data::NETWORK_PROJECT_KEY, &description, None)
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

    for key in &keys {
        session.open_task(&env, key).await?;
        session.cancel_task().await?;
    }
    Ok(())
}

/// task-ui, "Sort on a phone": at 390 px the sort menu is in the title row, choosing an option
/// sorts the list and the page does not scroll sideways.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_sort_menu_on_a_phone_sorts_the_list_without_sideways_scrolling() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
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
                .create_task(&env, data::NETWORK_PROJECT_KEY, &description, None)
                .await?,
        );
    }
    let (_, newer) = (&keys[0], &keys[1]);
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

    session
        .page
        .locator("section.ws-list div:has(> h1) button[aria-label='Recently updated']:visible")
        .click(None)
        .await
        .context("open the sort menu in the title row")?;
    assert_menu_inside_viewport(&session, 0.0).await?;
    session
        .page
        .locator("[role=menuitemradio]:has-text('Newest created'):visible")
        .click(None)
        .await
        .context("choose Newest created")?;
    expect_page(&session.page)
        .to_have_url_regex("sort=created")
        .await
        .context("the sort is part of the URL")?;
    expect(session.page.locator("#task-rows li[id^=row-]").first())
        .to_have_attribute("id", &format!("row-{newer}"))
        .await
        .context("the newest task is first")?;
    let overflows = session
        .page
        .evaluate_value(
            "document.documentElement.scrollWidth > document.documentElement.clientWidth",
        )
        .await?;
    assert_eq!(overflows, "false", "the page scrolls sideways at 390 px");

    // The status menu that cancels a task is in the details panel, which phones don't show.
    session.page.set_viewport_size(WIDE).await?;
    for key in &keys {
        session.open_task(&env, key).await?;
        session.cancel_task().await?;
    }
    Ok(())
}

/// egress-log: pause and resume, the divider (keyboard, reset, drag, remembered width) and the
/// details panel, on the live events of a task whose fetch the proxy keeps denying.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn traffic_page_pauses_resizes_and_opens_details() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    // The script retries the denied fetch once a second until the request is decided.
    let key = session
        .create_task(&env, data::NETWORK_PROJECT_KEY, "Live traffic.", None)
        .await?;
    session.expect_text(data::NETWORK_NEED, RUN_TIMEOUT).await?;
    let url = format!("{}/egress/log?task={key}", env.base_url);
    session.page.set_viewport_size(WIDE).await?;
    // Reloaded until the backlog holds the first denied fetch.
    eventually("the first event of the task", RUN_TIMEOUT, || async {
        session.page.goto(&url, None).await?;
        Ok((rows(&session).await? > 0).then_some(()))
    })
    .await?;

    pause_and_resume(&session, &env, &url).await?;
    divider(&session).await?;
    details(&session).await?;

    session.open_task(&env, &key).await?;
    session.cancel_task().await?;
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
    session
        .page
        .locator("button:has-text('Pause')")
        .click(None)
        .await?;
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
    let watcher = Session::start(env).await?;
    watcher.login(env).await?;
    let buffered = eventually("an event after the pause", RUN_TIMEOUT, || async {
        watcher.page.goto(url, None).await?;
        let count = rows(&watcher).await?;
        Ok((count > paused).then_some(count))
    })
    .await?;
    assert_eq!(rows(session).await?, paused, "rows changed while paused");

    session
        .page
        .locator("button:has-text('Resume')")
        .click(None)
        .await?;
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
    // By label (host and time): a live update re-renders the rows.
    let label = session
        .page
        .locator("#flows button[aria-label^='Request details for']:visible")
        .first()
        .get_attribute("aria-label")
        .await?
        .context("label of the newest row's details button")?;
    let opener = session
        .page
        .locator(format!("#flows button[aria-label='{label}']:visible"));
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
