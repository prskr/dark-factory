//! Question kinds (`question-kinds`, `just e2e-local`): the three kinds answered through the
//! cards, their field errors, the structured answer the agent receives, and the drafts that
//! survive live updates. The fake agent is `network-access.yaml`, told apart by the markers
//! `[kind-single]`, `[kind-choices]` and `[kind-drafts]`; its held fetch keeps the turn open while
//! the board member answers. Every test creates its own task and finds the requests through it.

use std::sync::Arc;

use anyhow::{Context, Result};
use df_core::{QuestionAnswer, QuestionKind, RequestId};
use df_db::requests::RequestState;
use df_e2e::browser::{Session, button_in, expect_text_in, option_in, press_answer};
use df_e2e::fixture::{self, World};
use df_e2e::wait::{RECONNECT_TIMEOUT, RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db};
use playwright_rs::expect;
use playwright_rs::protocol::{AriaRole, GetByRoleOptions, Locator};

const PG_QUESTION: &str = "Which PostgreSQL version should the chart default to?";
const PARTS_QUESTION: &str = "Which parts does the change touch?";
const WHY_QUESTION: &str = "Why did the build fail?";
const ROLLOUT_QUESTION: &str = "Describe the rollout.";
const ENVIRONMENTS_QUESTION: &str = "Which environments get it first?";

/// Approves the held `network-access` request of `key`, which ends the first turn of the script.
async fn release_the_turn(world: &World, key: &str) -> Result<()> {
    let gate = eventually("the held first-turn fetch", RUN_TIMEOUT, || async {
        db::network_request(&world.pool, key, data::NETWORK_HOST, data::NETWORK_PORT).await
    })
    .await?;
    world
        .session
        .approve_request(
            &world.env,
            &gate.id.to_string(),
            &format!(
                "network access to {}:{}",
                data::NETWORK_HOST,
                data::NETWORK_PORT
            ),
            Some("task"),
        )
        .await
}

fn tab(card: &Locator, name: &str) -> Locator {
    card.get_by_role(
        AriaRole::Tab,
        Some(GetByRoleOptions::default().name(name).exact(true)),
    )
}

/// Creates a task whose script asks the questions of `marker`; returns its key.
async fn task_asking(world: &World, marker: &str) -> Result<String> {
    world
        .session
        .create_task(
            &world.env,
            data::NETWORK_PROJECT_KEY,
            &format!("{marker} Ask the questions."),
            None,
        )
        .await
}

/// The smallest height in px of the options and the "Answer" button of the card `id` on `phone`.
async fn smallest_target(phone: &Session, id: RequestId) -> Result<f64> {
    let script = format!(
        "Math.min(...[...document.querySelectorAll('#request-{id} label, #request-{id} form button[type=submit]')].map(e => e.getBoundingClientRect().height))"
    );
    let height = phone
        .evaluate(&format!("measure the targets of {id}"), &script)
        .await?;
    height.trim_matches('"').parse().context("a height")
}

/// Spec "Structured choice answer": the board member picks an option and answers in the inbox;
/// the run receives the message whose JSON block names the option id; the timeline card shows
/// the option marked.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_single_choice_is_answered_in_the_inbox_and_the_agent_gets_the_option_id() -> Result<()> {
    fixture::run(
        "a_single_choice_is_answered_in_the_inbox_and_the_agent_gets_the_option_id",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-single]").await?;
            let asked = db::question(&world.pool, &key, PG_QUESTION).await?;
            assert_eq!(asked.question().kind, QuestionKind::SingleChoice);

            let card = world.session.open_inbox_card(&world.env, asked.id).await?;
            expect_text_in(&card, "Question · Single choice", UI_TIMEOUT).await?;
            expect_text_in(&card, "Supported until 2028", UI_TIMEOUT).await?;
            option_in(&card, AriaRole::Radio, "PostgreSQL 17")
                .click(None)
                .await?;
            press_answer(&card).await?;
            expect_text_in(&card, "Answered by", UI_TIMEOUT).await?;

            assert_eq!(
                db::answered_question(&world.pool, asked.id).await?.answer(),
                Some(QuestionAnswer::Choice {
                    selected: vec!["pg17".into()],
                    other: None
                })
            );
            release_the_turn(&world, &key).await?;
            world.session.open_task(&world.env, &key).await?;
            world
                .session
                .expect_region_text(
                    "#transcript",
                    "Chose pg17 from the JSON block.",
                    RUN_TIMEOUT,
                )
                .await?;
            let timeline = world.session.request_card(asked.id);
            expect_text_in(&timeline, "chosen", UI_TIMEOUT).await?;
            expect_text_in(&timeline, "PostgreSQL 17", UI_TIMEOUT).await?;
            Ok(())
        },
    )
    .await
}

