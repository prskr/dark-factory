//! Tests of the continuation summary.

use super::*;
use crate::assign::{build_assign, load_run_context};
use crate::runs::get_run;
use crate::testing::Env;
use df_core::{RunId, TaskId};
use df_proto::transcript::v1::ToolCallStatus;
use serde_json::{Value, json};
use sqlx::PgPool;

fn event(kind: EventKind) -> SummaryEvent {
    SummaryEvent {
        run_id: Uuid::nil(),
        kind,
        turn_id: String::new(),
        message_id: String::new(),
        text: String::new(),
        tool_call_id: String::new(),
        title: None,
        status: None,
    }
}

fn user(text: &str) -> SummaryEvent {
    SummaryEvent {
        text: text.into(),
        ..event(EventKind::UserMessage)
    }
}

fn chunk(message_id: &str, text: &str) -> SummaryEvent {
    SummaryEvent {
        message_id: message_id.into(),
        text: text.into(),
        ..event(EventKind::AgentMessageChunk)
    }
}

fn status(name: &str) -> Option<ToolCallStatus> {
    ToolCallStatus::from_str_name(name)
}

fn tool(run: Uuid, turn: &str, id: &str, title: &str, name: &str) -> SummaryEvent {
    SummaryEvent {
        run_id: run,
        turn_id: turn.into(),
        tool_call_id: id.into(),
        title: Some(title.to_string()).filter(|t| !t.is_empty()),
        status: status(name),
        ..event(EventKind::ToolCall)
    }
}

fn tool_update(run: Uuid, turn: &str, id: &str, title: &str, name: &str) -> SummaryEvent {
    SummaryEvent {
        kind: EventKind::ToolCallUpdate,
        ..tool(run, turn, id, title, name)
    }
}

const CAP: usize = 64 * 1024;

fn completed(run: Uuid, id: &str, title: &str) -> SummaryEvent {
    tool(run, "t", id, title, "TOOL_CALL_STATUS_COMPLETED")
}

fn cost(entries: &[String]) -> usize {
    entries.iter().map(|e| e.len() + SEPARATOR.len()).sum()
}

#[test]
fn text_messages_are_merged_by_message_id() {
    let events = [
        user("please fix"),
        chunk("m", "on "),
        chunk("m", "it"),
        chunk("n", "done"),
    ];

    let entries = fold_summary(&events, CAP);

    assert_eq!(entries, ["User: please fix", "Agent: on it", "Agent: done"]);
}

#[test]
fn a_tool_call_is_one_line_with_the_last_title_and_status() {
    let run = Uuid::from_u128(1);
    let events = [
        tool(run, "t1", "c", "cargo", "TOOL_CALL_STATUS_PENDING"),
        tool_update(run, "t1", "c", "cargo test", ""),
        tool_update(run, "t1", "c", "", "TOOL_CALL_STATUS_FAILED"),
    ];

    assert_eq!(fold_summary(&events, CAP), ["Tool: cargo test (failed)"]);
}

#[test]
fn the_same_tool_call_id_in_two_turns_stays_two_lines() {
    let run = Uuid::from_u128(1);
    let events = [
        tool(run, "t1", "c", "first", "TOOL_CALL_STATUS_PENDING"),
        tool(run, "t2", "c", "second", "TOOL_CALL_STATUS_PENDING"),
        tool_update(run, "t2", "c", "", "TOOL_CALL_STATUS_FAILED"),
    ];

    assert_eq!(
        fold_summary(&events, CAP),
        ["Tool: first (pending)", "Tool: second (failed)"]
    );
}

#[test]
fn consecutive_completed_calls_collapse_into_one_counted_line() {
    let run = Uuid::from_u128(1);
    let events: Vec<_> = (0..37)
        .map(|i| completed(run, &i.to_string(), "ls"))
        .collect();

    assert_eq!(fold_summary(&events, CAP), ["Tool: 37 calls completed"]);
}

#[test]
fn a_single_completed_call_keeps_its_own_line() {
    let run = Uuid::from_u128(1);

    assert_eq!(
        fold_summary(&[completed(run, "a", "ls")], CAP),
        ["Tool: ls (completed)"]
    );
}

