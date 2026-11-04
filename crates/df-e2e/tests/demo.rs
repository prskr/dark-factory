//! The iteration 1 demo through the web UI (`just e2e-local`). Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use df_e2e::browser::Session;
use df_e2e::{E2eEnv, data};
use playwright_rs::{Viewport, expect};

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn login_shows_the_demo_project_on_the_board() -> anyhow::Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;

    session.login(&env).await?;

    // The project filter of the board lists the seeded projects as `KEY · <open tasks>`.
    session
        .page
        .locator("button:has-text('All projects')")
        .first()
        .click(None)
        .await?;
    expect(
        session
            .page
            .get_by_text(&format!("{} ·", data::PROJECT_KEY), false),
    )
    .to_be_visible()
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn settings_sidebar_shows_on_desktop_and_a_phone_has_none_and_no_horizontal_scroll()
-> anyhow::Result<()> {
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;

    session
        .page
        .set_viewport_size(Viewport {
            width: 1280,
            height: 800,
        })
        .await?;
    session
        .page
        .goto(&format!("{}/personas", env.base_url), None)
        .await?;
    expect(session.page.locator("#settings-menu"))
        .to_be_visible()
        .await?;

    session
        .page
        .set_viewport_size(Viewport {
            width: 390,
            height: 800,
        })
        .await?;
    session
        .page
        .goto(&format!("{}/personas", env.base_url), None)
        .await?;

    expect(session.page.locator("h1:has-text('Personas')"))
        .to_be_visible()
        .await?;
    expect(session.page.locator("#settings-menu"))
        .to_be_hidden()
        .await?;
    let overflows = session
        .page
        .evaluate_value(
            "document.documentElement.scrollWidth > document.documentElement.clientWidth",
        )
        .await?;
    assert_eq!(overflows, "false", "the page scrolls horizontally");
    Ok(())
}
