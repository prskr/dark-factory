use chrono::TimeZone;
use serde_json::{Value, json};

use super::*;

fn stored(id: i64, turn: &str, kind: Value) -> StoredEvent {
    let mut event = json!({"schemaVersion": 1, "turnId": turn});
    event
        .as_object_mut()
        .unwrap()
        .extend(kind.as_object().unwrap().clone());
    stored_in_run(
        id,
        1,
        Some(serde_json::from_value(event).expect("fixture decodes")),
    )
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 23, 10, minute, 0).unwrap()
}

fn stored_in_run(id: i64, run: i64, event: Option<df_proto::transcript::v1::Event>) -> StoredEvent {
    StoredEvent {
        id,
        event,
        occurred_at: at(u32::try_from(id).unwrap()),
        run_number: run,
        run_started_at: at(0),
    }
}

fn chunk(id: i64, turn: &str, message: &str, text: &str) -> StoredEvent {
    stored(
        id,
        turn,
        json!({"agentMessageChunk": {"messageId": message, "content": {"text": {"text": text}}}}),
    )
}

fn fold(events: &[StoredEvent]) -> Folder {
    let mut folder = Folder::default();
    for e in events {
        folder.apply(e);
    }
    folder
}

fn render_all(folder: &Folder) -> String {
    render_transcript(folder.items())
        .into_string()
        .replace("><", ">\n<")
}

/// One fixture per event kind, in the order a turn produces them.
fn every_kind() -> Vec<StoredEvent> {
    vec![
        stored(1, "t1", json!({"turnStarted": {"promptIds": ["p1"]}})),
        stored(
            2,
            "t1",
            json!({"userMessage": {"promptId": "p1",
            "author": {"human": {"userId": "u1", "displayName": "Peter"}},
            "content": [{"text": {"text": "please fix <b>login</b>"}}]}}),
        ),
        stored(
            3,
            "t1",
            json!({"agentThoughtChunk": {"messageId": "th1", "content": {"text": {"text": "hmm"}}}}),
        ),
        chunk(4, "t1", "m1", "Looking at "),
        chunk(5, "t1", "m1", "**the code**"),
        stored(
            6,
            "t1",
            json!({"toolCall": {"toolCallId": "c1", "title": "Run tests", "kind": "TOOL_KIND_EXECUTE",
            "status": "TOOL_CALL_STATUS_IN_PROGRESS", "rawInputJson": "{\"cmd\":\"cargo test\"}",
            "locations": [{"path": "src/lib.rs", "line": 3}]}}),
        ),
        stored(
            7,
            "t1",
            json!({"toolCallUpdate": {"toolCallId": "c1", "status": "TOOL_CALL_STATUS_COMPLETED",
            "content": {"items": [
                {"content": {"text": {"text": "ok <script>x</script>"}}},
                {"diff": {"path": "a.rs", "oldText": "old\nline", "newText": "new"}}]},
            "rawOutputJson": "{\"exit\":0}", "truncated": true}}),
        ),
        stored(
            8,
            "t1",
            json!({"plan": {"entries": [
            {"content": "write test", "priority": "PLAN_ENTRY_PRIORITY_HIGH", "status": "PLAN_ENTRY_STATUS_COMPLETED"},
            {"content": "fix", "status": "PLAN_ENTRY_STATUS_IN_PROGRESS"}]}}),
        ),
        stored(
            9,
            "t1",
            json!({"notice": {"level": "NOTICE_LEVEL_WARNING", "code": "workspace.diverged", "message": "branch diverged"}}),
        ),
        stored(
            10,
            "t1",
            json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN", "detail": "done",
            "usage": [{"modelId": "opus", "inputTokens": "10", "outputTokens": "5", "cacheReadTokens": "2", "cacheWriteTokens": "1"}]}}),
        ),
    ]
}

#[test]
fn renders_every_event_kind() {
    insta::assert_snapshot!(render_all(&fold(&every_kind())));
}

