//! Questions in chats (`question-kinds`, `just e2e-local`): the chat agent asks a blocking
//! single choice, the chat needs you, the board member answers on the chat timeline and the next
//! turn receives the JSON block; archiving a chat closes its pending question. The script is
//! `crates/fake-acp-agent/scripts/chat.yaml` (marker `[chat-question]`).

use std::sync::Arc;

use anyhow::Result;
use df_core::{QuestionAnswer, TaskStatus};
use df_db::requests::RequestState;
use df_e2e::browser::{expect_text_in, option_in, press_answer};
use df_e2e::data;
use df_e2e::db;
use df_e2e::fixture::{self, World};
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT};
use playwright_rs::expect;
use playwright_rs::protocol::AriaRole;

const LAYOUT_QUESTION: &str = "Which chart layout do you prefer?";

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_chat_question_marks_the_chat_is_answered_on_its_timeline_and_reaches_the_next_turn()
-> Result<()> {
    fixture::run(
        "a_chat_question_marks_the_chat_is_answered_on_its_timeline_and_reaches_the_next_turn",
        |world: Arc<World>| async move {
            let chat = world
                .session
                .create_chat(
                    &world.env,
                    data::CHAT_PROJECT_KEY,
                    "[chat-question] Ask me about the layout.",
                )
                .await?;
            let asked = db::question(&world.pool, &chat, LAYOUT_QUESTION).await?;
            // A blocking question ends the turn: the chat waits for the answer, `blocked`.
            world.wait_for_status(&chat, TaskStatus::Blocked).await?;

            // The chat list marks the chat as needing you.
            world
                .session
                .page
                .goto(
                    &format!(
                        "{}/chats?project={}",
                        world.env.base_url,
                        data::CHAT_PROJECT_KEY
                    ),
                    None,
                )
                .await?;
            expect(
                world
                    .session
                    .page
                    .locator(format!("#row-{chat} [aria-label='Needs you']")),
            )
            .with_timeout(UI_TIMEOUT)
            .to_be_visible()
            .await?;

            // The card is on the chat timeline; the answer is given there.
            world.session.open_chat(&world.env, &chat).await?;
            let card = world.session.request_card(asked.id);
            expect_text_in(&card, "Question · Single choice", UI_TIMEOUT).await?;
            option_in(&card, AriaRole::Radio, "Split charts")
                .click(None)
                .await?;
            press_answer(&card).await?;
            expect_text_in(&card, "Answered by", UI_TIMEOUT).await?;
            assert_eq!(
                db::answered_question(&world.pool, asked.id).await?.answer(),
                Some(QuestionAnswer::Choice {
                    selected: vec!["split".into()],
                    other: None
                })
            );

            // The next turn received the JSON block.
            world
                .session
                .expect_region_text("#transcript", "The chat got the split layout.", RUN_TIMEOUT)
                .await?;
            Ok(())
        },
    )
    .await
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn archiving_a_chat_closes_its_pending_question() -> Result<()> {
    fixture::run(
        "archiving_a_chat_closes_its_pending_question",
        |world: Arc<World>| async move {
            let chat = world
                .session
                .create_chat(
                    &world.env,
                    data::CHAT_PROJECT_KEY,
                    "[chat-question] Ask me, I will not answer.",
                )
                .await?;
            let asked = db::question(&world.pool, &chat, LAYOUT_QUESTION).await?;
            world.wait_for_status(&chat, TaskStatus::Blocked).await?;
            world.session.open_inbox_card(&world.env, asked.id).await?;

            world.session.open_chat(&world.env, &chat).await?;
            world.session.archive_chat().await?;

            let row = db::request(&world.pool, asked.id).await?;
            assert_eq!(row.state, RequestState::Closed);
            assert_eq!(row.close_reason.as_deref(), Some("chat-archived"));
            Ok(())
        },
    )
    .await
}