#[test]
fn a_failed_call_breaks_a_streak_and_stays_individual() {
    let run = Uuid::from_u128(1);
    let events = [
        completed(run, "1", "a"),
        completed(run, "2", "b"),
        tool(run, "t", "3", "cargo test", "TOOL_CALL_STATUS_FAILED"),
        completed(run, "4", "c"),
        completed(run, "5", "d"),
        completed(run, "6", "e"),
    ];

    assert_eq!(
        fold_summary(&events, CAP),
        [
            "Tool: 2 calls completed",
            "Tool: cargo test (failed)",
            "Tool: 3 calls completed"
        ]
    );
}

#[test]
fn pending_and_in_progress_calls_are_not_collapsed() {
    let run = Uuid::from_u128(1);
    let events = [
        completed(run, "1", "a"),
        tool(run, "t", "2", "b", "TOOL_CALL_STATUS_IN_PROGRESS"),
        tool(run, "t", "3", "c", "TOOL_CALL_STATUS_PENDING"),
        completed(run, "4", "d"),
    ];

    assert_eq!(
        fold_summary(&events, CAP),
        [
            "Tool: a (completed)",
            "Tool: b (in progress)",
            "Tool: c (pending)",
            "Tool: d (completed)"
        ]
    );
}

#[test]
fn a_message_between_completed_calls_breaks_the_streak() {
    let run = Uuid::from_u128(1);
    let events = [
        completed(run, "1", "a"),
        completed(run, "2", "b"),
        chunk("m", "hm"),
        completed(run, "3", "c"),
    ];

    assert_eq!(
        fold_summary(&events, CAP),
        [
            "Tool: 2 calls completed",
            "Agent: hm",
            "Tool: c (completed)"
        ]
    );
}

#[test]
fn tool_lines_and_agent_messages_drop_before_user_messages_at_the_cap() {
    let run = Uuid::from_u128(1);
    let events = [
        tool(run, "t", "1", "old tool", "TOOL_CALL_STATUS_FAILED"),
        chunk("a", &"a".repeat(100)),
        user(&"u".repeat(100)),
        tool(run, "t", "2", "new tool", "TOOL_CALL_STATUS_FAILED"),
        chunk("b", &"b".repeat(100)),
    ];
    let full = fold_summary(&events, CAP);
    // Room for everything but the older tool line and the older agent message.
    let budget = cost(&full) - cost(&full[..2]);

    let entries = fold_summary(&events, budget);

    assert_eq!(entries.len(), 3, "{entries:?}");
    assert!(entries[0].starts_with("User: "));
    assert!(!entries.iter().any(|e| e.contains("old tool")));
    assert!(!entries.iter().any(|e| e.contains(&"a".repeat(100))));
}

#[test]
fn the_newest_entry_stays_when_it_is_a_tool_line_and_user_messages_drop() {
    let run = Uuid::from_u128(1);
    let events = [
        user(&"u".repeat(100)),
        chunk("a", &"a".repeat(100)),
        tool(run, "t", "1", "cargo test", "TOOL_CALL_STATUS_FAILED"),
    ];
    let full = fold_summary(&events, CAP);

    let entries = fold_summary(&events, cost(&full[2..]));

    assert_eq!(entries, ["Tool: cargo test (failed)"]);
}

#[test]
fn user_messages_drop_oldest_first_only_when_nothing_else_is_left() {
    let events = [
        user(&format!("first {}", "x".repeat(100))),
        chunk("a", "agent"),
        user(&format!("second {}", "x".repeat(100))),
        user(&format!("third {}", "x".repeat(100))),
    ];
    let full = fold_summary(&events, CAP);
    let budget = cost(&full) - cost(&full[..2]) + 1; // room for the last two users only

    let entries = fold_summary(&events, budget);

    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries[0].starts_with("User: second"));
    assert!(entries[1].starts_with("User: third"));
}

#[test]
fn the_newest_entry_is_kept_and_shortened_when_it_alone_exceeds_the_budget() {
    let events = [user("old"), user(&"z".repeat(40_000))];

    let entries = fold_summary(&events, 1000);

    assert_eq!(entries.len(), 1);
    assert!(cost(&entries) <= 1000);
    assert!(entries[0].starts_with("User: zzz"));
}

#[test]
fn kept_entries_stay_in_transcript_order_after_dropping() {
    let events: Vec<_> = (0..20)
        .map(|i| {
            chunk(
                &format!("m{i:02}"),
                &format!("msg-{i:02} {}", "y".repeat(1000)),
            )
        })
        .collect();

    let entries = fold_summary(&events, 5000);

    assert!(cost(&entries) <= 5000);
    assert!(entries.last().unwrap().contains("msg-19"));
    assert!(!entries.iter().any(|e| e.contains("msg-00")));
    let mut sorted = entries.clone();
    sorted.sort();
    assert_eq!(entries, sorted);
}

