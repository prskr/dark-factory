use jiff::SignedDuration;
use serde_json::{Value, json};

use super::*;
use crate::components::Icon;
use crate::testkit::tag_around;

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

fn at(minute: u32) -> Timestamp {
    format!("2026-10-23T10:{minute:02}:00Z").parse().unwrap()
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
    render_transcript(folder.items(), &[])
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
            json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN", "detail": "done"}}),
        ),
    ]
}

#[test]
fn system_message_authors_show_a_readable_name_never_the_reason() {
    for (reason, name) in [
        ("task.assigned", "Task assignment"),
        ("unknown.reason", "Dark Factory"),
    ] {
        let folder = fold(&[stored(
            1,
            "t1",
            json!({"userMessage": {
                "promptId": "p1",
                "author": {"system": {"reason": reason}},
                "content": [{"text": {"text": "System message"}}]
            }}),
        )]);

        let html = render_all(&folder);
        assert!(html.contains(name), "{html}");
        assert!(!html.contains(reason), "{html}");
    }
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
#[cfg_attr(
    not(df_web_styles),
    ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
)]
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
    let pre = &body[body.find("<pre class=\"hl-code\">").expect("pre block")..];
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
    // Messages, the tool group and its row are `agent`, the board member's message `human`: CSS
    // groups them.
    assert_eq!(html.matches("data-author=\"agent\"").count(), 4, "{html}");
    assert_eq!(html.matches("data-author=\"human\"").count(), 1);
}

#[test]
#[cfg_attr(
    not(df_web_styles),
    ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
)]
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

#[test]
fn a_card_goes_before_an_item_recorded_at_the_same_time() {
    assert!(card_goes_before(at(5), at(5)));
    assert!(card_goes_before(at(5), at(6)));
    assert!(!card_goes_before(at(6), at(5)));
}

/// A forge activity event as the control plane stores it (no turn).
fn forge(id: i64, activity: Value) -> StoredEvent {
    stored(id, "", json!({"forgeActivity": activity}))
}

fn pushed(verdict: &str, old: &str, reason: &str) -> Value {
    json!({"branchPushed": {"ref": "refs/heads/factory/task/DF-1", "oldSha": old,
        "newSha": "def5678901234567890123456789012345678901", "verdict": verdict, "reason": reason}})
}

const OLD: &str = "abc1234567890123456789012345678901234567";

fn forge_row(event: &StoredEvent) -> String {
    let folder = fold(std::slice::from_ref(event));
    // The run divider comes first.
    render_item(folder.items().last().unwrap()).into_string()
}

#[test]
fn accepted_push_row_shows_the_branch_and_the_short_shas_with_the_commit_icon() {
    let html = forge_row(&forge(1, pushed("PUSH_VERDICT_ACCEPTED", OLD, "")));

    assert!(html.contains("id=\"ev-1\""), "{html}");
    assert!(html.contains(&c_icon(Icon::GitCommit)), "{html}");
    assert!(
        html.contains("Pushed factory/task/DF-1 abc1234..def5678"),
        "{html}"
    );
    assert!(
        html.contains("text-accent-text") && !html.contains("text-danger"),
        "{html}"
    );
}

#[test]
fn push_that_created_the_branch_shows_the_new_sha_alone() {
    let html = forge_row(&forge(
        1,
        pushed("PUSH_VERDICT_ACCEPTED", &"0".repeat(40), ""),
    ));

    assert!(html.contains("Pushed factory/task/DF-1 def5678"), "{html}");
    assert!(!html.contains(".."), "{html}");
}

#[test]
fn rejected_and_failed_pushes_are_tinted_as_errors_and_name_the_reason() {
    for verdict in ["PUSH_VERDICT_REJECTED", "PUSH_VERDICT_UPSTREAM_FAILED"] {
        let html = forge_row(&forge(1, pushed(verdict, OLD, "df:non-fast-forward")));

        assert!(html.contains(&c_icon(Icon::GitCommit)), "{html}");
        assert!(
            html.contains("Push to factory/task/DF-1 rejected df:non-fast-forward"),
            "{html}"
        );
        assert!(
            html.contains("text-danger") && !html.contains("text-accent-text"),
            "{html}"
        );
    }
}

