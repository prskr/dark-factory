//! Tests of the chat pages (`specs/chat-ui/spec.md`); fixtures shared by the topic modules.

use df_core::TaskStatus;

use crate::testkit::{Seed, TestApp, new_chat, seed, set_task_status};

mod archive;
mod details;
mod dialogs;
mod head;
mod list;
mod live;
mod pages;
mod phone;
mod scripts;
mod tasks;

/// The seeded project `DF` and an idle chat `DF-1-CHAT` of the seeded persona Linus: its first
/// message was delivered and the run ended.
async fn idle_chat(app: &TestApp) -> (Seed, df_db::TaskRow) {
    let seed = seed(&app.pool).await;
    let chat = new_chat(&app.pool, &seed, "Plan the Helm chart release").await;
    deliver_first_message(app, &chat).await;
    set_task_status(&app.pool, chat.id, TaskStatus::Waiting).await;
    (seed, chat)
}

/// The first message of the chat is delivered by a run that ended.
async fn deliver_first_message(app: &TestApp, chat: &df_db::TaskRow) {
    let queued = df_db::list_undelivered_messages(&app.pool, chat.id)
        .await
        .unwrap();
    let run = crate::testkit::insert_run(&app.pool, chat.id, 1, "succeeded").await;
    for message in queued {
        df_db::mark_prompt_delivered(
            &app.pool,
            chat.id,
            message.id,
            df_core::RunId::from_uuid(run),
        )
        .await
        .unwrap();
    }
}

/// The agent of the chat says `text` in its first run.
async fn agent_says(app: &TestApp, chat: &df_db::TaskRow, text: &str) {
    let run: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1 ORDER BY number LIMIT 1")
            .bind(chat.id.as_uuid())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let seq: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(seq), 0) + 1 FROM transcript_events WHERE run_id = $1",
    )
    .bind(run)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    crate::testkit::add_event(
        &app.pool,
        chat.id,
        run,
        seq,
        "turn_1",
        crate::testkit::chunk("m1", text),
    )
    .await;
}

/// The part of `html` from `marker` to the first `end` after it.
fn between<'a>(html: &'a str, marker: &str, end: &str) -> &'a str {
    let start = html
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} in {html}"));
    let rest = &html[start..];
    &rest[..rest.find(end).map_or(rest.len(), |at| at + end.len())]
}

/// The row of the chat in the list.
fn row<'a>(html: &'a str, key: &str) -> &'a str {
    between(html, &format!("<li id=\"row-{key}\""), "</li>")
}
