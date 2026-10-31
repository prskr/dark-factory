//! The iteration 1 demo through the web UI (`just e2e-local`). Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use df_e2e::browser::Session;
use df_e2e::{E2eEnv, data};
use playwright_rs::expect;

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