#[test]
fn pull_request_rows_have_their_own_icon_text_and_link() {
    let url = "https://git.example.org/o/r/pulls/12";
    let draft = forge_row(&forge(
        1,
        json!({"pullRequestOpened": {"number": "12", "url": url, "draft": true}}),
    ));
    assert!(draft.contains(&c_icon(Icon::PullRequestDraft)), "{draft}");
    assert!(draft.contains("Opened draft PR "), "{draft}");
    assert!(
        draft.contains(&format!("<a href=\"{url}\"")) && draft.contains(">#12</a>"),
        "{draft}"
    );

    let open = forge_row(&forge(
        1,
        json!({"pullRequestOpened": {"number": "12", "url": url}}),
    ));
    assert!(open.contains(&c_icon(Icon::PullRequest)), "{open}");
    assert!(open.contains("Opened PR "), "{open}");

    let ready = forge_row(&forge(
        1,
        json!({"pullRequestReady": {"number": "12", "url": url}}),
    ));
    assert!(ready.contains(&c_icon(Icon::PullRequest)), "{ready}");
    assert!(ready.contains(">#12</a> ready for review"), "{ready}");
    assert!(ready.contains("text-accent-text"), "{ready}");
}

#[test]
fn a_pull_request_url_that_is_not_http_is_shown_without_a_link() {
    let html = forge_row(&forge(
        1,
        json!({"pullRequestReady": {"number": "12", "url": "javascript:alert(1)"}}),
    ));

    assert!(html.contains("#12 ready for review"), "{html}");
    assert!(
        !html.contains("<a ") && !html.contains("javascript"),
        "{html}"
    );
}

#[test]
fn forge_rows_share_divider_lines_with_tool_and_notice_rows() {
    let html = forge_row(&forge(1, pushed("PUSH_VERDICT_ACCEPTED", OLD, "")));

    assert!(html.contains("data-system"), "{html}");
}

#[test]
fn forge_rows_sit_in_order_between_the_agent_messages() {
    let folder = fold(&[
        chunk(1, "t1", "m1", "before"),
        forge(2, pushed("PUSH_VERDICT_ACCEPTED", OLD, "")),
        chunk(3, "t2", "m2", "after"),
    ]);

    let ids: Vec<&str> = folder.items().iter().map(|i| i.dom_id.as_str()).collect();
    assert_eq!(ids, ["run-1", "msg-r1-m1", "ev-2", "msg-r1-m2"]);
}

fn c_icon(glyph: Icon) -> String {
    crate::components::icon(glyph).into_string()
}

#[test]
fn code_is_highlighted_only_after_the_turn_ends() {
    let mut folder = fold(&[chunk(1, "t", "m1", "```rust\nlet x = 1;\n```")]);
    let streaming = render_item(&folder.items()[1]).into_string();
    assert!(!streaming.contains("hl-"), "{streaming}");
    folder.apply(&stored(
        2,
        "t",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    ));
    let complete = render_item(&folder.items()[1]).into_string();
    assert!(complete.contains("hl-keyword"), "{complete}");
}

#[test]
fn an_interrupted_turn_ends_with_the_row_naming_who_interrupted() {
    let folder = fold(&[
        chunk(1, "t1", "m1", "working"),
        stored(
            2,
            "t1",
            json!({"turnEnded": {"stopReason": "STOP_REASON_CANCELLED",
                "detail": "Interrupted by Peter Kurfer", "interruptedBy": "Peter Kurfer"}}),
        ),
        chunk(3, "t2", "m2", "next turn"),
    ]);

    let ids: Vec<&str> = folder.items().iter().map(|i| i.dom_id.as_str()).collect();
    assert_eq!(ids, ["run-1", "msg-r1-m1", "ev-2", "msg-r1-m2"]);
    let row = render_item(&folder.items()[2]).into_string();
    assert!(row.contains("Interrupted by Peter Kurfer"), "{row}");
    assert!(row.contains("data-system"), "{row}");
}

#[test]
fn a_turn_that_was_not_interrupted_adds_no_row() {
    let folder = fold(&[
        chunk(1, "t1", "m1", "working"),
        stored(
            2,
            "t1",
            json!({"turnEnded": {"stopReason": "STOP_REASON_CANCELLED"}}),
        ),
    ]);

    assert_eq!(folder.items().len(), 2);
}

