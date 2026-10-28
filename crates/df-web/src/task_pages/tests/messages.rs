//! Follow-up and queued messages, editing.

use axum::http::StatusCode;
use df_core::Principal;

use crate::task_pages::{details_region, head_region, pending_region, runs_region};
use crate::testkit::{TestApp, form, header_str, insert_run, new_task, seed, tag_around, text};

#[tokio::test]
async fn follow_up_message_is_pending_and_requeues_the_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await;
    // The initial assignment was delivered; the agent is waiting.
    let initial = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    let run = insert_run(&app.pool, task.id, 1, "succeeded").await;
    df_db::mark_prompt_delivered(&app.pool, initial[0].id, df_core::RunId::from_uuid(run))
        .await
        .unwrap();
    df_db::update_task_status(&app.pool, task.id, df_core::TaskStatus::Waiting)
        .await
        .unwrap();

    let response = app
        .post(
            "/tasks/DF-1/messages",
            &form(&[("message", "also add a test for X")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "message-queued");
    let fragment = text(response).await;
    assert!(fragment.contains("also add a test for X") && fragment.contains("1 queued"));
    assert!(
        fragment.contains("aria-label=\"Edit queued message\""),
        "{fragment}"
    );

    let status: String = sqlx::query_scalar("SELECT status FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "queued");
    let queued = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    assert!(
        matches!(&queued[0].author, Principal::Human { display_name, .. } if display_name == "Peter")
    );

    // The page shows it as pending until the transcript delivers it.
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("also add a test for X"));
    df_db::mark_prompt_delivered(&app.pool, queued[0].id, df_core::RunId::from_uuid(run))
        .await
        .unwrap();
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(!page.contains("also add a test for X"));
}

#[tokio::test]
async fn blank_messages_are_refused() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let response = app
        .post(
            "/tasks/DF-1/messages",
            &form(&[("message", "   ")]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Write a message first."));
}

#[tokio::test]
async fn message_form_keeps_the_draft_outside_the_streamed_regions() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert_eq!(html.matches("x-model=\"draft\"").count(), 1);
    // The transcript container follows new items while the reader is at the bottom.
    for needle in [
        "id=\"transcript-scroll\"",
        "x-data=\"{ pinned: true }\"",
        "x-init=",
        // Content growth without a DOM mutation (images) and a shrinking viewport (queued
        // messages) also keep the last item in view.
        "new ResizeObserver(down).observe($el)",
        "new ResizeObserver(down).observe($el.querySelector(",
        "x-on:scroll=",
        "&lt; 48",
    ] {
        assert!(html.contains(needle), "{needle}");
    }

    // The regions the stream replaces never contain the draft state.
    let detail = df_db::task_views::get_task_detail_by_key(&app.pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    for region in [
        head_region(&detail, None),
        details_region(&detail),
        runs_region(&[], None),
        pending_region("DF-1", "Linus", "queued", &[], false, None),
    ] {
        assert!(!region.into_string().contains("draft"));
    }
}

fn human() -> Principal {
    Principal::human(df_core::UserId::new(), "Peter")
}

fn text_content(text: &str) -> Vec<df_proto::transcript::v1::ContentBlock> {
    use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
    vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: text.into() })),
        hidden: false,
    }]
}

/// A task whose assignment was delivered and that has one queued message of a board member.
async fn task_with_queued_message(
    app: &TestApp,
    seed: &crate::testkit::Seed,
    text: &str,
) -> (df_db::TaskRow, df_db::TaskMessageRow) {
    let task = new_task(&app.pool, seed, "t").await;
    let initial = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    let run = insert_run(&app.pool, task.id, 1, "succeeded").await;
    df_db::mark_prompt_delivered(&app.pool, initial[0].id, df_core::RunId::from_uuid(run))
        .await
        .unwrap();
    df_db::update_task_status(&app.pool, task.id, df_core::TaskStatus::Waiting)
        .await
        .unwrap();
    let message = df_db::queue_message(&app.pool, task.id, human(), text_content(text))
        .await
        .unwrap();
    (task, message)
}

#[tokio::test]
async fn queued_stack_sits_above_the_input_and_not_in_the_transcript() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (_, message) = task_with_queued_message(&app, &seed, "also check logout").await;

    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let transcript = &page[page.find("id=\"transcript\"").unwrap()..];
    let transcript_end = transcript
        .find("id=\"pending\"")
        .expect("stack after transcript");
    assert!(!transcript[..transcript_end].contains("also check logout"));
    let stack = &transcript[transcript_end..];
    assert!(stack.find("also check logout").unwrap() < stack.find("id=\"message\"").unwrap());
    assert!(stack.contains(&format!(
        "hx-delete=\"/tasks/DF-1/messages/{}\"",
        message.id
    )));
    // The assignment prompt of the system is not a message of a board member.
    let task = df_db::task_views::get_task_detail_by_key(&app.pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.key, "DF-1");
    assert_eq!(stack.matches("data-queued-id").count(), 1);
}

#[tokio::test]
async fn message_input_can_switch_to_editing_with_cancel_and_save() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    for part in [
        "Editing a queued message",
        "aria-label=\"Send\"",
        "aria-label=\"Save\"",
        "hx-patch=\"/tasks/DF-1/messages/_\"",
        ":hx-patch=\"'/tasks/DF-1/messages/' + editing\"",
        "x-on:click=\"editing = null; draft = saved\"",
        "x-on:message-saved.window=\"editing = null; draft = ''\"",
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
    // Stack and input share one Alpine scope.
    let scope = html
        .find("x-data=\"{ draft: '', saved: '', editing: null }\"")
        .unwrap();
    assert!(
        scope < html.find("id=\"pending\"").unwrap()
            && html.find("id=\"pending\"").unwrap() < html.find("id=\"message\"").unwrap()
    );
}