#[test]
fn truncation_keeps_char_boundaries() {
    assert_eq!(truncate_utf8("aäb".into(), 2), "a");
    assert_eq!(truncate_utf8("abc".into(), 10), "abc");
}

#[test]
fn a_long_header_is_cut_and_the_whole_summary_stays_within_the_cap() {
    let events: Vec<_> = (0..100)
        .map(|i| {
            chunk(
                &format!("m{i:03}"),
                &format!("msg-{i:03} {}", "y".repeat(1000)),
            )
        })
        .collect();

    let summary = render_summary("x".repeat(10 * 1024), &events);

    assert!(summary.len() <= SUMMARY_MAX_BYTES);
    assert!(summary.starts_with(&"x".repeat(SUMMARY_HEADER_MAX_BYTES)));
    assert!(summary.contains("msg-099"));
    assert!(!summary.contains("msg-000"));
}

async fn insert_event(
    pool: &PgPool,
    task: TaskId,
    run: RunId,
    seq: i64,
    kind: &str,
    payload: Value,
) {
    sqlx::query(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
         VALUES ($1, $2, $3, now(), $4, $5)",
    )
    .bind(task.as_uuid())
    .bind(run.as_uuid())
    .bind(seq)
    .bind(kind)
    .bind(payload)
    .execute(pool)
    .await
    .unwrap();
}

fn agent_payload(text: &str) -> Value {
    json!({"agentMessageChunk": {"messageId": text, "content": {"text": {"text": text}}}})
}

/// Task with run 1 (`succeeded`, session `S`) and a `connecting` run 2; returns the `Assign`
/// summary of run 2.
async fn run_two_summary(env: &Env, pool: &PgPool, run1: RunId) -> anyhow::Result<String> {
    sqlx::query(
        "UPDATE runs SET harness_session_id = 'S',
         state = CASE WHEN state = 'running' THEN 'succeeded' ELSE state END WHERE id = $1",
    )
    .bind(run1.as_uuid())
    .execute(pool)
    .await?;
    let run2 = env.insert_run(env.task.id, 2, "connecting").await;
    let rc = load_run_context(pool, get_run(pool, run2).await?.expect("run")).await?;
    Ok(build_assign(&env.ctx, &rc)
        .await?
        .continuation
        .expect("continuation")
        .summary)
}

#[tokio::test]
async fn the_assign_summary_has_run_one_events_and_none_of_the_assigned_run() -> anyhow::Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let task = env.task.id;
    let run1 = env.insert_run(task, 1, "running").await;
    insert_event(
        &pool,
        task,
        run1,
        1,
        "agent_message_chunk",
        agent_payload("one"),
    )
    .await;
    sqlx::query("UPDATE runs SET state = 'succeeded', harness_session_id = 'S' WHERE id = $1")
        .bind(run1.as_uuid())
        .execute(&pool)
        .await?;
    let run2 = env.insert_run(task, 2, "connecting").await;
    insert_event(
        &pool,
        task,
        run2,
        1,
        "agent_message_chunk",
        agent_payload("two"),
    )
    .await;
    let rc = load_run_context(&pool, get_run(&pool, run2).await?.expect("run")).await?;

    let summary = build_assign(&env.ctx, &rc)
        .await?
        .continuation
        .expect("continuation")
        .summary;

    assert!(summary.contains("Agent: one"), "{summary}");
    assert!(!summary.contains("Agent: two"), "{summary}");
    assert!(summary.contains("Fix login"), "{summary}");
    Ok(())
}

#[tokio::test]
async fn a_run_that_ended_without_a_report_is_in_the_summary() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run1 = env.insert_run(env.task.id, 1, "running").await;
    insert_event(
        &pool,
        env.task.id,
        run1,
        1,
        "agent_message_chunk",
        agent_payload("lost"),
    )
    .await;
    // Run 1 ends as `failed` (lost sandbox) and never sent a report.
    sqlx::query("UPDATE runs SET state = 'failed' WHERE id = $1")
        .bind(run1.as_uuid())
        .execute(&pool)
        .await?;

    let summary = run_two_summary(&env, &pool, run1).await?;

    assert!(summary.contains("Agent: lost"), "{summary}");
    Ok(())
}

