use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, header};
use df_core::{TaskId, TaskStatus};
use serde_json::json;

use crate::testkit::{SseReader, TestApp, add_event, chunk, insert_run, new_task, seed};

/// Deadline for updates that a notification triggers; it must stay well below the poll, which
/// would otherwise mask a missing notification arm.
const LIVE: Duration = Duration::from_secs(1);
const _: () = assert!(LIVE.as_millis() * 2 <= super::POLL.as_millis());

struct Fixture {
    app: TestApp,
    cookie: String,
    task: TaskId,
    run: uuid::Uuid,
}

async fn fixture() -> Fixture {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "t").await.id;
    let run = insert_run(&app.pool, task, 1, "running").await;
    Fixture {
        app,
        cookie,
        task,
        run,
    }
}

impl Fixture {
    async fn events(&self, uri: &str) -> SseReader {
        SseReader::new(self.app.get(uri, &self.cookie).await)
    }

    /// Queues a message of a board member.
    async fn queue(&self, text: &str) -> df_core::PromptId {
        let content = vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent { text: text.into() },
            )),
        }];
        let (actor, _) = crate::testkit::actor();
        df_db::queue_message(&self.app.pool, self.task, actor, content)
            .await
            .unwrap()
            .id
    }

    async fn human_messages(&self) -> Vec<df_db::TaskMessageRow> {
        let all = df_db::list_undelivered_messages(&self.app.pool, self.task)
            .await
            .unwrap();
        all.into_iter()
            .filter(|m| matches!(m.author, df_core::Principal::Human { .. }))
            .collect()
    }

    async fn add(&self, seq: i64, turn: &str, body: serde_json::Value) -> i64 {
        add_event(&self.app.pool, self.task, self.run, seq, turn, body).await
    }
}

fn ended() -> serde_json::Value {
    json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}})
}