/// An artifact-written event as the control plane stores it (no turn).
fn artifact_written(id: i64, name: &str, mime_type: &str) -> StoredEvent {
    stored(
        id,
        "",
        json!({"factoryActivity": {"artifactWritten": {
            "name": name, "kind": "ARTIFACT_KIND_TEXT", "mimeType": mime_type, "size": "12"}}}),
    )
}

#[test]
fn an_artifact_written_event_renders_the_artifact_row_with_the_label_of_its_mime_type() {
    let folder = fold(&[artifact_written(1, "findings.md", "text/markdown")]);

    let html = render_item(folder.items().last().unwrap()).into_string();

    assert!(html.contains("id=\"ev-1\""), "{html}");
    assert!(
        html.contains("data-artifact-written=\"findings.md\""),
        "{html}"
    );
    assert!(
        html.contains("x-on:click=\"open('findings.md')\""),
        "{html}"
    );
    assert!(html.contains("\u{b7} Markdown"), "{html}");
}

#[test]
fn an_artifact_written_event_of_an_unknown_kind_renders_as_a_text_row() {
    let folder = fold(&[stored(
        1,
        "",
        json!({"factoryActivity": {"artifactWritten": {
            "name": "notes.txt", "kind": "ARTIFACT_KIND_UNSPECIFIED", "mimeType": "text/plain"}}}),
    )]);

    let html = render_item(folder.items().last().unwrap()).into_string();

    assert!(
        html.contains("data-artifact-written=\"notes.txt\""),
        "{html}"
    );
    assert!(!html.contains("<circle"), "{html}");
}

#[test]
fn an_artifact_written_event_with_an_invalid_name_adds_no_row() {
    let folder = fold(&[artifact_written(1, "x');alert(1);('", "text/markdown")]);

    assert!(
        folder
            .items()
            .iter()
            .all(|i| !matches!(i.kind, ItemKind::ArtifactWritten(_)))
    );
}

