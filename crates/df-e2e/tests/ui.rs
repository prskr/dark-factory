//! Client-side behaviour of the web UI in a real browser (`just e2e-local`): tab rows on phones,
//! task swaps without console errors and the network traffic page. Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use anyhow::{Context, Result};
use df_e2e::browser::Session;
use df_e2e::{E2eEnv, data};
use playwright_rs::{Viewport, expect};

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
