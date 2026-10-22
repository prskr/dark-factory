use serde_json::{Value, json};

use super::*;

fn stored(id: i64, turn: &str, kind: Value) -> StoredEvent {
    let mut event = json!({"schemaVersion": 1, "turnId": turn});
    event
        .as_object_mut()
        .unwrap()
        .extend(kind.as_object().unwrap().clone());
    StoredEvent {
        id,
        event: Some(serde_json::from_value(event).expect("fixture decodes")),
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
    assert_eq!(
        first,
        [Change {
            index: 0,
            added: true
        }]
    );
    assert_eq!(
        second,
        [Change {
            index: 0,
            added: false
        }]
    );
    assert_eq!(folder.items().len(), 1);
    let html = render_item(&folder.items()[0]).into_string();
    assert!(
        html.contains(r#"id="msg-m1""#) && html.contains("Hi there"),
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
        render_item(&folder.items()[0])
            .into_string()
            .contains("**bold**")
    );
    let changes = folder.apply(&stored(
        3,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    assert_eq!(changes.len(), 2);
    assert_eq!(
        changes[0],
        Change {
            index: 0,
            added: false
        }
    );
    assert!(changes[1].added);
    let done = render_item(&folder.items()[0]).into_string();
    assert!(done.contains("<strong>bold</strong>"), "{done}");
    // Another turn's message is untouched.
    assert!(
        render_item(&folder.items()[1])
            .into_string()
            .contains("**other**")
    );
}

#[test]
fn html_in_agent_output_is_shown_literally() {
    let mut folder = fold(&[chunk(
        1,
        "t",
        "m1",
        "<script>alert(1)</script> and [x](javascript:alert(2))",
    )]);
    let streaming = render_item(&folder.items()[0]).into_string();
    folder.apply(&stored(
        2,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    let complete = render_item(&folder.items()[0]).into_string();
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
    assert_eq!(
        changes,
        [Change {
            index: 0,
            added: true
        }]
    );
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
            index: 0,
            added: false
        }]
    );
    assert!(render_all(&folder).contains("two") && !render_all(&folder).contains("one"));
}

#[test]
fn undecodable_events_render_a_placeholder() {
    let folder = fold(&[StoredEvent { id: 3, event: None }]);
    assert!(render_all(&folder).contains("Unreadable transcript event"));
    assert_eq!(folder.last_id(), 3);
}

#[test]
fn runtime_ids_cannot_break_out_of_dom_ids() {
    let folder = fold(&[chunk(1, "t", "a b\"#<", "x")]);
    assert_eq!(folder.items()[0].dom_id, "msg-a_b___");
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