/// Spec "Single choice on a phone" and "Tap on the label": at 390 px a board member taps the
/// label text of an option, then "Answer", and sees the answered card.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn tapping_the_label_of_an_option_and_answer_answers_on_a_phone() -> Result<()> {
    fixture::run(
        "tapping_the_label_of_an_option_and_answer_answers_on_a_phone",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-single]").await?;
            let asked = db::question(&world.pool, &key, PG_QUESTION).await?;

            let phone = Session::start_logged_in_tablet(&world.env, 390, 844).await?;
            let card = phone.open_inbox_card(&world.env, asked.id).await?;
            // The label text, not the radio: the whole row is the click target.
            card.get_by_text("PostgreSQL 17", true).click(None).await?;
            expect(option_in(&card, AriaRole::Radio, "PostgreSQL 17"))
                .with_timeout(UI_TIMEOUT)
                .to_be_checked()
                .await?;
            press_answer(&card).await?;
            expect_text_in(&card, "Answered by", UI_TIMEOUT).await?;

            assert_eq!(
                db::answered_question(&world.pool, asked.id).await?.answer(),
                Some(QuestionAnswer::Choice {
                    selected: vec!["pg17".into()],
                    other: None
                })
            );
            phone.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Choice targets on phones": at 390 px every option, "Other" and "Answer" is at least
/// 44 px high.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn options_and_the_answer_button_are_at_least_44_px_high_on_a_phone() -> Result<()> {
    fixture::run(
        "options_and_the_answer_button_are_at_least_44_px_high_on_a_phone",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-choices]").await?;
            let multi = db::question(&world.pool, &key, PARTS_QUESTION).await?;

            let phone = Session::start_logged_in_tablet(&world.env, 390, 844).await?;
            phone.open_inbox_card(&world.env, multi.id).await?;
            let smallest = smallest_target(&phone, multi.id).await?;

            assert!(smallest >= 44.0, "the smallest target is {smallest} px");
            phone.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Too many checked": an over-limit answer keeps the checked options and shows its error.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn an_answer_over_the_limit_keeps_the_checked_options_and_shows_the_error() -> Result<()> {
    fixture::run(
        "an_answer_over_the_limit_keeps_the_checked_options_and_shows_the_error",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-choices]").await?;
            let multi = db::question(&world.pool, &key, PARTS_QUESTION).await?;
            let card = world.session.open_inbox_card(&world.env, multi.id).await?;
            expect_text_in(&card, "Choose 1 to 2", UI_TIMEOUT).await?;

            for label in ["API", "UI", "CLI"] {
                option_in(&card, AriaRole::Checkbox, label)
                    .click(None)
                    .await?;
            }
            press_answer(&card).await?;

            expect_text_in(&card, "Choose at most 2 options.", UI_TIMEOUT).await?;
            for label in ["API", "UI", "CLI"] {
                expect(option_in(&card, AriaRole::Checkbox, label))
                    .with_timeout(UI_TIMEOUT)
                    .to_be_checked()
                    .await?;
            }
            assert_eq!(
                db::request(&world.pool, multi.id).await?.state,
                RequestState::Pending
            );
            Ok(())
        },
    )
    .await
}

