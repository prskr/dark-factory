//! The Markdown field of the System prompt tab (`markdown-editor`, `just e2e-local`): the split
//! layout with its live preview, the views, the failures of the preview, the highlighting editor
//! and the field without its scripts. Needs the environment of `E2eEnv`, so the tests are ignored
//! by default.

use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, ensure};
use df_e2e::browser::{Session, assert_no_sideways_scroll};
use df_e2e::fixture::{self, World};
use df_e2e::wait::UI_TIMEOUT;
use playwright_rs::protocol::{
    AriaRole, ColorScheme, EmulateMediaOptions, FulfillOptions, GetByRoleOptions, Locator, Page,
};
use playwright_rs::{Viewport, expect};

/// The preview panel of the System prompt field.
const PREVIEW: &str = "#system_prompt-preview";
/// The body of the error response the tests make the preview endpoint answer with.
const ERROR_BODY: &str = "BODY-OF-THE-ERROR";
/// The text field of the System prompt (the textarea; the highlighting editor hides it).
const TEXT_FIELD: &str = "#system_prompt";

/// How `POST /markdown/preview` answers while a test breaks it, see [`break_preview`].
#[derive(Debug, Clone, Copy)]
enum Answer {
    /// The server answers.
    Pass,
    /// The request fails like a network error.
    Abort,
    /// The response has this HTTP status and an error body.
    Status(u16),
}

/// What `break_preview` makes the endpoint do; a test changes it between steps.
type Mode = Arc<Mutex<Answer>>;

fn set_answer(mode: &Mode, answer: Answer) {
    *mode.lock().unwrap_or_else(PoisonError::into_inner) = answer;
}

/// Scrolls the preview panel to its end; returns its `scrollTop`.
async fn scroll_preview_to_the_end(page: &Page) -> Result<f64> {
    let top = page
        .evaluate_value(&format!(
            "(() => {{ const panel = document.querySelector('{PREVIEW}'); panel.scrollTop = panel.scrollHeight; return String(panel.scrollTop); }})()"
        ))
        .await?;
    Ok(top.parse()?)
}

/// How far the line of the preview that contains `text` is below the top of the panel's visible
/// box: what the board member sees, whatever the scroll position is.
async fn offset_in_preview(page: &Page, text: &str) -> Result<f64> {
    let text = serde_json::to_string(text)?;
    let offset = page
        .evaluate_value(&format!(
            "(() => {{ const panel = document.querySelector('{PREVIEW}'); const line = [...panel.querySelectorAll('p')].find((p) => p.textContent.includes({text})); if (!line) return 'missing'; return String(line.getBoundingClientRect().top - panel.getBoundingClientRect().top); }})()"
        ))
        .await?;
    ensure!(
        offset != "missing",
        "the line `{text}` is not in the preview"
    );
    Ok(offset.parse()?)
}

/// The height of the status block above the preview.
async fn status_block_height(page: &Page) -> Result<f64> {
    let height = page
        .evaluate_value(
            "String(document.querySelector('[data-preview-status]').getBoundingClientRect().height)",
        )
        .await?;
    Ok(height.parse()?)
}

/// Whether the shown line `text` of the preview panel lies inside the panel's visible box.
async fn in_view_of_the_preview(page: &Page, text: &str) -> Result<bool> {
    let text = serde_json::to_string(text)?;
    let inside = page
        .evaluate_value(&format!(
            "(() => {{ const panel = document.querySelector('{PREVIEW}'); const line = [...panel.querySelectorAll('p')].find((p) => p.offsetParent !== null && p.textContent.includes({text})); if (!line) return 'missing'; const a = line.getBoundingClientRect(); const b = panel.getBoundingClientRect(); return String(a.top >= b.top - 0.5 && a.bottom <= b.bottom + 0.5); }})()"
        ))
        .await?;
    ensure!(inside != "missing", "the line `{text}` is not shown");
    Ok(inside == "true")
}

/// Whether the focus is inside the highlighting editor.
async fn focus_in_editor(page: &Page) -> Result<bool> {
    let inside = page
        .evaluate_value("document.activeElement.closest('.cm-editor') !== null")
        .await?;
    Ok(inside == "true")
}

/// The segment `name` of the view switch of the field.
fn view(page: &Page, name: &str) -> Locator {
    page.get_by_role(
        AriaRole::Radio,
        Some(GetByRoleOptions::default().name(name).exact(true)),
    )
}

/// The switch "Highlighting".
fn highlighting(page: &Page) -> Locator {
    page.get_by_role(
        AriaRole::Switch,
        Some(GetByRoleOptions::default().name("Highlighting").exact(true)),
    )
}

/// The width of the element `selector` in pixels.
async fn width_of(page: &Page, selector: &str) -> Result<f64> {
    let width = page
        .locator(selector)
        .bounding_box()
        .await?
        .with_context(|| format!("{selector} is shown"))?
        .width;
    Ok(width)
}

