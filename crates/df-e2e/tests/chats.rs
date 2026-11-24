//! A chat that delegates (`iter3-chats`, `just e2e-local`): a board member starts a chat, the persona
//! sets the topic and creates two tasks, its sandbox cannot push, the chat is archived while the
//! implementation runs, the implementation completes, and unarchiving the chat starts the run that
//! delivers the child's message. The tasks are found through the chat the test created, by the
//! marker of the prompt they were created with, never by a predicted key. Needs the environment of
//! `E2eEnv`, so the test is ignored by default.

use anyhow::Result;
use df_core::TaskStatus;
use df_e2e::fixture;
use df_e2e::fixture::World;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db};

/// `create_issue` raised a request: the issue exists only after a member approved it, exactly as
/// shown, and the chat is told its number.
async fn approve_the_requested_issue(world: &World, chat: &str) -> Result<()> {
    let request = eventually("the create-issue request of the chat", UI_TIMEOUT, || {
        db::create_issue_request(&world.pool, chat)
    })
    .await?;
    world
        .session
        .approve_request(
            &world.env,
            &request.to_string(),
            data::CHAT_ISSUE_TITLE,
            None,
        )
        .await?;
    let number = eventually("the created issue to be recorded", UI_TIMEOUT, || {
        db::created_issue_number(&world.pool, request)
    })
    .await?;
    let (title, body) = world.forgejo.issue_text(number).await?;
    assert_eq!(title, data::CHAT_ISSUE_TITLE);
    assert_eq!(body, data::CHAT_ISSUE_BODY);
    let told =
        db::system_messages(&world.pool, chat, df_core::system_reason::ISSUE_CREATED).await?;
    assert!(
        told.iter()
            .any(|m| m.contains(&format!("Issue #{number} created"))),
        "{told:?}"
    );
    // The approval was given in the inbox; the rest of the test works on the chat's page.
    world.session.open_chat(&world.env, chat).await?;
    Ok(())
}

/// Demo step "chats" of `docs/demo.md`; the script is `crates/fake-acp-agent/scripts/chat.yaml`.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_chat_delegates_is_archived_while_a_task_runs_and_wakes_when_unarchived() -> Result<()> {
    fixture::run(
        "a_chat_delegates_is_archived_while_a_task_runs_and_wakes_when_unarchived",
        |world| async move {
            let chat = world
                .session
                .create_chat(
                    &world.env,
                    data::CHAT_PROJECT_KEY,
                    "[chat] Plan the Helm chart release.",
                )
                .await?;

            // The persona sets the topic and creates the two tasks; the checkout is read-only.
            world
                .session
                .expect_text(data::CHAT_TOPIC, RUN_TIMEOUT)
                .await?;
            let implementation = world.child_of(&chat, data::CHAT_IMPLEMENTATION).await?;
            let review = world.child_of(&chat, data::CHAT_REVIEW).await?;
            world
                .session
                .expect_region_text("#transcript", "df:read-only", RUN_TIMEOUT)
                .await?;
            world.wait_for_status(&chat, TaskStatus::Waiting).await?;

            // `create_task` linked the review to issue 1 of the repository.
            let seeded = i64::try_from(data::FORGE_ISSUE_NUMBER)?;
            assert_eq!(db::linked_issue(&world.pool, &review).await?, Some(seeded));
            assert_eq!(db::linked_issue(&world.pool, &implementation).await?, None);
            // `list_issues` answered with a page (the newest 30 of a shared repository, so the
            // seeded issue may be beyond it) and its paging fields.
            assert!(
                db::transcript_events_containing(&world.pool, &chat, "next_cursor: ").await? > 0,
                "the list_issues result in the transcript"
            );

            approve_the_requested_issue(&world, &chat).await?;

            // The implementation runs until it is interrupted; archiving the chat leaves it alone.
            world
                .wait_for_status(&implementation, TaskStatus::Running)
                .await?;
            world.session.archive_chat().await?;
            assert_eq!(
                db::task_status(&world.pool, &implementation).await?,
                TaskStatus::Running
            );
            let runs_before = db::runs(&world.pool, &chat).await?.len();

            world.session.open_task(&world.env, &implementation).await?;
            world.session.interrupt_turn().await?;
            world
                .session
                .send_message(&format!("{} Finish the chart.", data::CHAT_FINISH))
                .await?;
            world.wait_for_runs(&implementation, 1).await?;

            // The message of the completed child is queued on the archived chat, which starts no run.
            eventually(
                "the child-ended message on the archived chat",
                UI_TIMEOUT,
                || async {
                    let messages = db::system_messages(
                        &world.pool,
                        &chat,
                        df_core::system_reason::CHILD_ENDED,
                    )
                    .await?;
                    Ok((!messages.is_empty()).then_some(()))
                },
            )
            .await?;
            assert_eq!(
                db::runs(&world.pool, &chat).await?.len(),
                runs_before,
                "an archived chat starts no run"
            );

            // Unarchiving starts the run that delivers it.
            world.session.open_chat(&world.env, &chat).await?;
            world.session.unarchive_chat().await?;
            world
                .session
                .expect_region_text("#transcript", data::CHAT_IMPLEMENTATION_DONE, RUN_TIMEOUT)
                .await?;
            assert!(db::runs(&world.pool, &chat).await?.len() > runs_before);
            assert_eq!(
                db::children(&world.pool, &chat).await?,
                [implementation, review],
                "the chat created exactly these two, in this order"
            );
            Ok(())
        },
    )
    .await
}
