//! Client-side behaviour of the web UI in a real browser (`just e2e-local`): tab rows on phones,
//! task swaps without console errors and the network traffic page. Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::wait::RUN_TIMEOUT;
use df_e2e::{E2eEnv, data};
use playwright_rs::{Viewport, expect};

/// What the first turn of the `network-access` script says; it then holds the turn.
const NEED: &str = "I need the package index from pypi.org.";

/// Opens the dialog of project `key` from the projects page.
async fn open_project_dialog(session: &Session, env: &E2eEnv, key: &str) -> Result<()> {
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
        .with_context(|| format!("open the dialog of project {key}"))?;
    expect(session.page.locator("[role=tablist]"))
        .to_be_visible()
        .await
        .context("tab row of the project dialog")
}

/// configuration-management, "Form tabs on phones": the chosen last tab lies inside the tab row
/// and the page does not scroll sideways. At 300 px the row overflows, so the tab has to scroll
/// into view; at 390 px it does once the dialog has a fourth tab. The click is dispatched, not
/// performed: Playwright would scroll the tab into view itself before a real click.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_last_tab_of_the_project_dialog_stays_in_view_on_phones() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    for width in [390, 300] {
        session
            .page
            .set_viewport_size(Viewport { width, height: 800 })
            .await?;
        open_project_dialog(&session, &env, data::PROJECT_KEY).await?;
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
/// when htmx cleaned up the old stream.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn opening_tasks_from_the_list_leaves_a_clean_console() -> Result<()> {
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