/// A persona of its own (`prefix` made unique), opened on the System prompt tab at `width` px.
/// Returns its name.
async fn open_prompt_tab(world: &World, prefix: &str, width: u32) -> Result<String> {
    let name = world.unique(prefix);
    world
        .session
        .page
        .set_viewport_size(Viewport { width, height: 900 })
        .await?;
    world
        .session
        .create_persona(&world.env, &name, &name, &format!("{name}@example.org"))
        .await?;
    Ok(name)
}

/// Turns "Highlighting" off, so that a test of the text field types into the textarea: the build
/// of the suite has the editor bundle, which takes the textarea's place from 768 px.
async fn use_the_plain_text_field(page: &Page) -> Result<()> {
    let switch = highlighting(page);
    expect(switch.clone())
        .to_be_visible()
        .await
        .context("the switch is offered where the editor can attach")?;
    switch.click(None).await?;
    expect(switch)
        .to_have_attribute("aria-checked", "false")
        .await?;
    expect(page.locator(".cm-editor")).to_have_count(0).await?;
    expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
    Ok(())
}

/// Lets `POST /markdown/preview` of `page` pass through, abort or answer with a status, as the
/// returned [`Mode`] says.
async fn break_preview(page: &Page) -> Result<Mode> {
    let mode = Arc::new(Mutex::new(Answer::Pass));
    let seen = Arc::clone(&mode);
    page.route("**/markdown/preview", move |route| {
        let mode = Arc::clone(&seen);
        async move {
            let answer = *mode.lock().unwrap_or_else(PoisonError::into_inner);
            match answer {
                Answer::Pass => route.continue_(None).await,
                Answer::Abort => route.abort(None).await,
                Answer::Status(status) => {
                    route
                        .fulfill(
                            FulfillOptions::builder()
                                .status(status)
                                .body_string(ERROR_BODY)
                                .build(),
                        )
                        .await
                }
            }
        }
    })
    .await?;
    Ok(mode)
}

/// Answers every request for a script of the page with `404`.
async fn refuse_script(page: &Page, pattern: &str) -> Result<()> {
    page.route(pattern, |route| async move {
        route
            .fulfill(FulfillOptions::builder().status(404).build())
            .await
    })
    .await?;
    Ok(())
}

