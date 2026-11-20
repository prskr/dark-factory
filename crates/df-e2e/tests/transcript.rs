//! The transcript of a task in a real browser (`just e2e-local`): where it ends when request cards
//! are pending, and tool call groups that stay open while another call arrives. Needs the
//! environment of `E2eEnv`, so the tests are ignored by default.

use std::sync::Arc;

use anyhow::{Context, Result};
use df_e2e::fixture::{self, World};
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT};
use df_e2e::{data, db};
use playwright_rs::protocol::{AriaRole, GetByRoleOptions};
use playwright_rs::{Viewport, expect};

const DESKTOP: Viewport = Viewport {
    width: 1440,
    height: 900,
};
const PHONE: Viewport = Viewport {
    width: 390,
    height: 844,
};

/// What the first turn of the `[blocking-question]` script asks.
const QUESTION: &str = "Which greeting should the file contain?";

/// task-ui, "Transcript follows the latest item", scenarios "Pending question at the end" and
/// "Pending question arrives live": a transcript that ends with a pending question and a pending
/// network access approval leaves no blank area after the card and the page does not scroll, on a
/// desktop and on a phone, when the cards arrive while the page is open and after a reload.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn pending_cards_at_the_end_leave_no_blank_area_and_the_page_does_not_scroll() -> Result<()> {
    fixture::run(
        "pending_cards_at_the_end_leave_no_blank_area_and_the_page_does_not_scroll",
        pending_cards_body,
    )
    .await
}

async fn pending_cards_body(world: Arc<World>) -> Result<()> {
    let session = &world.session;
    let network = format!(
        "network access to {}:{}",
        data::NETWORK_HOST,
        data::NETWORK_PORT
    );
    session.page.set_viewport_size(DESKTOP).await?;
    let key = session
        .create_task(
            &world.env,
            data::NETWORK_PROJECT_KEY,
            "[blocking-question] Ask before fetching.",
            None,
        )
        .await?;

    // The cards arrive over the open page's stream.
    session
        .expect_region_text("#transcript", QUESTION, RUN_TIMEOUT)
        .await?;
    session
        .expect_region_text("#transcript", &network, UI_TIMEOUT)
        .await?;
    session.expect_transcript_ends_at_last_item().await?;

    // The same transcript opened from scratch, wide and narrow.
    for viewport in [DESKTOP, PHONE] {
        session.page.set_viewport_size(viewport).await?;
        session.open_task(&world.env, &key).await?;
        session
            .expect_region_text("#transcript", &network, UI_TIMEOUT)
            .await?;
        session.expect_transcript_ends_at_last_item().await?;
    }
    Ok(())
}

/// task-ui, "Expanding a tool call group", scenario "Group grows while open": the group and a
/// nested row are open, a tool call arrives, and the count grows with both still open.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn an_open_tool_call_group_and_row_stay_open_while_another_call_arrives() -> Result<()> {
    fixture::run(
        "an_open_tool_call_group_and_row_stay_open_while_another_call_arrives",
        group_body,
    )
    .await
}

async fn group_body(world: Arc<World>) -> Result<()> {
    let session = &world.session;
    session.page.set_viewport_size(DESKTOP).await?;
    let key = session
        .create_task(
            &world.env,
            data::NETWORK_PROJECT_KEY,
            "Fetch the package index",
            None,
        )
        .await?;
    // The agent's first turn is on its held fetch and writes nothing more, so the streak below
    // is the end of the transcript.
    session.expect_text(data::NETWORK_NEED, RUN_TIMEOUT).await?;

    db::append_tool_call(&world.pool, &key, 1, "streak-1", "Read src/lib.rs").await?;
    db::append_tool_call(&world.pool, &key, 2, "streak-2", "Read src/main.rs").await?;
    let transcript = session.page.locator("#transcript");
    // The head of the group is the button named by its count.
    let head = transcript.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name("tool calls")),
    );
    expect(head.clone())
        .with_timeout(UI_TIMEOUT)
        .to_contain_text("2 tool calls")
        .await
        .context("the group of the seeded streak")?;
    let first_row = transcript.get_by_text("Read src/lib.rs", true);
    expect(first_row.clone())
        .with_timeout(UI_TIMEOUT)
        .to_be_hidden()
        .await
        .context("a collapsed group shows no rows")?;

    head.click(None).await.context("open the group")?;
    expect(head.clone())
        .with_timeout(UI_TIMEOUT)
        .to_have_attribute("aria-expanded", "true")
        .await?;
    expect(first_row.clone())
        .with_timeout(UI_TIMEOUT)
        .to_be_visible()
        .await
        .context("an open group shows its rows")?;
    // An opened row shows its input; a closed one hides it.
    // `.first()`: every row has its input in the page, only an opened one shows it; the opened
    // row is the first.
    let input = transcript.get_by_text("Input", true).first();
    first_row
        .click(None)
        .await
        .context("open the first nested row")?;
    expect(input.clone())
        .with_timeout(UI_TIMEOUT)
        .to_be_visible()
        .await
        .context("an opened row shows its input")?;

    // A third call arrives while both are open.
    db::append_tool_call(&world.pool, &key, 3, "streak-3", "Read src/db.rs").await?;
    expect(head.clone())
        .with_timeout(UI_TIMEOUT)
        .to_contain_text("3 tool calls")
        .await
        .context("the count of the grown group")?;
    expect(transcript.get_by_text("Read src/", false))
        .with_timeout(UI_TIMEOUT)
        .to_have_count(3)
        .await
        .context("the new call is the last nested row")?;
    expect(head)
        .with_timeout(UI_TIMEOUT)
        .to_have_attribute("aria-expanded", "true")
        .await
        .context("the group stays open")?;
    expect(input)
        .with_timeout(UI_TIMEOUT)
        .to_be_visible()
        .await
        .context("the nested row stays open")?;
    Ok(())
}
