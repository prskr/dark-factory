//! The iteration 1 demo through the web UI (`just e2e-local`). Needs the environment of
//! `E2eEnv`, so the tests are ignored by default.

use df_e2e::browser::{Session, button};
use df_e2e::data;
use df_e2e::fixture;
use playwright_rs::protocol::SelectOption;
use playwright_rs::{Viewport, expect};

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn login_shows_the_demo_project_on_the_board() -> anyhow::Result<()> {
    fixture::run(
        "login_shows_the_demo_project_on_the_board",
        |world| async move {
            // The fixture's session starts with the login shared by the binary; this test checks
            // the login itself, so it submits the Dex form in a context of its own.
            let session = Session::start(&world.env).await?;
            session.login(&world.env).await?;
            // The project filter of the board lists the seeded projects as `KEY · <open tasks>`.
            button(&session.page, "All projects")
                // `.first()`: the board has the filter once per layout (desktop and phone).
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
            session.close_page().await;
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn settings_sidebar_shows_on_desktop_and_a_phone_has_none_and_no_horizontal_scroll()
-> anyhow::Result<()> {
    fixture::run(
        "settings_sidebar_shows_on_desktop_and_a_phone_has_none_and_no_horizontal_scroll",
        |world| async move {
            let page = &world.session.page;
            let personas = format!("{}/personas", world.env.base_url);
            page.set_viewport_size(Viewport {
                width: 1280,
                height: 800,
            })
            .await?;
            page.goto(&personas, None).await?;
            expect(page.locator("#settings-menu"))
                .to_be_visible()
                .await?;

            page.set_viewport_size(Viewport {
                width: 390,
                height: 800,
            })
            .await?;
            page.goto(&personas, None).await?;

            expect(page.locator("h1:has-text('Personas')"))
                .to_be_visible()
                .await?;
            expect(page.locator("#settings-menu"))
                .to_be_hidden()
                .await?;
            let overflows = page
                .evaluate_value(
                    "document.documentElement.scrollWidth > document.documentElement.clientWidth",
                )
                .await?;
            assert_eq!(overflows, "false", "the page scrolls horizontally");
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_persona_is_created_with_a_model_of_its_provider() -> anyhow::Result<()> {
    fixture::run(
        "a_persona_is_created_with_a_model_of_its_provider",
        |world| async move {
            let page = &world.session.page;
            let name = world.unique("e2e-models");
            page.goto(&format!("{}/personas", world.env.base_url), None)
                .await?;

            page.locator("button[hx-get='/personas/new']:visible")
                // `.first()`: the list has the button once per layout (desktop and phone); the
                // visible one is used.
                .first()
                .click(None)
                .await?;
            page.locator("#name").fill(&name, None).await?;
            page.locator("#system_prompt")
                .fill("Check the model select.", None)
                .await?;
            // The provider and the model select are filled from the seeded provider and its
            // model; the model is required.
            page.locator("#model_provider_id")
                .select_option(
                    SelectOption::Label(data::MODEL_PROVIDER_NAME.to_string()),
                    None,
                )
                .await?;
            page.locator("#model")
                .select_option(
                    SelectOption::Label(format!("{} (Anthropic Messages)", data::PERSONA_MODEL)),
                    None,
                )
                .await?;
            button(page, "Create persona").click(None).await?;

            expect(page.get_by_text(&name, false))
                .to_be_visible()
                .await?;
            Ok(())
        },
    )
    .await
}