/// Scenario "Field script not running": the script of the field does not load while the rest of
/// the page works.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn without_the_field_script_the_system_prompt_is_a_plain_text_field() -> Result<()> {
    fixture::run(
        "without_the_field_script_the_system_prompt_is_a_plain_text_field",
        |world| async move {
            let page = &world.session.page;
            refuse_script(page, "**/assets/markdownField.js*").await?;
            open_prompt_tab(&world, "md-noscript", 1440).await?;

            // A labelled text field, no view switch, no editor: Alpine cannot evaluate what the
            // missing component defines, and the field and its switch must not follow that.
            expect(world.session.system_prompt_editor())
                .to_be_visible()
                .await?;
            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            expect(view(page, "Preview")).to_be_hidden().await?;
            expect(view(page, "Split")).to_be_hidden().await?;
            expect(highlighting(page)).to_be_hidden().await?;

            page.locator(TEXT_FIELD)
                .fill("Typed without the script", None)
                .await?;
            world.session.save_persona().await?;
            page.reload(None).await?;
            expect(page.locator(TEXT_FIELD))
                .to_have_value("Typed without the script")
                .await
                .context("the saved text equals the typed text")?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "System prompt at 1440 px" and "First preview in the split layout".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn split_shows_the_editor_and_the_rendered_preview_in_two_equal_columns() -> Result<()> {
    fixture::run(
        "split_shows_the_editor_and_the_rendered_preview_in_two_equal_columns",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-split", 1440).await?;
            use_the_plain_text_field(page).await?;

            expect(view(page, "Split"))
                .to_have_attribute("aria-checked", "true")
                .await?;
            expect(page.locator(PREVIEW))
                .to_contain_text("Nothing to preview")
                .await?;
            let editor = width_of(page, TEXT_FIELD).await?;
            let preview = width_of(page, PREVIEW).await?;
            ensure!(
                editor < 700.0 && preview < 700.0 && (editor - preview).abs() < 40.0,
                "Split shows two equal columns, got {editor} and {preview}"
            );

            // After saving and a reload the page holds the rendered preview at once.
            page.locator(TEXT_FIELD)
                .fill("## Rules\n\n- be kind", None)
                .await?;
            world.session.save_persona().await?;
            page.reload(None).await?;
            expect(page.locator(format!("{PREVIEW} h2")))
                .to_contain_text("Rules")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "Typing updates the preview" and "Heading and fence highlighted" (preview side).
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_split_preview_follows_the_typed_text_and_highlights_a_fence() -> Result<()> {
    fixture::run(
        "the_split_preview_follows_the_typed_text_and_highlights_a_fence",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-live", 1440).await?;
            use_the_plain_text_field(page).await?;

            page.locator(TEXT_FIELD)
                .fill("## Rules\n\n```rust\nfn main() {}\n```", None)
                .await?;

            expect(page.locator(format!("{PREVIEW} h2")))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("Rules")
                .await?;
            expect(page.locator(format!("{PREVIEW} [class*='hl-']")).first())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await
                .context("the fence is highlighted in the preview")
        },
    )
    .await
}

/// Scenario "Diagram kept while typing elsewhere".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_drawn_diagram_keeps_its_frame_while_a_paragraph_below_it_changes() -> Result<()> {
    fixture::run(
        "a_drawn_diagram_keeps_its_frame_while_a_paragraph_below_it_changes",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-diagram", 1440).await?;
            use_the_plain_text_field(page).await?;
            let fence = "```mermaid\nflowchart LR; A-->B\n```";
            page.locator(TEXT_FIELD)
                .fill(&format!("first\n\n{fence}\n\nsecond"), None)
                .await?;
            let frame = format!("{PREVIEW} iframe");
            expect(page.locator(frame.clone()))
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await
                .context("the diagram is drawn")?;
            page.evaluate_value(&format!(
                "window.__diagram = document.querySelector('{frame}'); 'marked'"
            ))
            .await?;

            page.locator(TEXT_FIELD)
                .fill(&format!("first\n\n{fence}\n\nsecond, changed"), None)
                .await?;

            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("second, changed")
                .await?;
            let same = page
                .evaluate_value(&format!(
                    "document.querySelector('{frame}') === window.__diagram"
                ))
                .await?;
            ensure!(same == "true", "the diagram frame was replaced ({same})");
            Ok(())
        },
    )
    .await
}

/// Scenarios "Full width on a wide screen" and "View not remembered".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn editor_and_preview_views_show_one_panel_at_the_full_width_and_are_not_remembered()
-> Result<()> {
    fixture::run(
        "editor_and_preview_views_show_one_panel_at_the_full_width_and_are_not_remembered",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-views", 1440).await?;
            use_the_plain_text_field(page).await?;
            page.locator(TEXT_FIELD).fill("some text", None).await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("some text")
                .await?;

            view(page, "Editor").click(None).await?;
            expect(page.locator(PREVIEW)).to_be_hidden().await?;
            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            ensure!(width_of(page, TEXT_FIELD).await? > 800.0);
            view(page, "Preview").click(None).await?;
            expect(page.locator(TEXT_FIELD)).to_be_hidden().await?;
            expect(page.locator(PREVIEW)).to_be_visible().await?;
            expect(page.locator(PREVIEW))
                .to_contain_text("some text")
                .await?;
            ensure!(width_of(page, PREVIEW).await? > 800.0);

            // The view is not remembered: after a reload the field opens on "Split" again.
            page.reload(None).await?;
            expect(view(page, "Split"))
                .to_have_attribute("aria-checked", "true")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "Split view height" and "Long prompt in Split": a source whose rendering is much
/// shorter than its text still grows both columns to the visible page height, and each column
/// scrolls on its own.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_long_prompt_grows_the_split_columns_to_the_page_height_and_each_scrolls_alone()
-> Result<()> {
    fixture::run(
        "a_long_prompt_grows_the_split_columns_to_the_page_height_and_each_scrolls_alone",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-long", 1440).await?;
            use_the_plain_text_field(page).await?;

            // 300 lines of one paragraph: tall as text, short as rendered Markdown.
            let text = vec!["word word word word"; 300].join("\n");
            page.locator(TEXT_FIELD).fill(&text, None).await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("word word word word")
                .await?;

            let measured = page
                .evaluate_value(
                    "(() => { const box = (s) => document.querySelector(s).getBoundingClientRect().height; return JSON.stringify({ write: box('#system_prompt-write'), preview: box('#system_prompt-preview'), cap: innerHeight - 192 }); })()",
                )
                .await?;
            let sizes: serde_json::Value = serde_json::from_str(&measured)?;
            let (write, preview, cap) = (
                sizes["write"].as_f64().context("write height")?,
                sizes["preview"].as_f64().context("preview height")?,
                sizes["cap"].as_f64().context("page height")?,
            );
            ensure!(
                (write - preview).abs() <= 1.0,
                "the columns share one height: {write} and {preview}"
            );
            ensure!(
                (write - cap).abs() <= 2.0,
                "the columns grew to the visible page height {cap}, not {write}"
            );

            // Scrolling the text does not scroll the preview.
            let scrolled = page
                .evaluate_value(
                    "(() => { const w = document.querySelector('#system_prompt-write'); w.scrollTop = 400; return JSON.stringify({ write: w.scrollTop, preview: document.querySelector('#system_prompt-preview').scrollTop }); })()",
                )
                .await?;
            let scrolled: serde_json::Value = serde_json::from_str(&scrolled)?;
            ensure!(
                scrolled["write"].as_f64().unwrap_or(0.0) > 0.0
                    && scrolled["preview"].as_f64() == Some(0.0),
                "the columns scroll independently: {scrolled}"
            );
            Ok(())
        },
    )
    .await
}

/// Scenario "Typing updates the preview": "Parts of the preview whose source did not change,
/// including the scroll position, stay as they are". A board member who reads the end of a long
/// preview and edits a paragraph above the visible part must not see the text jump.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn editing_above_the_visible_part_of_the_preview_leaves_the_text_being_read_in_place()
-> Result<()> {
    fixture::run(
        "editing_above_the_visible_part_of_the_preview_leaves_the_text_being_read_in_place",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-anchor", 1440).await?;
            use_the_plain_text_field(page).await?;
            let paragraphs = |first: &str| {
                std::iter::once(first.to_owned())
                    .chain((2..=80).map(|n| format!("paragraph {n} of the long preview")))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };
            page.locator(TEXT_FIELD)
                .fill(&paragraphs("paragraph 1 of the long preview"), None)
                .await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("paragraph 80 of the long preview")
                .await?;
            ensure!(
                scroll_preview_to_the_end(page).await? > 0.0,
                "the preview does not scroll"
            );
            let last = "paragraph 80 of the long preview";
            let before = offset_in_preview(page, last).await?;

            // The first paragraph, far above the visible part, wraps to many more lines.
            let longer = format!(
                "the first paragraph, now longer. {}",
                "more words ".repeat(120)
            );
            page.locator(TEXT_FIELD)
                .fill(&paragraphs(&longer), None)
                .await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("the first paragraph, now longer.")
                .await?;

            let after = offset_in_preview(page, last).await?;
            ensure!(
                (after - before).abs() <= 1.0,
                "the paragraph being read moved from {before} to {after} px below the top"
            );
            Ok(())
        },
    )
    .await
}

/// Scenarios "Server not reachable" and "Error response" in "Split": the last preview stays with
/// the note, an error body never replaces it, and the next change tries again.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_failed_live_preview_keeps_the_last_preview_and_the_next_change_tries_again() -> Result<()>
{
    fixture::run(
        "a_failed_live_preview_keeps_the_last_preview_and_the_next_change_tries_again",
        |world| async move {
            let page = &world.session.page;
            let mode = break_preview(page).await?;
            open_prompt_tab(&world, "md-fail", 1440).await?;
            use_the_plain_text_field(page).await?;
            let note = page.get_by_text("Preview not up to date", true);
            page.locator(TEXT_FIELD)
                .fill("first paragraph", None)
                .await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("first paragraph")
                .await?;

            for (answer, text) in [
                (Answer::Status(500), "answered with 500"),
                (Answer::Abort, "network error"),
            ] {
                set_answer(&mode, answer);
                page.locator(TEXT_FIELD).fill(text, None).await?;
                expect(note.clone())
                    .with_timeout(UI_TIMEOUT)
                    .to_be_visible()
                    .await
                    .with_context(|| format!("the note after {text}"))?;
                expect(page.locator(PREVIEW))
                    .to_contain_text("first paragraph")
                    .await?;
                let shown = page.locator(PREVIEW).inner_text().await?;
                ensure!(
                    !shown.contains(ERROR_BODY),
                    "the error body is shown: {shown}"
                );
                expect(page.locator(TEXT_FIELD)).to_have_value(text).await?;
            }

            set_answer(&mode, Answer::Pass);
            page.locator(TEXT_FIELD)
                .fill("recovered paragraph", None)
                .await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("recovered paragraph")
                .await?;
            expect(note).to_be_hidden().await?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "Text over the limit" and "Session expired while typing" in "Split": the last
/// preview stays with the message, the page does not navigate and the text stays.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_text_over_the_limit_and_an_ended_session_keep_the_last_preview_and_say_why() -> Result<()>
{
    fixture::run(
        "a_text_over_the_limit_and_an_ended_session_keep_the_last_preview_and_say_why",
        |world| async move {
            let page = &world.session.page;
            let mode = break_preview(page).await?;
            open_prompt_tab(&world, "md-limit", 1440).await?;
            use_the_plain_text_field(page).await?;
            // A preview long enough to scroll: a message below it would be out of view.
            let last_good = (1..=80)
                .map(|n| format!("paragraph {n} of the last good preview"))
                .collect::<Vec<_>>()
                .join("\n\n");
            page.locator(TEXT_FIELD).fill(&last_good, None).await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("paragraph 80 of the last good preview")
                .await?;

            // The board member has scrolled the preview down and reads the last paragraph.
            let last = "paragraph 80 of the last good preview";
            ensure!(
                scroll_preview_to_the_end(page).await? > 0.0,
                "the preview does not scroll"
            );
            let before = offset_in_preview(page, last).await?;

            // 300 KiB: the endpoint answers 413; saving would still work.
            let too_long_message = "Text too long for a preview (max 256 KiB)";
            let too_long = "a".repeat(300 * 1024);
            page.locator(TEXT_FIELD).fill(&too_long, None).await?;
            expect(page.get_by_text(too_long_message, true))
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            ensure!(
                in_view_of_the_preview(page, too_long_message).await?,
                "the 413 message is outside the visible part of the preview"
            );
            // The last preview stayed where it was read: the paragraph did not move on screen by
            // more than the status line that appeared above it.
            let after = offset_in_preview(page, last).await?;
            let status = status_block_height(page).await?;
            ensure!(
                (after - before).abs() <= status + 1.0,
                "the paragraph moved from {before} to {after} px below the top (status {status} px)"
            );

            // The session ends: the next request is answered 401. The page stays where it is.
            let session_message =
                "Preview not available: your session has ended. Copy your text, then reload.";
            let url = page.url();
            set_answer(&mode, Answer::Status(401));
            let before = offset_in_preview(page, last).await?;
            page.locator(TEXT_FIELD)
                .fill("typed after the session ended", None)
                .await?;
            expect(page.get_by_text(session_message, true))
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            ensure!(
                in_view_of_the_preview(page, session_message).await?,
                "the session message is outside the visible part of the preview"
            );
            let after = offset_in_preview(page, last).await?;
            let status = status_block_height(page).await?;
            ensure!(
                (after - before).abs() <= status + 1.0,
                "the paragraph moved from {before} to {after} px below the top (status {status} px)"
            );
            ensure!(page.url() == url, "the page navigated to {}", page.url());
            expect(page.locator(TEXT_FIELD))
                .to_have_value("typed after the session ended")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenario "Preview request fails" in "Preview": "Preview not available" with "Retry", and the
/// text stays.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_preview_view_says_not_available_with_retry_and_renders_after_it() -> Result<()> {
    fixture::run(
        "the_preview_view_says_not_available_with_retry_and_renders_after_it",
        |world| async move {
            let page = &world.session.page;
            let mode = break_preview(page).await?;
            open_prompt_tab(&world, "md-retry", 1024).await?;
            use_the_plain_text_field(page).await?;
            page.locator(TEXT_FIELD).fill("retry me", None).await?;
            set_answer(&mode, Answer::Abort);

            view(page, "Preview").click(None).await?;

            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("Preview not available")
                .await?;
            set_answer(&mode, Answer::Pass);
            page.get_by_role(
                AriaRole::Button,
                Some(GetByRoleOptions::default().name("Retry").exact(true)),
            )
            .click(None)
            .await?;
            expect(page.locator(PREVIEW))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("retry me")
                .await?;
            view(page, "Editor").click(None).await?;
            expect(page.locator(TEXT_FIELD))
                .to_have_value("retry me")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenario "System prompt below 1280 px": the switch has "Editor" and "Preview", "Editor" is
/// selected and the text is monospace.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn below_1280_px_the_switch_offers_editor_and_preview_in_monospace() -> Result<()> {
    fixture::run(
        "below_1280_px_the_switch_offers_editor_and_preview_in_monospace",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-narrow", 1024).await?;
            use_the_plain_text_field(page).await?;

            expect(view(page, "Editor"))
                .to_have_attribute("aria-checked", "true")
                .await?;
            expect(view(page, "Preview")).to_be_visible().await?;
            expect(view(page, "Split")).to_be_hidden().await?;
            let mono = page
                .evaluate_value(
                    "getComputedStyle(document.querySelector('#system_prompt')).fontFamily.toLowerCase().includes('mono')",
                )
                .await?;
            ensure!(mono == "true", "the text field is monospace");
            Ok(())
        },
    )
    .await
}

/// Scenario "Keyboard on the switch".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_right_arrow_key_selects_shows_and_focuses_the_next_view() -> Result<()> {
    fixture::run(
        "the_right_arrow_key_selects_shows_and_focuses_the_next_view",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-keys", 1024).await?;
            use_the_plain_text_field(page).await?;

            view(page, "Editor").focus().await?;
            page.keyboard().press("ArrowRight", None).await?;

            expect(view(page, "Preview"))
                .to_have_attribute("aria-checked", "true")
                .await?;
            expect(view(page, "Preview")).to_be_focused().await?;
            expect(page.locator(PREVIEW)).to_be_visible().await?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "Script in the text" and "Preview rendered by the server": opening "Preview" shows
/// the typed text rendered by the server, a script as text.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_preview_view_renders_the_typed_text_on_the_server_and_shows_a_script_as_text()
-> Result<()> {
    fixture::run(
        "the_preview_view_renders_the_typed_text_on_the_server_and_shows_a_script_as_text",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-render", 1024).await?;
            use_the_plain_text_field(page).await?;
            page.locator(TEXT_FIELD)
                .fill("## Rules\n\n- be kind\n\n<script>alert(1)</script>", None)
                .await?;

            view(page, "Preview").click(None).await?;

            expect(page.locator(format!("{PREVIEW} li")))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("be kind")
                .await?;
            expect(page.locator(PREVIEW))
                .to_contain_text("<script>alert(1)</script>")
                .await
                .context("the script is shown as text")
        },
    )
    .await
}

/// Scenario "Window narrowed".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn narrowing_the_window_below_1280_px_turns_split_into_editor_and_keeps_the_text()
-> Result<()> {
    fixture::run(
        "narrowing_the_window_below_1280_px_turns_split_into_editor_and_keeps_the_text",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-resize", 1440).await?;
            use_the_plain_text_field(page).await?;
            page.locator(TEXT_FIELD).fill("kept text", None).await?;
            expect(view(page, "Split"))
                .to_have_attribute("aria-checked", "true")
                .await?;

            page.set_viewport_size(Viewport {
                width: 1024,
                height: 900,
            })
            .await?;

            expect(view(page, "Editor"))
                .to_have_attribute("aria-checked", "true")
                .await
                .context("Split falls back to Editor")?;
            expect(page.locator(TEXT_FIELD))
                .to_have_value("kept text")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenarios "Heading and fence highlighted" (editor side) and "Hint with the editor".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_editor_highlights_a_fence_and_the_hint_names_the_indent_keys() -> Result<()> {
    fixture::run(
        "the_editor_highlights_a_fence_and_the_hint_names_the_indent_keys",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-editor", 1440).await?;

            world
                .session
                .write_system_prompt("# Rules\n\n```rust\nfn main() {}\n```")
                .await?;

            expect(page.locator(TEXT_FIELD)).to_be_hidden().await?;
            expect(
                world
                    .session
                    .system_prompt_editor()
                    .locator("[class*='hl-']")
                    .first(),
            )
            .to_be_visible()
            .await
            .context("the fence is highlighted in the editor")?;
            expect(page.locator(format!("{PREVIEW} [class*='hl-']")).first())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await
                .context("the fence is highlighted in the preview")?;
            expect(page.locator("[data-indent-hint]"))
                .to_contain_text("Tab moves to the next field. Indent with")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Scenario "Heading in the code theme": the heading text carries the classes of the function-name
/// scope and so the function-name color of the configured code theme (`InspiredGitHub` in light,
/// `base16-ocean.dark` in dark), semibold, and the `#` keeps the marker color (`faint`). The
/// editor follows the color scheme without a reload.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn an_editor_heading_takes_the_function_color_of_the_code_theme_and_its_marker_stays_faint()
-> Result<()> {
    fixture::run(
        "an_editor_heading_takes_the_function_color_of_the_code_theme_and_its_marker_stays_faint",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-heading", 1440).await?;
            world.session.write_system_prompt("# Rules").await?;

            for (scheme, title, marker) in [
                // `#795DA3` and `--faint` (`#6b6560`) in light, `#8FA1B3` and `#a8a29e` in dark.
                (ColorScheme::Light, "rgb(121, 93, 163)", "rgb(107, 101, 96)"),
                (ColorScheme::Dark, "rgb(143, 161, 179)", "rgb(168, 162, 158)"),
            ] {
                page.emulate_media(EmulateMediaOptions::builder().color_scheme(scheme).build())
                    .await?;
                let styles = page
                    .evaluate_value(
                        "(() => { const line = document.querySelector('.cm-content .cm-line'); const heading = [...line.querySelectorAll('.hl-function')].find((s) => s.textContent.includes('Rules')); const mark = [...line.querySelectorAll('span')].find((s) => s.textContent.trim() === '#'); if (!heading || !mark) return 'missing'; return JSON.stringify({ classes: heading.className, weight: getComputedStyle(heading).fontWeight, title: getComputedStyle(heading).color, marker: getComputedStyle(mark).color }); })()",
                    )
                    .await?;
                ensure!(styles != "missing", "the heading or its marker has no span");
                let styles: serde_json::Value = serde_json::from_str(&styles)?;
                let classes = styles["classes"].as_str().unwrap_or_default();
                ensure!(
                    ["hl-entity", "hl-name", "hl-function"]
                        .iter()
                        .all(|class| classes.split(' ').any(|c| c == *class)),
                    "the heading lacks the function-name classes: {styles}"
                );
                ensure!(styles["weight"] == "600", "the heading is not semibold: {styles}");
                ensure!(styles["title"] == title, "heading color in {scheme:?}: {styles}");
                ensure!(styles["marker"] == marker, "marker color in {scheme:?}: {styles}");
            }
            Ok(())
        },
    )
    .await
}

/// Scenario "Heading in the code theme" (code part): inline code is a chip on `sunk`, the body of a
/// fence whose language has no highlighter (`mermaid`) is not.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn inline_code_has_the_sunk_background_in_the_editor_but_a_fence_body_does_not() -> Result<()>
{
    fixture::run(
        "inline_code_has_the_sunk_background_in_the_editor_but_a_fence_body_does_not",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-chip", 1440).await?;
            world
                .session
                .write_system_prompt("`inline` text\n\n```mermaid\nflowchart LR\n```")
                .await?;
            page.emulate_media(
                EmulateMediaOptions::builder()
                    .color_scheme(ColorScheme::Light)
                    .build(),
            )
            .await?;

            let backgrounds = page
                .evaluate_value(
                    "(() => { const lines = [...document.querySelectorAll('.cm-content .cm-line')]; const fence = lines.find((l) => l.textContent.includes('flowchart')); const inline = lines.flatMap((l) => [...l.querySelectorAll('span')]).filter((s) => s.children.length === 0 && s.textContent.includes('inline')).pop(); if (!fence || !inline) return JSON.stringify({ missing: { fence: !fence, inline: !inline } }); return JSON.stringify({ inline: getComputedStyle(inline).backgroundColor, fence: [fence, ...fence.querySelectorAll('*')].map((e) => getComputedStyle(e).backgroundColor) }); })()",
                )
                .await?;
            let backgrounds: serde_json::Value = serde_json::from_str(&backgrounds)?;
            ensure!(
                backgrounds.get("missing").is_none(),
                "the editor has no line or span to check: {backgrounds}"
            );
            // `--sunk` in light is #f5f5f4.
            ensure!(
                backgrounds["inline"] == "rgb(245, 245, 244)",
                "inline code is not on sunk: {backgrounds}"
            );
            // The line of the fence body and everything in it has no fill.
            let fence = backgrounds["fence"].as_array().context("fence backgrounds")?;
            ensure!(
                fence.iter().all(|color| color == "rgba(0, 0, 0, 0)"),
                "the body of the fence has a background: {backgrounds}"
            );
            Ok(())
        },
    )
    .await
}

/// Scenarios "Tab leaves the editor" and "Save after editing".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn tab_leaves_the_editor_and_the_saved_text_is_what_was_typed() -> Result<()> {
    fixture::run(
        "tab_leaves_the_editor_and_the_saved_text_is_what_was_typed",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-tab", 1440).await?;
            let typed = "# Rules\n\n- one";
            world.session.write_system_prompt(typed).await?;

            page.keyboard().press("Tab", None).await?;

            ensure!(
                !focus_in_editor(page).await?,
                "Tab kept the focus in the editor"
            );
            world.session.save_persona().await?;
            page.reload(None).await?;
            expect(world.session.system_prompt_editor())
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("- one")
                .await?;
            let stored = page.locator(TEXT_FIELD).input_value(None).await?;
            ensure!(stored == typed, "the stored text differs: {stored:?}");
            Ok(())
        },
    )
    .await
}

/// Scenario "Indent with the keyboard".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn ctrl_and_closing_bracket_indent_a_list_line_and_keep_the_focus() -> Result<()> {
    fixture::run(
        "ctrl_and_closing_bracket_indent_a_list_line_and_keep_the_focus",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-indent", 1440).await?;
            world.session.write_system_prompt("- item").await?;

            page.keyboard().press("Control+]", None).await?;

            expect(page.locator(TEXT_FIELD))
                .to_have_value("  - item")
                .await?;
            ensure!(focus_in_editor(page).await?, "the focus left the editor");
            Ok(())
        },
    )
    .await
}

/// Scenarios "Highlighting off" and "No hint on the plain field".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn highlighting_off_brings_the_text_field_back_for_this_browser() -> Result<()> {
    fixture::run(
        "highlighting_off_brings_the_text_field_back_for_this_browser",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-off", 1440).await?;
            expect(world.session.system_prompt_editor())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            expect(highlighting(page))
                .to_have_attribute("aria-checked", "true")
                .await?;

            highlighting(page).click(None).await?;

            expect(page.locator(".cm-editor")).to_have_count(0).await?;
            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            expect(page.locator("[data-indent-hint]"))
                .to_be_hidden()
                .await?;
            page.reload(None).await?;
            expect(highlighting(page))
                .to_have_attribute("aria-checked", "false")
                .await?;
            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            expect(page.locator(".cm-editor")).to_have_count(0).await?;
            Ok(())
        },
    )
    .await
}

/// Scenario "Bundle fails to load": the field stays a text field with its preview and no switch.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_failed_editor_load_leaves_the_text_field_with_its_preview() -> Result<()> {
    fixture::run(
        "a_failed_editor_load_leaves_the_text_field_with_its_preview",
        |world| async move {
            let page = &world.session.page;
            refuse_script(page, "**/assets/editor.js*").await?;
            open_prompt_tab(&world, "md-nobundle", 1440).await?;

            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            expect(page.locator(".cm-editor")).to_have_count(0).await?;
            expect(highlighting(page)).to_be_hidden().await?;
            page.locator(TEXT_FIELD)
                .fill("## Still previewed", None)
                .await?;
            expect(page.locator(format!("{PREVIEW} h2")))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("Still previewed")
                .await?;
            expect(page.locator(TEXT_FIELD))
                .to_have_value("## Still previewed")
                .await?;
            Ok(())
        },
    )
    .await
}

/// Whether the browser requested the editor bundle since the page loaded.
async fn editor_requested(page: &Page) -> Result<bool> {
    let requested = page
        .evaluate_value(
            "performance.getEntriesByType('resource').some((e) => e.name.includes('/assets/editor.js'))",
        )
        .await?;
    Ok(requested == "true")
}

/// Scenarios "System prompt on a phone" and "Persona page on a phone": a 390 px phone has the tab
/// row, the plain text field with the switch Editor | Preview, no "Highlighting", no editor bundle
/// and no horizontal scroll, and 16 px text.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_phone_has_the_text_field_without_the_editor_and_no_sideways_scroll() -> Result<()> {
    fixture::run(
        "a_phone_has_the_text_field_without_the_editor_and_no_sideways_scroll",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-phone", 390).await?;
            // A fresh load, so that the resources of the page are the phone's.
            page.goto(&page.url(), None).await?;

            for tab in ["General", "System prompt", "Commits"] {
                expect(page.get_by_role(
                    AriaRole::Tab,
                    Some(GetByRoleOptions::default().name(tab).exact(true)),
                ))
                .to_be_visible()
                .await?;
            }
            expect(view(page, "Preview")).to_be_visible().await?;
            expect(page.locator(TEXT_FIELD)).to_be_visible().await?;
            expect(page.locator(TEXT_FIELD))
                .to_have_css("font-size", "16px")
                .await?;
            expect(page.locator(".cm-editor")).to_have_count(0).await?;
            expect(highlighting(page)).to_be_hidden().await?;
            ensure!(
                !editor_requested(page).await?,
                "the phone requested the editor bundle"
            );
            assert_no_sideways_scroll(page, "on the System prompt tab at 390 px").await?;
            page.get_by_role(
                AriaRole::Tab,
                Some(GetByRoleOptions::default().name("General").exact(true)),
            )
            .click(None)
            .await?;
            assert_no_sideways_scroll(page, "on the General tab at 390 px").await
        },
    )
    .await
}