/// The `write_artifact` call as Claude Code and `OpenCode` (inside its `execute` tool) report it.
fn write_artifact_harness_calls() -> [(&'static str, String); 2] {
    [
        (
            "mcp__factory__write_artifact",
            json!({"name": "findings.md", "content": "x"}).to_string(),
        ),
        (
            "execute",
            json!({"code": "await tools.factory.write_artifact({name: 'findings.md'})"})
                .to_string(),
        ),
    ]
}

fn write_artifact_tool_call(title: &str, input: &str, status: &str) -> StoredEvent {
    stored(
        1,
        "t1",
        json!({"toolCall": {"toolCallId": "c1", "title": title, "kind": "TOOL_KIND_OTHER",
            "status": status, "rawInputJson": input}}),
    )
}

#[test]
fn a_failed_write_artifact_call_shows_only_its_tool_row() {
    for (title, input) in write_artifact_harness_calls() {
        let html = render_all(&fold(&[write_artifact_tool_call(
            title,
            &input,
            "TOOL_CALL_STATUS_FAILED",
        )]));

        assert!(html.contains(title), "{html}");
        assert!(!html.contains("data-artifact-written"), "{html}");
    }
}

#[test]
fn write_artifact_tool_calls_of_every_harness_keep_their_tool_row_above_the_artifact_row() {
    for (title, input) in write_artifact_harness_calls() {
        let written = render_all(&fold(&[
            write_artifact_tool_call(title, &input, "TOOL_CALL_STATUS_COMPLETED"),
            artifact_written(2, "findings.md", "text/markdown"),
        ]));
        let tool_at = written.find(title).expect("tool row");
        let artifact_at = written
            .find("data-artifact-written=\"findings.md\"")
            .expect("artifact row");
        assert!(tool_at < artifact_at, "{written}");
        assert!(written.contains("\u{b7} Markdown"), "{written}");
    }
}

const COMPLETED: &str = "TOOL_CALL_STATUS_COMPLETED";

fn tool_call(id: i64, status: &str) -> StoredEvent {
    stored(
        id,
        "t1",
        json!({"toolCall": {"toolCallId": format!("c{id}"), "title": format!("Call {id}"),
            "kind": "TOOL_KIND_EXECUTE", "status": status}}),
    )
}

fn thought_chunk(id: i64, text: &str) -> StoredEvent {
    stored(
        id,
        "t1",
        json!({"agentThoughtChunk": {"messageId": format!("th{id}"), "content": {"text": {"text": text}}}}),
    )
}

fn notice_event(id: i64) -> StoredEvent {
    stored(
        id,
        "t1",
        json!({"notice": {"level": "NOTICE_LEVEL_INFO", "code": "x", "message": "m"}}),
    )
}

fn groups_of(folder: &Folder) -> Vec<Option<&str>> {
    folder
        .items()
        .iter()
        .map(|item| item.group.as_deref())
        .collect()
}

fn turn_end(id: i64) -> StoredEvent {
    stored(
        id,
        "t1",
        json!({"turnEnded": {"stopReason": "STOP_REASON_END_TURN"}}),
    )
}

#[test]
fn a_single_tool_call_is_a_group_of_one_whose_head_is_hidden() {
    let folder = fold(&[
        chunk(1, "t1", "m1", "hi"),
        tool_call(2, COMPLETED),
        chunk(3, "t1", "m2", "ok"),
    ]);

    // run divider, message, tool, message
    assert_eq!(groups_of(&folder), [None, None, Some("grp-ev-2"), None]);
    let html = render_all(&folder);
    assert!(
        html.contains("id=\"grp-ev-2-head\" data-group-head data-single"),
        "{html}"
    );
}

#[test]
fn a_streak_of_tool_calls_with_thoughts_after_the_first_call_is_one_group() {
    let events = [
        thought_chunk(1, "before the first call"),
        tool_call(2, COMPLETED),
        thought_chunk(3, "between"),
        tool_call(4, "TOOL_CALL_STATUS_FAILED"),
        notice_event(5),
        tool_call(6, COMPLETED),
        thought_chunk(7, "trailing"),
        chunk(8, "t1", "m1", "done"),
    ];
    let folder = fold(&events);

    // run divider, thought (outside), tool, thought, tool, notice, tool, thought, message
    let group = Some("grp-ev-2");
    assert_eq!(
        groups_of(&folder),
        [None, None, group, group, group, group, group, group, None]
    );
    // Thoughts and notices do not count as tool calls.
    let html = render_all(&folder);
    assert!(
        html.contains("3 tool calls") && html.contains("1 failed"),
        "{html}"
    );
}

#[test]
fn a_message_splits_a_streak_into_two_groups() {
    let mut events = vec![
        tool_call(1, COMPLETED),
        tool_call(2, COMPLETED),
        tool_call(3, COMPLETED),
    ];
    events.push(chunk(4, "t1", "m1", "between"));
    events.extend((5..=8).map(|id| tool_call(id, COMPLETED)));
    let html = render_all(&fold(&events));

    assert!(
        html.contains("3 tool calls") && html.contains("4 tool calls"),
        "{html}"
    );
    assert!(
        html.find("3 tool calls").unwrap() < html.find("between").unwrap()
            && html.find("between").unwrap() < html.find("4 tool calls").unwrap(),
        "{html}"
    );
}

#[test]
fn a_run_divider_and_other_rows_end_a_group() {
    let mut folder = fold(&[tool_call(1, COMPLETED), tool_call(2, COMPLETED)]);
    // The next run starts with a tool call: a new group behind the divider.
    folder.apply(&stored_in_run(
        3,
        2,
        Some(
            serde_json::from_value(json!({"schemaVersion": 1, "turnId": "t2",
                "toolCall": {"toolCallId": "c3", "title": "Call 3"}}))
            .unwrap(),
        ),
    ));

    assert_eq!(
        groups_of(&folder),
        [
            None,
            Some("grp-ev-1"),
            Some("grp-ev-1"),
            None,
            Some("grp-ev-3")
        ]
    );
}

#[test]
fn the_head_counts_failed_calls_and_shows_the_running_one() {
    let folder = fold(&[
        tool_call(1, "TOOL_CALL_STATUS_FAILED"),
        tool_call(2, COMPLETED),
        tool_call(3, "TOOL_CALL_STATUS_IN_PROGRESS"),
    ]);
    let html = render_all(&folder);

    assert!(
        html.contains("3 tool calls") && html.contains("1 failed"),
        "{html}"
    );
    assert!(
        html.contains("animate-spin") && html.contains("running"),
        "{html}"
    );

    let idle = render_all(&fold(&[tool_call(1, COMPLETED), tool_call(2, COMPLETED)]));
    assert!(
        !idle.contains("animate-spin") && !idle.contains("failed"),
        "{idle}"
    );
}

#[test]
fn a_fold_of_all_events_and_one_fed_in_pieces_have_the_same_groups() {
    let events = [
        tool_call(1, COMPLETED),
        thought_chunk(2, "a"),
        tool_call(3, COMPLETED),
        chunk(4, "t1", "m1", "x"),
        tool_call(5, COMPLETED),
        tool_call(6, COMPLETED),
    ];
    let whole = fold(&events);
    let mut pieces = Folder::default();
    for chunk in events.chunks(2) {
        for event in chunk {
            pieces.apply(event);
        }
        // Rendering in between changes nothing.
        let _ = render_all(&pieces);
    }

    assert_eq!(groups_of(&whole), groups_of(&pieces));
    assert_eq!(render_all(&whole), render_all(&pieces));
}

#[test]
fn a_change_of_a_member_reports_the_item_and_keeps_its_group() {
    let mut folder = fold(&[
        tool_call(1, "TOOL_CALL_STATUS_IN_PROGRESS"),
        tool_call(2, COMPLETED),
    ]);
    let changes = folder.apply(&stored(
        3,
        "t1",
        json!({"toolCallUpdate": {"toolCallId": "c1", "status": "TOOL_CALL_STATUS_FAILED"}}),
    ));

    assert_eq!(
        changes,
        [Change {
            index: 1,
            added: false
        }]
    );
    assert_eq!(folder.group_members(1).len(), 2);
    assert!(render_all(&folder).contains("1 failed"));
}

fn card_markup(id: &str) -> Markup {
    html! { div id=(id) {} }
}

#[test]
fn a_card_raised_inside_a_streak_follows_the_group() {
    let mut events: Vec<StoredEvent> = (1..=9).map(|id| tool_call(id, COMPLETED)).collect();
    events.push(chunk(10, "t1", "m1", "after"));
    let folder = fold(&events);
    // Raised between the 4th and 5th call (events 4 and 5 are minutes 4 and 5).
    let card = (at(4) + SignedDuration::from_secs(30), card_markup("card-1"));

    let html = render_transcript(folder.items(), std::slice::from_ref(&card)).into_string();

    let message = html.find("id=\"msg-r1-m1\"").unwrap();
    let card_at = html.find("id=\"card-1\"").expect("card");
    assert!(html.find("9 tool calls").unwrap() < card_at, "{html}");
    assert!(
        card_at < message,
        "the card goes before the message that follows: {html}"
    );
    assert!(
        html.find("id=\"ev-9\"").unwrap() < card_at,
        "all nine calls are above it: {html}"
    );
}

#[test]
fn a_card_before_a_streak_goes_before_its_group() {
    let folder = fold(&[tool_call(5, COMPLETED), tool_call(6, COMPLETED)]);
    let card = (at(1), card_markup("card-1"));

    let html = render_transcript(folder.items(), &[card]).into_string();

    assert!(
        html.find("id=\"card-1\"").unwrap() < html.find("id=\"grp-ev-5\"").unwrap(),
        "{html}"
    );
}

#[test]
fn the_live_anchor_of_a_card_is_the_element_after_the_group_it_falls_into() {
    let mut events: Vec<StoredEvent> = (1..=3).map(|id| tool_call(id, COMPLETED)).collect();
    events.push(chunk(4, "t1", "m1", "after"));
    let folder = fold(&events);
    let inside = at(2) + SignedDuration::from_secs(30);

    assert_eq!(folder.card_anchor(inside).as_deref(), Some("msg-r1-m1"));
    // Before the first member the card goes before the group, not into it.
    assert_eq!(folder.card_anchor(at(1)).as_deref(), Some("grp-ev-1"));
    // Behind everything it goes to the end.
    assert_eq!(folder.card_anchor(at(30)), None);
    // A card inside a group that ends the transcript also goes to the end.
    let open = fold(&[tool_call(1, COMPLETED), tool_call(2, COMPLETED)]);
    assert_eq!(
        open.card_anchor(at(1) + SignedDuration::from_secs(30)),
        None
    );
}

#[test]
fn a_thought_is_a_collapsed_row_with_a_preview_of_its_first_line() {
    let folder = fold(&[thought_chunk(
        1,
        "\nCheck the failing test first\nthen more",
    )]);
    let html = render_item(folder.items().last().unwrap()).into_string();

    assert!(
        html.contains("data-tool") && html.contains("Thinking"),
        "{html}"
    );
    assert!(html.contains(&c_icon(Icon::Brain)), "{html}");
    let summary = &html[..html.find("</summary>").unwrap()];
    assert!(
        summary.contains("Check the failing test first") && !summary.contains("then more"),
        "{html}"
    );
    assert!(!html.contains(" open"), "{html}");
}

#[test]
fn a_thought_is_plain_text_while_streaming_and_highlighted_markdown_after_the_turn_ends() {
    let mut folder = fold(&[thought_chunk(1, "```rust\nlet x = 1;\n```")]);
    let streaming = render_item(folder.items().last().unwrap()).into_string();
    assert!(
        !streaming.contains("hl-") && streaming.contains("```rust"),
        "{streaming}"
    );

    let changes = folder.apply(&turn_end(2));
    assert_eq!(
        changes,
        [Change {
            index: 1,
            added: false
        }]
    );
    let done = render_item(folder.items().last().unwrap()).into_string();
    let body = &done[done.find("</summary>").unwrap()..];
    assert!(
        body.contains("hl-keyword") && !body.contains("```"),
        "{done}"
    );
}

fn finished_tool(input: &str, output: &str, status: &str) -> String {
    let call = stored(
        1,
        "t1",
        json!({"toolCall": {"toolCallId": "c1", "title": "Run", "kind": "TOOL_KIND_EXECUTE",
            "status": status, "rawInputJson": input,
            "content": [{"content": {"text": {"text": output}}}]}}),
    );
    render_all(&fold(&[call]))
}

#[test]
fn console_output_of_a_finished_call_is_a_highlighted_block_without_fences() {
    let html = finished_tool("{}", "```console\n$ cargo test\n```", COMPLETED);

    let output = &html[html
        .find(">Output<")
        .or_else(|| html.find("Output"))
        .expect("output label")..];
    assert!(
        output.contains("hl-code") && output.contains("cargo test"),
        "{output}"
    );
    assert!(!output.contains("```"), "{output}");
}

#[test]
fn output_of_a_running_call_stays_plain_text() {
    let html = finished_tool(
        "{}",
        "```console\n$ cargo test\n```",
        "TOOL_CALL_STATUS_IN_PROGRESS",
    );

    assert!(html.contains("```console"), "{html}");
    assert!(!html.contains("<p>"), "{html}");
}

#[test]
fn script_in_the_output_of_a_finished_call_is_shown_literally() {
    let html = finished_tool("{}", "<script>alert(1)</script>", COMPLETED);

    assert!(!html.contains("<script>"), "{html}");
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{html}"
    );
}