/// Spec "Other chosen" and "Other chosen without text": checking Other shows and focuses its
/// input, Other without text is rejected, and an Other answer is recorded and shown.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn other_shows_and_focuses_its_input_and_needs_a_text() -> Result<()> {
    fixture::run(
        "other_shows_and_focuses_its_input_and_needs_a_text",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-choices]").await?;
            let multi = db::question(&world.pool, &key, PARTS_QUESTION).await?;
            let card = world.session.open_inbox_card(&world.env, multi.id).await?;

            option_in(&card, AriaRole::Checkbox, "Other")
                .click(None)
                .await?;
            let other = card.locator("input[name=other]");
            expect(other.clone())
                .with_timeout(UI_TIMEOUT)
                .to_be_focused()
                .await?;
            press_answer(&card).await?;
            expect_text_in(&card, "Write your Other answer.", UI_TIMEOUT).await?;

            other.fill("also the CLI", None).await?;
            option_in(&card, AriaRole::Checkbox, "API")
                .click(None)
                .await?;
            press_answer(&card).await?;
            expect_text_in(&card, "Other: also the CLI", UI_TIMEOUT).await?;
            expect_text_in(&card, "chosen", UI_TIMEOUT).await?;
            assert_eq!(
                db::answered_question(&world.pool, multi.id).await?.answer(),
                Some(QuestionAnswer::Choice {
                    selected: vec!["api".into()],
                    other: Some("also the CLI".into())
                })
            );
            Ok(())
        },
    )
    .await
}

/// Spec "Text answer preview": the preview shows the Markdown, the editor bundle is not
/// requested, and the answered card shows the rendered answer.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_text_answer_previews_as_markdown_without_the_editor_bundle() -> Result<()> {
    fixture::run(
        "a_text_answer_previews_as_markdown_without_the_editor_bundle",
        |world: Arc<World>| async move {
            let key = task_asking(&world, "[kind-choices]").await?;
            let text = db::question(&world.pool, &key, WHY_QUESTION).await?;
            let card = world.session.open_inbox_card(&world.env, text.id).await?;

            card.locator("textarea[name=text]")
                .fill("The **build** failed.", None)
                .await?;
            tab(&card, "Preview").click(None).await?;
            expect(card.locator("strong"))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("build")
                .await?;
            assert!(
                !world.session.requested_editor_bundle().await?,
                "the plain tabs layout never loads the editor bundle"
            );
            tab(&card, "Write").click(None).await?;
            press_answer(&card).await?;

            expect_text_in(&card, "Answered by", UI_TIMEOUT).await?;
            expect(card.locator("strong"))
                .with_timeout(UI_TIMEOUT)
                .to_contain_text("build")
                .await?;
            assert_eq!(
                db::answered_question(&world.pool, text.id).await?.answer(),
                Some(QuestionAnswer::Text("The **build** failed.".into()))
            );
            Ok(())
        },
    )
    .await
}

/// A drafts task: its text question and the viewer (a second browser of the same board member).
struct Drafts {
    key: String,
    text: RequestId,
    multi: RequestId,
    viewer: Session,
}

async fn drafts(world: &World) -> Result<Drafts> {
    let key = task_asking(world, "[kind-drafts]").await?;
    let text = db::question(&world.pool, &key, ROLLOUT_QUESTION).await?.id;
    let multi = db::question(&world.pool, &key, ENVIRONMENTS_QUESTION)
        .await?
        .id;
    let viewer = Session::start_logged_in(&world.env).await?;
    Ok(Drafts {
        key,
        text,
        multi,
        viewer,
    })
}

/// Raises one more text question on the drafts task, as its agent would, and waits until the
/// viewer's page shows its card; returns its id.
async fn raise_and_show(world: &World, d: &Drafts, text: &str) -> Result<RequestId> {
    let id = db::raise_text_question(&world.pool, &d.key, &world.unique(text)).await?;
    expect(d.viewer.request_card(id))
        .with_timeout(UI_TIMEOUT)
        .to_be_visible()
        .await?;
    Ok(id)
}