/// The height of the text field in pixels, and its line height.
async fn text_field_height_and_line_height(page: &Page) -> Result<(f64, f64)> {
    let measured = page
        .evaluate_value(&format!(
            "(() => {{ const field = document.querySelector('{TEXT_FIELD}'); return JSON.stringify({{ height: field.getBoundingClientRect().height, line: parseFloat(getComputedStyle(field).lineHeight) }}); }})()"
        ))
        .await?;
    let measured: serde_json::Value = serde_json::from_str(&measured)?;
    let number = |key: &str| {
        measured
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .with_context(|| format!("the {key} of the text field"))
    };
    Ok((number("height")?, number("line")?))
}

/// Scenario "Markdown field without scripts" and "Field script not running": the text field is at
/// least its configured rows tall (20 on the System prompt tab) for a short text, on a phone and
/// in the Split view, not only as tall as a single-line input.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_text_field_is_at_least_its_rows_tall_for_a_short_text_on_a_phone_and_in_split()
-> Result<()> {
    fixture::run(
        "the_text_field_is_at_least_its_rows_tall_for_a_short_text_on_a_phone_and_in_split",
        |world| async move {
            let page = &world.session.page;
            open_prompt_tab(&world, "md-rows", 390).await?;
            page.locator(TEXT_FIELD).fill("short", None).await?;

            let (height, line) = text_field_height_and_line_height(page).await?;
            ensure!(
                height >= 20.0 * line,
                "on a phone the field is {height} px, less than 20 rows of {line} px"
            );

            page.set_viewport_size(Viewport {
                width: 1440,
                height: 900,
            })
            .await?;
            // The view of a field is chosen when the page loads.
            page.reload(None).await?;
            use_the_plain_text_field(page).await?;
            expect(view(page, "Split"))
                .to_have_attribute("aria-checked", "true")
                .await?;
            page.locator(TEXT_FIELD).fill("short", None).await?;
            let (height, line) = text_field_height_and_line_height(page).await?;
            ensure!(
                height >= 20.0 * line,
                "in Split the field is {height} px, less than 20 rows of {line} px"
            );
            Ok(())
        },
    )
    .await
}

/// Scenario "Tablet without keyboard": a 1366 px tablet with only a touch screen gets the editor
/// in the Split view, with 16 px text.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_touch_tablet_gets_the_editor_in_the_split_view_with_16px_text() -> Result<()> {
    fixture::run(
        "a_touch_tablet_gets_the_editor_in_the_split_view_with_16px_text",
        |world| async move {
            open_prompt_tab(&world, "md-tablet", 1440).await?;
            let url = world.session.page.url();
            let tablet = Session::start_logged_in_tablet(&world.env, 1366, 1024).await?;

            let shown = async {
                tablet.page.goto(&url, None).await?;
                expect(tablet.system_prompt_editor())
                    .with_timeout(UI_TIMEOUT)
                    .to_be_visible()
                    .await
                    .context("the tablet gets the editor")?;
                expect(view(&tablet.page, "Split"))
                    .to_have_attribute("aria-checked", "true")
                    .await?;
                expect(tablet.system_prompt_editor())
                    .to_have_css("font-size", "16px")
                    .await?;
                Ok::<(), anyhow::Error>(())
            }
            .await;
            tablet.close_page().await;
            shown
        },
    )
    .await
}