#[test]
fn tool_call_before_its_update_renders_the_collapsed_summary() {
    let folder = fold(&every_kind()[..6]);
    insta::assert_snapshot!(render_all(&folder));
}

#[test]
fn chunks_of_one_message_share_one_element_that_grows() {
    let mut folder = Folder::default();
    let first = folder.apply(&chunk(1, "t", "m1", "Hi"));
    let second = folder.apply(&chunk(2, "t", "m1", " there"));
    // The run divider comes first.
    let added = |index| Change { index, added: true };
    assert_eq!(first, [added(0), added(1)]);
    assert_eq!(
        second,
        [Change {
            index: 1,
            added: false
        }]
    );
    assert_eq!(folder.items().len(), 2);
    let html = render_item(&folder.items()[1]).into_string();
    assert!(
        html.contains(r#"id="msg-r1-m1""#) && html.contains("Hi there"),
        "{html}"
    );
    assert_eq!(folder.last_id(), 2);
}

#[test]
fn turn_end_completes_agent_messages_and_renders_markdown() {
    let mut folder = fold(&[
        chunk(1, "t", "m1", "**bold**"),
        chunk(2, "u", "m2", "**other**"),
    ]);
    assert!(
        render_item(&folder.items()[1])
            .into_string()
            .contains("**bold**")
    );
    let changes = folder.apply(&stored(
        3,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    // The message is re-rendered; the turn end adds no item.
    assert_eq!(
        changes,
        [Change {
            index: 1,
            added: false
        }]
    );
    assert_eq!(folder.items().len(), 3);
    let done = render_item(&folder.items()[1]).into_string();
    assert!(done.contains("<strong>bold</strong>"), "{done}");
    // Another turn's message is untouched.
    assert!(
        render_item(&folder.items()[2])
            .into_string()
            .contains("**other**")
    );
}

#[test]
fn a_turn_end_without_messages_changes_nothing() {
    let mut folder = Folder::default();
    let changes = folder.apply(&stored(
        1,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    assert!(changes.is_empty());
    assert!(folder.items().is_empty());
    assert_eq!(folder.last_id(), 1);
}

#[test]
fn html_in_agent_output_is_shown_literally() {
    let mut folder = fold(&[chunk(
        1,
        "t",
        "m1",
        "<script>alert(1)</script> and [x](javascript:alert(2))",
    )]);
    let streaming = render_item(&folder.items()[1]).into_string();
    folder.apply(&stored(
        2,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    let complete = render_item(&folder.items()[1]).into_string();
    for html in [&streaming, &complete] {
        assert!(!html.contains("<script>"), "{html}");
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{html}"
        );
    }
    // Rendered Markdown does not link unsafe schemes.
    assert!(!complete.contains("href=\"javascript:"), "{complete}");
}

#[test]
fn tool_details_are_escaped() {
    let folder = fold(&every_kind()[..7]);
    let html = render_all(&folder);
    assert!(!html.contains("<script>x"), "{html}");
    assert!(html.contains("ok &lt;script&gt;x&lt;/script&gt;"));
}

#[test]
fn update_of_an_unknown_tool_call_creates_it() {
    let mut folder = Folder::default();
    let changes = folder.apply(&stored(
        5,
        "t",
        json!({"toolCallUpdate": {"toolCallId": "x", "title": "late"}}),
    ));
    let added = |index| Change { index, added: true };
    assert_eq!(changes, [added(0), added(1)]);
}

#[test]
fn plan_updates_replace_the_plan_of_the_turn() {
    let plan = |id, text: &str| stored(id, "t", json!({"plan": {"entries": [{"content": text}]}}));
    let mut folder = Folder::default();
    folder.apply(&plan(1, "one"));
    let changes = folder.apply(&plan(2, "two"));
    assert_eq!(
        changes,
        [Change {
            index: 1,
            added: false
        }]
    );
    assert!(render_all(&folder).contains("two") && !render_all(&folder).contains("one"));
}

#[test]
fn undecodable_events_render_a_placeholder() {
    let folder = fold(&[stored_in_run(3, 1, None)]);
    assert!(render_all(&folder).contains("Unreadable transcript event"));
    assert_eq!(folder.last_id(), 3);
}

#[test]
fn runtime_ids_cannot_break_out_of_dom_ids() {
    let folder = fold(&[chunk(1, "t", "a b\"#<", "x")]);
    assert_eq!(folder.items()[1].dom_id, "msg-r1-a_b___");
}

#[test]
fn a_long_code_line_stays_inside_a_block_that_scrolls_horizontally() {
    let long = "x".repeat(300);
    let mut folder = Folder::default();
    folder.apply(&chunk(1, "t1", "m1", &format!("```\n{long}\n```")));
    folder.apply(&stored(
        2,
        "t1",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    let html = render_all(&folder);
    // The code is in a `pre` inside the Markdown body, not in the page flow ...
    let body = &html[html.find("md-body").expect("markdown body")..];
    let pre = &body[body.find("<pre>").expect("pre block")..];
    assert!(pre[..pre.find("</pre>").unwrap()].contains(&long));
    // ... and the stylesheet makes exactly that block scroll.
    let css = crate::assets::app_css().expect("app.css is embedded");
    let rule = &css[css.find(".md-body pre{").expect("pre rule")..];
    assert!(rule[..rule.find('}').unwrap()].contains("overflow-x:auto"));
}

#[test]
fn each_run_starts_with_a_divider_before_its_first_item() {
    let in_run = |_id: i64, run: i64, event: StoredEvent| StoredEvent {
        run_number: run,
        run_started_at: at(u32::try_from(run).unwrap() * 10),
        ..event
    };
    let mut folder = Folder::default();
    // TurnStarted produces no item: the divider waits for the first one.
    let started = folder.apply(&in_run(1, 1, stored(1, "t1", json!({"turnStarted": {}}))));
    assert!(started.is_empty());
    folder.apply(&in_run(2, 1, chunk(2, "t1", "m1", "one")));
    folder.apply(&in_run(3, 1, chunk(3, "t1", "m1", " more")));
    let second = folder.apply(&in_run(4, 2, chunk(4, "t2", "m2", "two")));

    let ids: Vec<&str> = folder.items().iter().map(|i| i.dom_id.as_str()).collect();
    assert_eq!(ids, ["run-1", "msg-r1-m1", "run-2", "msg-r2-m2"]);
    assert_eq!(
        second
            .iter()
            .map(|c| (c.index, c.added))
            .collect::<Vec<_>>(),
        [(2, true), (3, true)]
    );
    let divider = render_item(&folder.items()[2]).into_string();
    assert!(
        divider.contains("Run #2 started") && divider.contains("10:20"),
        "{divider}"
    );
}

fn assignment(blocks: Value) -> StoredEvent {
    stored(
        2,
        "t",
        json!({"userMessage": {"promptId": "p1", "content": blocks}}),
    )
}

#[test]
fn hidden_blocks_are_left_out_and_an_all_hidden_message_is_skipped() {
    let mut folder = Folder::new("Linus");
    let changes = folder.apply(&assignment(json!([
        {"text": {"text": "Fix login"}},
        {"hidden": true, "text": {"text": "call set_task_title"}},
    ])));
    assert_eq!(changes.len(), 2, "run divider and message");
    let html = render_all(&folder);
    assert!(html.contains("Fix login"), "{html}");
    assert!(
        !html.contains("set_task_title") && !html.contains("Instruction"),
        "{html}"
    );

    let mut folder = Folder::new("Linus");
    let changes = folder.apply(&assignment(
        json!([{"hidden": true, "text": {"text": "secret"}}]),
    ));
    assert!(changes.is_empty());
    assert!(folder.items().is_empty());
}

#[test]
fn hidden_blocks_are_shown_muted_with_the_option() {
    let mut folder = Folder::new("Linus").with_hidden_blocks(true);
    folder.apply(&assignment(json!([
        {"text": {"text": "Fix login"}},
        {"hidden": true, "text": {"text": "call set_task_title"}},
    ])));
    let html = render_all(&folder);
    assert!(
        html.contains("Fix login") && html.contains("call set_task_title"),
        "{html}"
    );
    assert!(html.contains("Instruction for the agent"), "{html}");

    let mut folder = Folder::new("Linus").with_hidden_blocks(true);
    let changes = folder.apply(&assignment(
        json!([{"hidden": true, "text": {"text": "secret"}}]),
    ));
    assert_eq!(
        changes.len(),
        2,
        "an all-hidden message is rendered with the option"
    );
}

#[test]
fn agent_items_carry_the_persona_name_and_the_time_of_their_first_event() {
    let mut folder = Folder::new("Linus");
    folder.apply(&chunk(7, "t", "m1", "Hi"));
    folder.apply(&chunk(9, "t", "m1", " there"));
    let item = &folder.items()[1];
    assert_eq!(item.author, "Linus");
    assert_eq!(item.at, at(7));
    let html = render_item(item).into_string();
    assert!(
        html.contains("Linus · <time") && html.contains(">10:07<"),
        "{html}"
    );
    // Without a name the bubble says "Agent".
    let mut anonymous = Folder::default();
    anonymous.apply(&chunk(1, "t", "m1", "Hi"));
    assert!(
        render_item(&anonymous.items()[1])
            .into_string()
            .contains("Agent · <time")
    );
}

#[test]
fn tool_calls_sit_between_agent_messages_as_agent_items() {
    let mut folder = Folder::new("Linus");
    for event in every_kind().iter().take(7) {
        folder.apply(event);
    }
    let html = render_all(&folder);
    // Messages and the tool row are `agent`, the board member's message `human`: CSS groups them.
    assert_eq!(html.matches("data-author=\"agent\"").count(), 3, "{html}");
    assert_eq!(html.matches("data-author=\"human\"").count(), 1);
}

#[test]
fn agent_writes_twice_with_a_collapsed_tool_row_between_and_css_that_hides_the_first_avatar() {
    let mut folder = Folder::new("Linus");
    folder.apply(&chunk(1, "t", "m1", "first"));
    folder.apply(&stored(
        2,
        "t",
        json!({"toolCall": {"toolCallId": "c1", "title": "Run tests", "kind": "TOOL_KIND_EXECUTE"}}),
    ));
    folder.apply(&chunk(3, "t", "m2", "second"));

    let ids: Vec<&str> = folder.items().iter().map(|i| i.dom_id.as_str()).collect();
    assert_eq!(ids, ["run-1", "msg-r1-m1", "ev-2", "msg-r1-m2"]);
    let tool = render_item(&folder.items()[2]).into_string();
    assert!(
        tool.starts_with("<details ") && !tool.contains(" open"),
        "{tool}"
    );
    for message in [1, 3] {
        let html = render_item(&folder.items()[message]).into_string();
        assert!(html.contains("data-author=\"agent\"") && html.contains("class=\"avatar "));
    }
    // The stylesheet hides an avatar when the next item is by the same author.
    let css = crate::assets::app_css().expect("app.css is embedded");
    assert!(css.contains(":has(+[data-author=agent]) .avatar"), "{css}");
}

#[test]
fn message_ids_restart_per_run_and_stay_separate_items() {
    let in_run = |id, run, text| {
        let event = json!({"schemaVersion": 1, "turnId": "turn_1", "agentMessageChunk": {"messageId": "msg-1", "content": {"text": {"text": text}}}});
        stored_in_run(
            id,
            run,
            Some(serde_json::from_value(event).expect("fixture decodes")),
        )
    };
    let folder = fold(&[in_run(1, 1, "first"), in_run(2, 2, "second")]);
    let texts: Vec<_> = folder
        .items()
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Agent { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["first", "second"]);
    let ids: std::collections::HashSet<_> = folder.items().iter().map(|i| &i.dom_id).collect();
    assert_eq!(ids.len(), folder.items().len(), "dom ids must be unique");
}