/// Spec "Several fields on one page": three pending text questions in the inbox each have their
/// own tabs and ids, and no editor bundle is requested.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn several_text_questions_in_the_inbox_have_their_own_tabs_and_no_editor_bundle() -> Result<()>
{
    fixture::run(
        "several_text_questions_in_the_inbox_have_their_own_tabs_and_no_editor_bundle",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            d.viewer.open_inbox_card(&world.env, d.text).await?;
            let second = raise_and_show(&world, &d, "Second text?").await?;
            let third = raise_and_show(&world, &d, "Third text?").await?;

            for id in [d.text, second, third] {
                let card = d.viewer.request_card(id);
                expect(tab(&card, "Write"))
                    .with_timeout(UI_TIMEOUT)
                    .to_be_visible()
                    .await?;
                expect(tab(&card, "Preview"))
                    .with_timeout(UI_TIMEOUT)
                    .to_be_visible()
                    .await?;
            }
            let duplicates = d
                .viewer
                .evaluate(
                    "count duplicate ids",
                    "(() => { const ids = [...document.querySelectorAll('[id]')].map(e => e.id); return ids.length - new Set(ids).size; })()",
                )
                .await?;
            assert_eq!(duplicates.trim_matches('"'), "0", "duplicate ids on the page");
            assert!(
                !d.viewer.requested_editor_bundle().await?,
                "no field loads the editor bundle"
            );
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "New request while typing in the inbox".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn half_an_answer_in_the_inbox_survives_a_new_request() -> Result<()> {
    fixture::run(
        "half_an_answer_in_the_inbox_survives_a_new_request",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            let card = d.viewer.open_inbox_card(&world.env, d.text).await?;
            let area = card.locator("textarea[name=text]");
            area.fill("half an answ", None).await?;

            raise_and_show(&world, &d, "Another request?").await?;

            expect(area)
                .with_timeout(UI_TIMEOUT)
                .to_have_value("half an answ")
                .await?;
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Rejected answer after a live update": a new request arrived while typing, and the answer
/// the server rejects keeps the typed text.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_rejected_answer_after_a_live_update_shows_the_error_and_keeps_the_text() -> Result<()> {
    fixture::run(
        "a_rejected_answer_after_a_live_update_shows_the_error_and_keeps_the_text",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            let card = d.viewer.open_inbox_card(&world.env, d.text).await?;
            raise_and_show(&world, &d, "Another request?").await?;

            let area = card.locator("textarea[name=text]");
            area.fill(&"x".repeat(4001), None).await?;
            press_answer(&card).await?;

            expect_text_in(&card, "At most 4,000 characters.", UI_TIMEOUT).await?;
            assert_eq!(
                card.locator("textarea[name=text]")
                    .input_value(None)
                    .await?
                    .len(),
                4001
            );
            assert_eq!(
                db::request(&world.pool, d.text).await?.state,
                RequestState::Pending
            );
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "New request while typing on the timeline".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn two_checked_options_on_the_timeline_survive_a_new_request() -> Result<()> {
    fixture::run(
        "two_checked_options_on_the_timeline_survive_a_new_request",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            d.viewer.open_task(&world.env, &d.key).await?;
            let card = d.viewer.request_card(d.multi);
            expect(card.clone())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            option_in(&card, AriaRole::Checkbox, "Dev")
                .click(None)
                .await?;
            option_in(&card, AriaRole::Checkbox, "Stage")
                .click(None)
                .await?;

            raise_and_show(&world, &d, "One more?").await?;
            for label in ["Dev", "Stage"] {
                expect(option_in(&card, AriaRole::Checkbox, label))
                    .with_timeout(UI_TIMEOUT)
                    .to_be_checked()
                    .await?;
            }
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Decided elsewhere while typing": a second browser answers, the first shows the answer.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_question_answered_in_another_browser_shows_the_answer_on_the_timeline() -> Result<()> {
    fixture::run(
        "a_question_answered_in_another_browser_shows_the_answer_on_the_timeline",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            d.viewer.open_task(&world.env, &d.key).await?;
            let timeline = d.viewer.request_card(d.multi);
            expect(timeline.clone())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            option_in(&timeline, AriaRole::Checkbox, "Prod")
                .click(None)
                .await?;

            let inbox = world.session.open_inbox_card(&world.env, d.multi).await?;
            option_in(&inbox, AriaRole::Checkbox, "Dev")
                .click(None)
                .await?;
            press_answer(&inbox).await?;
            expect_text_in(&inbox, "Answered by", UI_TIMEOUT).await?;

            expect_text_in(&timeline, "Answered by", UI_TIMEOUT).await?;
            expect(timeline.locator("input"))
                .with_timeout(UI_TIMEOUT)
                .to_have_count(0)
                .await?;
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Stream reconnects while typing": the live connection is dropped and restored; the text
/// and the open tab are kept, and a request raised meanwhile arrives with the reconnect.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_text_and_the_open_tab_survive_a_dropped_and_restored_connection() -> Result<()> {
    fixture::run(
        "the_text_and_the_open_tab_survive_a_dropped_and_restored_connection",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            d.viewer.open_task(&world.env, &d.key).await?;
            let card = d.viewer.request_card(d.text);
            expect(card.clone())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            card.locator("textarea[name=text]")
                .fill("half an answ", None)
                .await?;
            tab(&card, "Preview").click(None).await?;
            expect(tab(&card, "Preview"))
                .with_timeout(UI_TIMEOUT)
                .to_have_attribute("aria-selected", "true")
                .await?;

            d.viewer.set_offline(true).await?;
            let extra =
                db::raise_text_question(&world.pool, &d.key, &world.unique("While offline?"))
                    .await?;
            d.viewer.set_offline(false).await?;

            // The request raised while offline can only arrive through the reconnect.
            expect(d.viewer.request_card(extra))
                .with_timeout(RECONNECT_TIMEOUT)
                .to_be_visible()
                .await?;
            expect(card.locator("textarea[name=text]"))
                .with_timeout(UI_TIMEOUT)
                .to_have_value("half an answ")
                .await?;
            expect(tab(&card, "Preview"))
                .with_timeout(UI_TIMEOUT)
                .to_have_attribute("aria-selected", "true")
                .await?;
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}

/// Spec "Card changes while typing": the task is renamed while an option is checked.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_checked_option_survives_a_rename_of_the_task() -> Result<()> {
    fixture::run(
        "a_checked_option_survives_a_rename_of_the_task",
        |world: Arc<World>| async move {
            let d = drafts(&world).await?;
            d.viewer.open_task(&world.env, &d.key).await?;
            let card = d.viewer.request_card(d.multi);
            expect(card.clone())
                .with_timeout(UI_TIMEOUT)
                .to_be_visible()
                .await?;
            option_in(&card, AriaRole::Checkbox, "Stage")
                .click(None)
                .await?;

            let title = world.unique("Renamed rollout");
            d.viewer
                .page
                .get_by_role(
                    AriaRole::Button,
                    Some(GetByRoleOptions::default().name("Rename task")),
                )
                .click(None)
                .await?;
            d.viewer.page.locator("#title").fill(&title, None).await?;
            button_in(&d.viewer.page.locator("#task-head"), "Save")
                .click(None)
                .await?;

            expect_text_in(&d.viewer.page.locator("#task-head"), &title, UI_TIMEOUT).await?;
            expect_text_in(&card, &title, UI_TIMEOUT).await?;
            expect(option_in(&card, AriaRole::Checkbox, "Stage"))
                .with_timeout(UI_TIMEOUT)
                .to_be_checked()
                .await?;
            d.viewer.close_page().await;
            Ok(())
        },
    )
    .await
}