#[tokio::test]
async fn the_oldest_message_of_a_long_transcript_is_in_the_summary() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let task = env.task.id;
    let run1 = env.insert_run(task, 1, "running").await;
    insert_event(
        &pool,
        task,
        run1,
        1,
        "user_message",
        json!({"userMessage": {"content": [{"text": {"text": "oldest"}}]}}),
    )
    .await;
    for seq in 2..=601 {
        insert_event(
            &pool,
            task,
            run1,
            seq,
            "agent_message_chunk",
            agent_payload(&format!("m{seq}")),
        )
        .await;
    }

    let summary = run_two_summary(&env, &pool, run1).await?;

    assert!(summary.contains("User: oldest"), "{summary}");
    assert!(summary.contains("Agent: m601"), "{summary}");
    Ok(())
}

#[tokio::test]
async fn a_tool_call_of_run_one_is_in_the_summary_of_run_two() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let task = env.task.id;
    let run1 = env.insert_run(task, 1, "running").await;
    insert_event(&pool, task, run1, 1, "tool_call", json!({"turnId": "t", "toolCall": {"toolCallId": "c", "title": "cargo test", "rawInput": "x".repeat(100)}})).await;
    insert_event(&pool, task, run1, 2, "tool_call_update", json!({"turnId": "t", "toolCallUpdate": {"toolCallId": "c", "status": "TOOL_CALL_STATUS_FAILED"}})).await;

    let summary = run_two_summary(&env, &pool, run1).await?;

    assert!(summary.contains("Tool: cargo test (failed)"), "{summary}");
    Ok(())
}

#[tokio::test]
async fn hidden_blocks_of_a_user_message_stay_out_of_the_assign_summary() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run1 = env.insert_run(env.task.id, 1, "running").await;
    insert_event(&pool, env.task.id, run1, 1, "user_message", json!({"userMessage": {"content": [{"text": {"text": "please fix"}}, {"hidden": true, "text": {"text": "secret instruction"}}]}})).await;

    let summary = run_two_summary(&env, &pool, run1).await?;

    assert!(summary.contains("User: please fix"), "{summary}");
    assert!(!summary.contains("secret instruction"), "{summary}");
    Ok(())
}

#[tokio::test]
async fn stored_events_of_a_previous_run_become_summary_entries() -> anyhow::Result<()> {
    use df_proto::transcript::v1 as wire;
    use wire::event::Kind;

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let task = env.task.id;
    let run1 = env.insert_run(task, 1, "running").await;
    let text = |t: &str, hidden| wire::ContentBlock {
        kind: Some(wire::content_block::Kind::Text(wire::TextContent {
            text: t.into(),
        })),
        hidden,
    };
    let kinds = [
        Kind::UserMessage(wire::UserMessage {
            content: vec![text("please fix", false), text("secret", true)],
            ..wire::UserMessage::default()
        }),
        Kind::AgentMessageChunk(wire::AgentMessageChunk {
            message_id: "m".into(),
            content: Some(text("on it", false)),
        }),
        Kind::ToolCall(wire::ToolCall {
            tool_call_id: "c".into(),
            title: "cargo test".into(),
            status: wire::ToolCallStatus::Pending as i32,
            raw_input_json: "{\"big\": true}".into(),
            ..wire::ToolCall::default()
        }),
        Kind::ToolCallUpdate(wire::ToolCallUpdate {
            tool_call_id: "c".into(),
            status: Some(wire::ToolCallStatus::Failed as i32),
            ..wire::ToolCallUpdate::default()
        }),
    ];
    let blobs = df_db::PgBlobStore::new(pool.clone());
    let mut tx = pool.begin().await?;
    for (seq, kind) in (1..).zip(kinds) {
        let event = wire::Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(kind),
        };
        let prepared = crate::transcript::prepare(&blobs, seq, event).await?;
        crate::transcript::insert(&mut tx, task, run1, &prepared).await?;
    }
    tx.commit().await?;

    let summary = run_two_summary(&env, &pool, run1).await?;

    assert!(summary.contains("User: please fix"), "{summary}");
    assert!(!summary.contains("secret"), "{summary}");
    assert!(summary.contains("Agent: on it"), "{summary}");
    assert!(summary.contains("Tool: cargo test (failed)"), "{summary}");
    assert!(!summary.contains("big"), "{summary}");
    Ok(())
}