#[tokio::test]
async fn replay_after_an_id_has_no_gaps_and_no_duplicates() {
    let f = fixture().await;
    let e1 = f
        .add(
            1,
            "t1",
            json!({"userMessage": {"promptId": "p", "content": [{"text": {"text": "go"}}]}}),
        )
        .await;
    let e2 = f.add(2, "t1", chunk("m1", "A")).await;
    f.add(3, "t1", chunk("m1", "B")).await;
    let e4 = f
        .add(
            4,
            "t1",
            json!({"toolCall": {"toolCallId": "c1", "title": "Run tests"}}),
        )
        .await;
    let e5 = f.add(5, "t1", ended()).await;

    let mut sse = f.events(&format!("/tasks/DF-1/events?after={e2}")).await;
    let frame = sse.next().await;
    assert_eq!(frame.id.as_deref(), Some(e5.to_string().as_str()));
    // Events 3 to 5 arrive once. The message that grew is morphed in place, already as Markdown.
    assert!(
        frame
            .data
            .contains(r##"hx-target="#msg-m1" hx-swap="outerMorph""##),
        "{}",
        frame.data
    );
    assert!(frame.data.contains("<p>AB</p>"), "{}", frame.data);
    assert_eq!(frame.data.matches(&format!("id=\"ev-{e4}\"")).count(), 1);
    // The turn end is no item.
    assert_eq!(frame.data.matches(&format!("id=\"ev-{e5}\"")).count(), 0);
    assert!(
        frame
            .data
            .contains(r##"hx-target="#transcript" hx-swap="beforeend""##)
    );
    // Nothing from before the id is sent again.
    assert!(!frame.data.contains(&format!("id=\"ev-{e1}\"")));

    // A new event is delivered next, once.
    let e6 = f.add(6, "t2", chunk("m2", "C")).await;
    let next = sse.next().await;
    assert_eq!(next.id.as_deref(), Some(e6.to_string().as_str()));
    assert_eq!(next.data.matches("id=\"msg-m2\"").count(), 1);
    assert!(
        !next.data.contains("id=\"msg-m1\"") && !next.data.contains(&format!("id=\"ev-{e4}\""))
    );
}

#[tokio::test]
async fn last_event_id_header_takes_precedence_over_the_query() {
    let f = fixture().await;
    let e1 = f.add(1, "t1", chunk("m1", "one")).await;
    let e2 = f
        .add(
            2,
            "t1",
            json!({"toolCall": {"toolCallId": "c1", "title": "tool"}}),
        )
        .await;
    let request = Request::get("/tasks/DF-1/events?after=0")
        .header(header::COOKIE, &f.cookie)
        .header("Last-Event-ID", e1.to_string())
        .body(Body::empty())
        .unwrap();
    let mut sse = SseReader::new(f.app.send(request).await);
    let frame = sse.next().await;
    assert_eq!(frame.id.as_deref(), Some(e2.to_string().as_str()));
    assert!(frame.data.contains(&format!("id=\"ev-{e2}\"")));
    assert!(!frame.data.contains("id=\"msg-m1\""), "{}", frame.data);
}

#[tokio::test]
async fn reconnect_without_new_events_only_refreshes_the_regions() {
    let f = fixture().await;
    let e1 = f.add(1, "t1", chunk("m1", "one")).await;
    let mut sse = f.events(&format!("/tasks/DF-1/events?after={e1}")).await;
    let frame = sse.next().await;
    assert_eq!(frame.id, None);
    for region in ["#task-head", "#task-details", "#runs", "#pending"] {
        assert!(
            frame.data.contains(&format!("hx-target=\"{region}\"")),
            "{region}"
        );
    }
    assert!(!frame.data.contains("#transcript"));
}

#[tokio::test]
async fn chunks_of_one_message_are_coalesced_into_few_updates() {
    let f = fixture().await;
    // Already stored chunks arrive as one element with the full text.
    for seq in 1..=30 {
        f.add(seq, "t1", chunk("m1", &format!("w{seq} "))).await;
    }
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let frame = sse.next().await;
    assert_eq!(frame.data.matches("id=\"msg-m1\"").count(), 1);
    assert!(frame.data.contains("w1 w2 w3") && frame.data.contains("w30 "));

    // Chunks that arrive while connected are merged into few morphs of the same element.
    let total = 20;
    for seq in 31..31 + total {
        f.add(seq, "t1", chunk("m1", &format!("x{seq} "))).await;
    }
    let mut frames = 0;
    loop {
        let frame = sse.next().await;
        assert!(
            frame
                .data
                .contains(r##"hx-target="#msg-m1" hx-swap="outerMorph""##),
            "{}",
            frame.data
        );
        frames += 1;
        if frame.data.contains(&format!("x{} ", 30 + total)) {
            break;
        }
    }
    assert!(
        frames < usize::try_from(total).unwrap(),
        "{frames} updates for {total} chunks"
    );
}

#[tokio::test]
async fn message_is_rerendered_as_markdown_when_the_turn_ends() {
    let f = fixture().await;
    f.add(1, "t1", chunk("m1", "**bold**")).await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let first = sse.next().await;
    assert!(first.data.contains("**bold**") && !first.data.contains("<strong>"));

    f.add(2, "t1", ended()).await;
    let second = sse.next().await;
    assert!(
        second
            .data
            .contains(r##"hx-target="#msg-m1" hx-swap="outerMorph""##)
    );
    assert!(
        second.data.contains("<strong>bold</strong>"),
        "{}",
        second.data
    );
}

#[tokio::test]
async fn pull_request_link_and_pending_messages_update_live() {
    let f = fixture().await;
    f.queue("go on").await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let first = sse.next().await;
    assert!(!first.data.contains("PR #"));
    assert!(first.data.contains("go on"), "{}", first.data);

    sqlx::query("UPDATE tasks SET pr_number = 5, pr_url = 'https://git.example.org/o/r/pulls/5'")
        .execute(&f.app.pool)
        .await
        .unwrap();
    df_db::publish(
        &f.app.pool,
        &df_db::Notification::TaskChanged { task_id: f.task },
    )
    .await
    .unwrap();
    // Well below POLL, so only the notification can have triggered the update.
    let frame = sse.next_within(LIVE).await;
    assert!(frame.data.contains(r##"hx-target="#task-details""##));
    assert!(
        frame
            .data
            .contains("href=\"https://git.example.org/o/r/pulls/5\"")
            && frame.data.contains("PR #5")
    );

    // A message leaves the pending list as soon as the transcript shows it, even before
    // `delivered_at` is committed (nothing is published for that).
    let queued = f.human_messages().await;
    assert_eq!(queued.len(), 1);
    f.add(1, "t1", json!({"userMessage": {"promptId": queued[0].id.to_string(), "content": [{"text": {"text": "go on"}}]}})).await;
    let frame = sse.next().await;
    // The message is in the transcript now and left the stack.
    assert!(frame.data.contains("id=\"ev-"), "{}", frame.data);
    let stack = &frame.data[frame.data.find(r##"hx-target="#pending""##).expect("stack")..];
    assert!(!stack.contains("Queued"), "{stack}");
}

#[tokio::test]
async fn editing_and_removing_a_queued_message_rerenders_the_stack() {
    let f = fixture().await;
    let message = f.queue("first").await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    assert!(sse.next().await.data.contains("first"));

    let (actor, _) = crate::testkit::actor();
    df_db::edit_queued_message(&f.app.pool, f.task, message, "second", actor.clone())
        .await
        .unwrap();
    let edited = sse.next_within(LIVE).await;
    assert!(
        edited.data.contains(r##"hx-target="#pending""##) && edited.data.contains("second"),
        "{}",
        edited.data
    );

    df_db::remove_queued_message(&f.app.pool, f.task, message, actor)
        .await
        .unwrap();
    let removed = sse.next_within(LIVE).await;
    assert!(
        removed.data.contains(r##"hx-target="#pending""##) && !removed.data.contains("second"),
        "{}",
        removed.data
    );
}

#[tokio::test]
async fn the_live_stack_follows_the_task_status_for_forwarded_messages() {
    let f = fixture().await;
    f.queue("go on").await;
    df_db::claim_held_back_messages(&f.app.pool, f.task)
        .await
        .unwrap();
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let first = sse.next().await;
    assert!(
        first.data.contains("waiting for delivery"),
        "{}",
        first.data
    );

    // The task ends with the message still undelivered: the stack says so without a reload.
    df_db::update_task_status(&f.app.pool, f.task, TaskStatus::Failed)
        .await
        .unwrap();
    let frame = sse.next_within(LIVE).await;
    let stack = &frame.data[frame.data.find(r##"hx-target="#pending""##).expect("stack")..];
    assert!(
        stack.contains("not delivered, sent with the next run")
            && !stack.contains("waiting for delivery"),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn run_changes_rerender_the_runs_region() {
    let f = fixture().await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let first = sse.next().await;
    assert!(first.data.contains(">running<"), "{}", first.data);

    sqlx::query("UPDATE runs SET state = 'failed', outcome = 'failed' WHERE id = $1")
        .bind(f.run)
        .execute(&f.app.pool)
        .await
        .unwrap();
    df_db::publish(
        &f.app.pool,
        &df_db::Notification::RunChanged {
            run_id: df_core::RunId::from_uuid(f.run),
            task_id: f.task,
        },
    )
    .await
    .unwrap();
    let frame = sse.next_within(LIVE).await;
    assert!(
        frame.data.contains(r##"hx-target="#runs""##),
        "{}",
        frame.data
    );
    assert!(
        frame.data.contains(">failed<") && !frame.data.contains(">running<"),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn a_turn_end_refreshes_the_runs_and_details_regions_without_adding_an_item() {
    let f = fixture().await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    let first = sse.next().await;
    assert!(!first.data.contains("182k"), "{}", first.data);

    // Nothing is published: only the turn end itself can trigger the refresh.
    let id = f
        .add(
            1,
            "t1",
            json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN", "usage": [{"modelId": "opus", "inputTokens": "182345"}]}}),
        )
        .await;
    let frame = sse.next_within(LIVE).await;
    assert!(
        frame.data.contains(r##"hx-target="#runs""##) && frame.data.contains(">182k<"),
        "{}",
        frame.data
    );
    assert!(!frame.data.contains(&format!("id=\"ev-{id}\"")));
}

#[tokio::test]
async fn a_lost_notification_is_caught_by_the_poll() {
    let f = fixture().await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    sse.next().await;

    // No notification is published: only the poll can notice the change.
    sqlx::query("UPDATE runs SET state = 'failed', outcome = 'failed' WHERE id = $1")
        .bind(f.run)
        .execute(&f.app.pool)
        .await
        .unwrap();
    let frame = sse.next_within(super::POLL + Duration::from_secs(3)).await;
    assert!(
        frame.data.contains(r##"hx-target="#runs""##),
        "{}",
        frame.data
    );
    assert!(frame.data.contains(">failed<"), "{}", frame.data);
}

#[tokio::test]
async fn task_list_stream_shows_status_changes_within_two_seconds() {
    let f = fixture().await;
    let mut sse = f.events("/tasks/events").await;
    let first = sse.next().await;
    assert!(first.data.contains(r##"hx-target="#task-rows""##) && first.data.contains(">queued<"));

    df_db::update_task_status(&f.app.pool, f.task, TaskStatus::Running)
        .await
        .unwrap();
    let frame = sse.next_within(Duration::from_secs(2)).await;
    assert!(
        frame.data.contains(">running<") && !frame.data.contains(">queued<"),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn filtered_list_stream_drops_a_task_that_leaves_the_filter() {
    let f = fixture().await;
    let mut active = f.events("/tasks/events").await;
    let first = active.next().await;
    assert!(first.data.contains("id=\"row-DF-1\""), "{}", first.data);
    let mut done = f.events("/tasks/events?status=done").await;
    assert!(!done.next().await.data.contains("row-DF-1"));

    df_db::update_task_status(&f.app.pool, f.task, TaskStatus::Completed)
        .await
        .unwrap();

    // Well below POLL: only the notification can have triggered the updates.
    let left = active.next_within(LIVE).await;
    assert!(
        left.data.contains(r##"hx-target="#task-rows""##) && !left.data.contains("row-DF-1"),
        "{}",
        left.data
    );
    let joined = done.next_within(LIVE).await;
    assert!(joined.data.contains("id=\"row-DF-1\""), "{}", joined.data);
}

#[tokio::test]
async fn renaming_a_task_updates_the_list_live_in_regular_style() {
    let f = fixture().await;
    sqlx::query("UPDATE tasks SET title_source = 'placeholder'")
        .execute(&f.app.pool)
        .await
        .unwrap();
    let mut sse = f.events("/tasks/events").await;
    let first = sse.next().await;
    assert!(
        first.data.contains("italic text-muted\">t<"),
        "{}",
        first.data
    );

    let response = f
        .app
        .post(
            "/tasks/DF-1/title",
            &crate::testkit::form(&[("title", "Stop duplicate nightly backups")]),
            &f.cookie,
        )
        .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let frame = sse.next_within(Duration::from_secs(2)).await;
    assert!(frame.data.contains(r##"hx-target="#task-rows""##));
    assert!(
        frame.data.contains(">Stop duplicate nightly backups<"),
        "{}",
        frame.data
    );
    assert!(!frame.data.contains("italic"), "{}", frame.data);
}

#[tokio::test]
async fn project_filtered_list_stream_only_shows_that_project() {
    let f = fixture().await;
    let mut other = f.events("/tasks/events?project=NOPE").await;
    assert!(!other.next().await.data.contains("row-DF-1"));
    let mut own = f.events("/tasks/events?project=DF").await;
    assert!(own.next().await.data.contains("row-DF-1"));
}

#[tokio::test]
async fn narrower_status_filter_of_the_list_stream_follows_status_changes() {
    let f = fixture().await;
    let mut queued = f.events("/tasks/events?status=queued").await;
    let mut running = f.events("/tasks/events?status=running").await;
    assert!(queued.next().await.data.contains("row-DF-1"));
    assert!(!running.next().await.data.contains("row-DF-1"));

    df_db::update_task_status(&f.app.pool, f.task, TaskStatus::RateLimited)
        .await
        .unwrap();

    let left = queued.next_within(LIVE).await;
    assert!(!left.data.contains("row-DF-1"), "{}", left.data);
    let joined = running.next_within(LIVE).await;
    assert!(joined.data.contains("row-DF-1"), "{}", joined.data);
}

#[tokio::test]
async fn streams_of_unknown_tasks_are_404() {
    let f = fixture().await;
    let response = f.app.get("/tasks/DF-42/events", &f.cookie).await;
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn streams_require_login() {
    let f = fixture().await;
    let response = f.app.get("/tasks/DF-1/events", "").await;
    assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn streams_end_when_the_session_token_is_due_for_refresh() {
    let f = fixture().await;
    for uri in ["/tasks/DF-1/events?after=0", "/tasks/events"] {
        // Due for refresh one second from now (REFRESH_SKEW is 30 s).
        let (cookie, _) = crate::auth::testing::seed_session(
            &f.app.pool,
            &["board-member"],
            31,
            Some("refresh-0"),
        )
        .await;
        let mut sse = SseReader::new(f.app.get(uri, &cookie).await);
        sse.next().await;
        // The reconnect passes the auth middleware again, which refreshes the token and roles.
        assert!(
            sse.ends_within(Duration::from_secs(5)).await,
            "{uri} stayed open"
        );
    }
    // A fresh session keeps its stream open.
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    sse.next().await;
    assert!(!sse.ends_within(Duration::from_secs(2)).await);
}