#[test]
fn a_command_input_starts_with_a_shell_line_followed_by_indented_json() {
    let html = finished_tool(
        r#"{"command":"cargo test -p df-web","timeout":120000}"#,
        "ok",
        COMPLETED,
    );

    let input = &html[html.find("Input").unwrap()..html.find("Output").unwrap()];
    let shell = input.find("hl-shell").expect("shell block");
    let json = input.find("hl-json").expect("json block");
    assert!(shell < json, "{input}");
    assert!(
        input.contains("cargo test -p df-web") && input.contains("120000"),
        "{input}"
    );
    // Pretty-printed: each key on its own line.
    assert!(input.contains("\n  <span"), "{input}");
    // Two blocks: the shell line and the JSON each have their own `pre`.
    assert_eq!(
        input.matches("<pre class=\"hl-code\">").count(),
        2,
        "{input}"
    );
}

#[test]
fn an_input_without_a_command_is_one_json_block() {
    let html = finished_tool(r#"{"path":"a.rs"}"#, "ok", COMPLETED);

    let input = &html[html.find("Input").unwrap()..html.find("Output").unwrap()];
    assert_eq!(
        input.matches("<pre class=\"hl-code\">").count(),
        1,
        "{input}"
    );
    assert!(!input.contains("hl-shell"), "{input}");
}

#[test]
fn an_input_that_is_not_json_is_plain_escaped_text() {
    let html = finished_tool("not <json>", "ok", COMPLETED);

    assert!(html.contains("not &lt;json&gt;"), "{html}");
    let input = &html[html.find("Input").unwrap()..html.find("Output").unwrap()];
    assert!(!input.contains("<span class=\"hl-"), "{input}");
}

#[test]
fn a_diff_is_highlighted_as_a_diff_and_the_raw_result_as_json() {
    let folder = fold(&every_kind()[..7]);
    let html = render_all(&folder);

    let diff = &html[html.find(">Diff<").or_else(|| html.find("Diff")).unwrap()..];
    assert!(diff.contains("hl-diff") && diff.contains("a.rs"), "{diff}");
    let result = &html[html.find("Result").unwrap()..];
    assert!(result.contains("hl-json"), "{result}");
}

fn webhook(id: i64, body: Value) -> StoredEvent {
    forge(id, json!({"webhookReceived": body}))
}

fn webhook_html(body: Value) -> String {
    forge_row(&webhook(1, body))
}

#[test]
fn a_review_webhook_row_names_the_actor_the_verdict_and_links_the_pull_request() {
    let html = webhook_html(json!({"reviewSubmitted": {"number": 41, "actor": "alice",
        "verdict": "WEBHOOK_REVIEW_VERDICT_CHANGES_REQUESTED", "url": "https://forge.example/pr/41"}}));

    assert!(
        html.contains(&c_icon(Icon::Webhook)) && html.contains("data-system"),
        "{html}"
    );
    assert!(html.contains("alice requested changes on PR <a "), "{html}");
    let link = tag_around(&html, "href=\"https://forge.example/pr/41\"");
    assert!(
        link.contains("underline") && link.contains("text-accent-text"),
        "{link}"
    );
    assert!(html.contains(">#41</a>"), "{html}");
    // The wake-up text is not shown as a message.
    assert!(!html.contains("Forge event on your task"), "{html}");
}

#[test]
fn webhook_rows_read_like_the_board_for_every_kind() {
    let comment = webhook_html(
        json!({"commentCreated": {"number": 41, "isPullRequest": true,
        "actor": "bob", "url": "https://forge.example/pr/41"}}),
    );
    assert!(comment.contains("bob commented on PR <a "), "{comment}");

    let ci = webhook_html(
        json!({"ciFailed": {"head": "refs/heads/factory/DF-12", "check": "test",
        "url": "https://ci.example/run/7"}}),
    );
    assert!(ci.contains("CI check <a "), "{ci}");
    assert!(ci.contains(">test</a> failed on factory/DF-12"), "{ci}");

    let closed = webhook_html(json!({"pullRequestClosed": {"number": 41, "actor": "alice",
        "url": "https://forge.example/pr/41"}}));
    assert!(
        closed.contains("alice closed PR <a ") && closed.contains("</a> without merging"),
        "{closed}"
    );

    let approved = webhook_html(json!({"reviewSubmitted": {"number": 41, "actor": "alice",
        "verdict": "WEBHOOK_REVIEW_VERDICT_APPROVED"}}));
    assert!(approved.contains("alice approved PR #41"), "{approved}");
}

#[test]
fn a_webhook_row_with_an_unsafe_url_shows_the_text_without_a_link() {
    let html = webhook_html(
        json!({"ciFailed": {"head": "factory/DF-12", "check": "test",
        "url": "javascript:alert(1)"}}),
    );

    assert!(
        !html.contains("<a ") && !html.contains("javascript"),
        "{html}"
    );
    assert!(
        html.contains("CI check test failed on factory/DF-12"),
        "{html}"
    );
}

#[test]
fn a_webhook_event_without_a_kind_adds_no_row() {
    let folder = fold(&[webhook(1, json!({}))]);

    assert!(folder.items().is_empty(), "{:?}", folder.items());
}

#[test]
fn the_webhook_summary_is_one_sentence_for_the_queued_stack() {
    let summary = |body: Value| {
        let event: WebhookReceived = serde_json::from_value(body).unwrap();
        webhook::summary(&event).unwrap().text()
    };

    assert_eq!(
        summary(json!({"reviewSubmitted": {"number": 12, "actor": "alice",
            "verdict": "WEBHOOK_REVIEW_VERDICT_CHANGES_REQUESTED"}})),
        "alice requested changes on PR #12"
    );
    assert_eq!(
        summary(json!({"commentCreated": {"number": 7, "actor": "bob"}})),
        "bob commented on issue #7"
    );
}