#[tokio::test]
async fn editing_a_queued_message_replaces_its_text_and_answers_with_the_stack() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;

    let response = app
        .patch(
            &format!("/tasks/DF-1/messages/{}", message.id),
            &form(&[("message", "also check logout")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "message-saved");
    let stack = text(response).await;
    assert!(
        stack.contains("id=\"pending\"")
            && stack.contains("also check logout")
            && !stack.contains(">first<")
    );
    let stored = df_db::list_undelivered_messages(&app.pool, task.id)
        .await
        .unwrap();
    assert_eq!(stored[0].id, message.id);
}

#[tokio::test]
async fn editing_a_forwarded_message_is_a_conflict_that_keeps_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;
    df_db::claim_held_back_messages(&app.pool, task.id)
        .await
        .unwrap();

    let uri = format!("/tasks/DF-1/messages/{}", message.id);
    let edit = app
        .patch(&uri, &form(&[("message", "late")]), &cookie)
        .await;
    assert_eq!(edit.status(), StatusCode::CONFLICT);
    let stack = text(edit).await;
    assert!(
        stack.contains("The agent has already received this message."),
        "{stack}"
    );
    assert!(stack.contains("first") && stack.contains("waiting for delivery"));
    assert!(!stack.contains("sent to the agent"));
    assert!(!stack.contains("Edit queued message"));
    let remove = app.delete(&uri, &cookie).await;
    assert_eq!(remove.status(), StatusCode::CONFLICT);
    assert_eq!(
        df_db::list_undelivered_messages(&app.pool, task.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn empty_edit_is_unprocessable_and_removing_a_queued_message_empties_the_stack() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, message) = task_with_queued_message(&app, &seed, "first").await;
    let uri = format!("/tasks/DF-1/messages/{}", message.id);

    let empty = app.patch(&uri, &form(&[("message", "  ")]), &cookie).await;
    assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(empty).await.contains("Write a message first."));

    let removed = app.delete(&uri, &cookie).await;
    assert_eq!(removed.status(), StatusCode::OK);
    let stack = text(removed).await;
    assert!(stack.contains("id=\"pending\"") && !stack.contains("first"));
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "waiting");
    assert_eq!(
        app.delete("/tasks/DF-1/messages/not-an-id", &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&format!("/tasks/DF-9/messages/{}", message.id), &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_send_button_sits_inside_the_message_box() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let html = text(app.get("/tasks/DF-1", &cookie).await).await;
    let start = html.find("id=\"message-box\"").unwrap();
    let after = &html[start..];
    let mut depth = 0;
    let mut end = 0;
    for (i, _) in after.match_indices(['<']) {
        if after[i..].starts_with("<div") {
            depth += 1;
        }
        if after[i..].starts_with("</div") {
            depth -= 1;
            if depth == 0 {
                end = i;
                break;
            }
        }
    }
    let message_box = &after[..end];
    assert!(
        message_box.contains("<textarea") && message_box.contains("aria-label=\"Send\""),
        "{message_box}"
    );
    assert!(message_box.contains("rounded-full") && message_box.contains("aria-label=\"Save\""));
    assert!(
        message_box.find("<textarea").unwrap() < message_box.find("aria-label=\"Send\"").unwrap()
    );
}

#[tokio::test]
async fn forwarded_messages_wait_for_delivery_and_say_so_on_an_ended_task() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (task, _) = task_with_queued_message(&app, &seed, "first").await;
    df_db::claim_held_back_messages(&app.pool, task.id)
        .await
        .unwrap();

    let waiting = text(app.get("/tasks/DF-1", &cookie).await).await;
    let stack = &waiting[waiting.find("id=\"pending\"").unwrap()..];
    assert!(stack.contains("waiting for delivery"), "{stack}");
    assert!(!stack.contains("not delivered") && !stack.contains("sent to the agent"));

    for status in ["failed", "completed"] {
        sqlx::query("UPDATE tasks SET status = $1")
            .bind(status)
            .execute(&app.pool)
            .await
            .unwrap();
        let ended = text(app.get("/tasks/DF-1", &cookie).await).await;
        let stack = &ended[ended.find("id=\"pending\"").unwrap()..];
        assert!(
            stack.contains("not delivered, sent with the next run"),
            "{status}: {stack}"
        );
        assert!(!stack.contains("waiting for delivery"));
    }
}

#[tokio::test]
async fn both_message_boxes_signal_focus_neutrally() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    new_task(&app.pool, &seed, "t").await;
    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    let compose = text(app.get("/tasks/compose", &cookie).await).await;
    for (what, html, box_marker, textarea_marker) in [
        ("message box", &page, "id=\"message-box\"", "id=\"message\""),
        (
            "composer",
            &compose,
            "rounded-2xl border border-line bg-panel",
            "id=\"description\"",
        ),
    ] {
        let frame = tag_around(html, box_marker);
        let textarea = tag_around(html, textarea_marker);
        // Rest: line; focus inside: the stronger neutral line; never an accent.
        assert!(
            frame.contains("border-line ") && frame.contains("focus-within:border-line-strong"),
            "{what}: {frame}"
        );
        assert!(
            !frame.contains("border-line-strong bg-panel"),
            "{what} is not strong at rest: {frame}"
        );
        assert!(
            !frame.contains("accent") && !textarea.contains("accent"),
            "{what}: {frame} {textarea}"
        );
        assert!(
            textarea.contains("focus-visible:outline-none"),
            "{what}: {textarea}"
        );
    }
}
