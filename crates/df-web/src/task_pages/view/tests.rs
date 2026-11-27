//! Tests of the task workspace view.

use super::*;
use df_core::Principal;
use df_core::mcp::McpCallOutcome;
use df_db::mcp_servers::McpCallRecord;
use df_db::task_views::TaskListItem;

fn list_item(key: &str) -> TaskListItem {
    let at: Timestamp = "2026-10-03T04:05:06Z".parse().unwrap();
    TaskListItem {
        id: uuid::Uuid::nil(),
        key: key.parse().unwrap(),
        title: "Fix login".to_owned(),
        title_source: TitleSource::User,
        project_key: "DF".to_owned(),
        project_name: "Dark Factory".to_owned(),
        persona_id: uuid::Uuid::from_u128(0x0192_a000_0000_7000_8000_0000_0000_0001),
        persona_handle: "linus".to_owned(),
        persona_display_name: "Linus".to_owned(),
        status: "queued".to_owned(),
        status_line: String::new(),
        resume_at: None,
        subtasks: df_db::task_views::SubtaskProgress { done: 0, total: 0 },
        has_subtasks: false,
        needs_you: false,
        needs_you_below: 0,
        created_at: at,
        updated_at: at,
    }
}

fn key(key: &str) -> TaskKey {
    key.parse().unwrap()
}

fn rows_of(items: Vec<TaskListItem>, filters: &Filters) -> String {
    task_rows(&TaskList::of(items), None, filters).into_string()
}

fn item_in(key: &str, status: &str) -> TaskListItem {
    TaskListItem {
        status: status.to_owned(),
        ..list_item(key)
    }
}

#[test]
fn a_running_task_shows_no_badge_and_a_completed_one_a_struck_through_title() {
    let filters = Filters::default();

    let running = rows_of(vec![item_in("DF-14", "running")], &filters);
    let done = rows_of(vec![item_in("DF-10", "completed")], &filters);

    assert!(!running.contains("rounded-full px-2"), "{running}");
    assert!(!done.contains("rounded-full px-2"), "{done}");
    assert!(
        done.contains("line-through") && !running.contains("line-through"),
        "{done}"
    );
}

#[test]
fn a_task_that_needs_you_shows_the_dot_in_place_of_its_badge() {
    let waiting = TaskListItem {
        needs_you: true,
        ..item_in("DF-14", "waiting")
    };

    let html = rows_of(vec![waiting], &Filters::default());

    assert!(html.contains("aria-label=\"Needs you\""), "{html}");
    assert!(!html.contains(">waiting<"), "{html}");
}

#[test]
fn the_other_statuses_keep_their_badge() {
    for status in ["failed", "cancelled", "queued", "rate_limited"] {
        let html = rows_of(vec![item_in("DF-8", status)], &Filters::default());
        assert!(
            html.contains(&format!(">{}<", status.replace('_', " "))),
            "{status}: {html}"
        );
    }
}

#[test]
fn a_row_with_tasks_below_that_need_you_shows_the_dot_and_counts_them() {
    let one = TaskListItem {
        needs_you_below: 1,
        ..item_in("DF-12", "running")
    };
    let two = TaskListItem {
        needs_you_below: 2,
        ..item_in("DF-12", "running")
    };

    let one = rows_of(vec![one], &Filters::default());
    let two = rows_of(vec![two], &Filters::default());

    assert!(one.contains("aria-label=\"1 subtask needs you\""), "{one}");
    assert!(one.contains(">1 subtask needs you<"), "{one}");
    assert!(two.contains(">2 subtasks need you<"), "{two}");
    assert!(
        one.find("role=\"img\"").unwrap() < one.find(">DF-12<").unwrap(),
        "the dot comes before the key: {one}"
    );
    assert!(!one.contains(">running<"), "{one}");
}

#[test]
fn a_failed_row_with_tasks_below_that_need_you_shows_the_badge_and_the_dot() {
    let failed = TaskListItem {
        needs_you_below: 2,
        ..item_in("DF-8", "failed")
    };

    let html = rows_of(vec![failed], &Filters::default());

    assert!(
        html.contains(">failed<") && html.contains("role=\"img\""),
        "{html}"
    );
    assert!(html.contains(">2 subtasks need you<"), "{html}");
}

#[test]
fn a_resume_time_wins_over_the_needs_you_text() {
    let limited = TaskListItem {
        needs_you_below: 1,
        resume_at: Some("2099-01-01T00:00:00Z".parse().unwrap()),
        ..item_in("DF-9", "rate_limited")
    };

    let html = rows_of(vec![limited], &Filters::default());

    assert!(html.contains("Resumes at"), "{html}");
    assert!(!html.contains(">1 subtask needs you<"), "{html}");
}

#[test]
fn a_top_level_row_with_subtasks_has_a_disclosure_link_named_for_its_state() {
    let parent = TaskListItem {
        subtasks: df_db::task_views::SubtaskProgress { done: 1, total: 2 },
        has_subtasks: true,
        ..item_in("DF-12", "running")
    };
    let none = item_in("DF-14", "running");
    let filters = Filters::default();

    let closed = rows_of(vec![parent.clone(), none], &filters);
    let mut list = TaskList::of(vec![parent]);
    list.subtasks
        .insert(key("DF-12"), vec![item_in("DF-13", "completed")]);
    let open = task_rows(&list, None, &Filters::default().toggled(&key("DF-12"))).into_string();

    let control = crate::testkit::tag_around(&closed, "aria-label=\"Show subtasks of DF-12\"");
    assert!(
        control.contains("aria-expanded=\"false\"") && control.contains("href=\"?open=DF-12\""),
        "{control}"
    );
    assert!(!closed.contains("subtasks of DF-14"), "{closed}");
    let control = crate::testkit::tag_around(&open, "aria-label=\"Hide subtasks of DF-12\"");
    assert!(
        control.contains("aria-expanded=\"true\"") && control.contains("href=\"?\""),
        "{control}"
    );
    assert!(open.contains("row-DF-13"), "{open}");
}

#[test]
fn a_row_whose_subtasks_are_all_cancelled_has_a_disclosure_link_but_no_chip() {
    let parent = TaskListItem {
        has_subtasks: true,
        ..item_in("DF-12", "running")
    };

    let html = rows_of(vec![parent], &Filters::default());

    assert!(
        html.contains("aria-label=\"Show subtasks of DF-12\""),
        "{html}"
    );
    assert!(!html.contains("data-subtasks"), "{html}");
}

#[test]
fn a_running_row_without_tasks_below_that_need_you_shows_no_dot_and_its_own_status_line() {
    let running = TaskListItem {
        status_line: "Running cargo test".to_owned(),
        ..item_in("DF-14", "running")
    };

    let html = rows_of(vec![running], &Filters::default());

    assert!(
        !html.contains("role=\"img\" aria-label=\"Needs you\""),
        "{html}"
    );
    assert!(!html.contains("subtask"), "{html}");
    assert!(html.contains(">Running cargo test<"), "{html}");
}

#[test]
fn a_listed_subtask_with_subtasks_has_a_scope_link() {
    let parent = TaskListItem {
        has_subtasks: true,
        ..item_in("DF-12", "running")
    };
    let subtask = TaskListItem {
        has_subtasks: true,
        ..item_in("DF-17", "running")
    };
    let mut list = TaskList::of(vec![parent]);
    list.subtasks.insert(key("DF-12"), vec![subtask]);
    let filters = Filters::default().toggled(&key("DF-12"));

    let html = task_rows(&list, None, &filters).into_string();

    let control = crate::testkit::tag_around(&html, "aria-label=\"Open subtasks of DF-17\"");
    assert!(
        control.contains("href=\"?parent=DF-17\"") && !control.contains("aria-expanded"),
        "{control}"
    );
    // The scoped list shows DF-17 on top without a control: focus goes to its row link.
    assert!(
        control.contains("data-focus-after-swap=\"#row-DF-17 &gt; a\""),
        "{control}"
    );
    // A disclosure control stays in its row: focus comes back to it.
    let disclosure = crate::testkit::tag_around(&html, "aria-label=\"Hide subtasks of DF-12\"");
    assert!(
        disclosure.contains("data-focus-after-swap=\"#control-DF-12\""),
        "{disclosure}"
    );
}

#[test]
fn the_chevrons_are_centred_on_the_key_line_and_leave_a_gap_before_the_key() {
    let parent = TaskListItem {
        has_subtasks: true,
        ..item_in("DF-12", "running")
    };
    let subtask = TaskListItem {
        has_subtasks: true,
        ..item_in("DF-17", "running")
    };
    let mut list = TaskList::of(vec![parent]);
    list.subtasks.insert(key("DF-12"), vec![subtask]);
    let filters = Filters::default().toggled(&key("DF-12"));

    let html = task_rows(&list, None, &filters).into_string();

    // The key line is 20 px high below 14 px (top level) or 12 px (subtask) of padding; the 44 px
    // control is moved down by the difference. The 16 px chevron ends 8 px before the key.
    let disclosure = crate::testkit::tag_around(&html, "aria-label=\"Hide subtasks of DF-12\"");
    assert!(
        disclosure.contains("left-0") && disclosure.contains("top-0.5"),
        "{disclosure}"
    );
    let top_link = crate::testkit::tag_around(crate::testkit::row(&html, "DF-12"), "hx-push-url");
    assert!(top_link.contains("py-3.5 pl-[35px]"), "{top_link}");
    let scope = crate::testkit::tag_around(&html, "aria-label=\"Open subtasks of DF-17\"");
    assert!(
        scope.contains("left-5") && scope.contains("top-0 "),
        "{scope}"
    );
    let sub_link = crate::testkit::tag_around(crate::testkit::row(&html, "DF-17"), "hx-push-url");
    assert!(sub_link.contains("py-3 pl-[55px]"), "{sub_link}");
}

#[test]
fn open_and_parent_keys_are_deduplicated_and_malformed_ones_ignored() {
    let f = Filters::new(None, None, None).with_tree(Some("DF-12,DF-12,x y"), Some("DF-17"));

    assert_eq!(f.open, [key("DF-12")]);
    assert_eq!(f.query(), "?open=DF-12&parent=DF-17");
    assert_eq!(
        Filters::new(None, None, None)
            .with_tree(Some("a b"), Some("no key"))
            .query(),
        ""
    );
}

#[test]
fn the_chips_keep_the_open_rows() {
    let f = Filters::new(None, None, None).with_tree(Some("DF-12"), None);

    assert_eq!(
        f.with_status(StatusFilter::Done).query(),
        "?status=done&open=DF-12"
    );
}

#[test]
fn a_task_row_shows_the_persona_avatar_labelled_with_its_name_before_the_time() {
    let filters = Filters::new(None, None, None);

    let html = rows_of(vec![list_item("DF-21")], &filters);

    let avatar = crate::testkit::tag_around(&html, "aria-label=\"Linus\"");
    assert!(
        avatar.contains("role=\"img\"") && avatar.contains("size-5"),
        "{avatar}"
    );
    assert!(
        html.find("aria-label=\"Linus\"").unwrap() < html.find("<time ").unwrap(),
        "{html}"
    );
    assert!(html.contains(">L<"), "{html}");
}

#[test]
fn the_persona_in_the_details_shows_avatar_name_and_handle_and_links_to_the_list() {
    let task = crate::testkit::task_detail("DF-21");

    let html = persona_value(&task).into_string();

    assert!(html.starts_with("<a href=\"/personas\""), "{html}");
    assert!(
        html.contains("role=\"img\"") && html.contains(">L<") && html.contains("size-5"),
        "{html}"
    );
    assert!(
        html.contains("Linus <span class=\"font-normal text-muted\">@linus</span>"),
        "{html}"
    );
    assert!(!html.contains("Claude Code"), "{html}");
}

#[test]
fn the_full_time_shows_date_and_seconds_in_utc() {
    let at: Timestamp = "2026-10-03T04:05:06Z".parse().unwrap();
    assert_eq!(format_time(at), "2026-10-03 04:05:06 UTC");
}

#[test]
fn filters_parse_and_serialize_without_defaults() {
    assert_eq!(Filters::new(None, None, None).query(), "");
    assert_eq!(
        Filters::new(Some("nonsense"), Some(""), Some("bogus")).query(),
        ""
    );
    let f = Filters::new(Some("done"), Some("WEB"), Some("created"));
    assert_eq!(f.query(), "?status=done&project=WEB&sort=created");
    assert_eq!(f.status.group(), Some(StatusSet::Done));
    assert_eq!(f.to_db().sort, TaskSort::Created);
    assert_eq!(Filters::new(None, None, Some("updated")).query(), "");
    assert_eq!(
        Filters::new(None, None, Some("updated-asc")).query(),
        "?sort=updated-asc"
    );
    assert_eq!(StatusFilter::parse(Some("all")).group(), None);
    assert_eq!(
        Filters::new(None, Some("A B"), None).query(),
        "?project=A+B"
    );
}

#[test]
fn composer_remembers_the_issue_and_names_every_control() {
    let choices = Choices {
        projects: vec![("p1".into(), "DF - Dark Factory".into())],
        personas: vec![("u1".into(), "Linus".into())],
    };
    let values = TaskForm {
        issue_number: "7".into(),
        ..TaskForm::default()
    };
    let html = composer_form(&values, &FormErrors::default(), &choices).into_string();
    assert!(html.contains("&quot;issue&quot;:&quot;7&quot;"), "{html}");
    for name in [
        "Project",
        "Description",
        "Assignee",
        "Linked issue",
        "Remove linked issue",
    ] {
        assert!(html.contains(name), "{name}");
    }
    assert!(
        html.contains("x-on:keydown.ctrl.enter.prevent")
            && html.contains("x-on:keydown.meta.enter.prevent")
    );
}

#[test]
fn a_run_detail_needs_a_disclosure_only_beyond_one_short_line() {
    let long_line = "x".repeat(120);
    let long = run_detail(&long_line).into_string();
    assert!(
        long.contains("<details") && long.contains(&long_line),
        "{long}"
    );
    let at_limit = "y".repeat(DETAIL_LINE_CHARS);
    assert!(!run_detail(&at_limit).into_string().contains("<details"));
    let over = "z".repeat(DETAIL_LINE_CHARS + 1);
    assert!(run_detail(&over).into_string().contains("<details"));
    let multi = run_detail("short\nsecond").into_string();
    assert!(
        multi.contains("<summary class=\"cursor-pointer truncate\">short</summary>"),
        "{multi}"
    );
}

fn run(number: i64) -> RunView {
    RunView {
        id: uuid::Uuid::now_v7(),
        number,
        state: RunState::Succeeded,
        outcome: None,
        detail: String::new(),
        retry_after: None,
        started_at: Timestamp::UNIX_EPOCH,
        ended_at: None,
        usage: vec![],
        unknown_usage_requests: 0,
        mcp_calls: vec![],
    }
}

#[test]
fn queued_stack_shows_readable_system_authors_without_edit_actions() {
    for (reason, prefix) in [
        (df_core::system_reason::TASK_ASSIGNED, "Task assignment: "),
        ("unknown.reason", "Dark Factory: "),
    ] {
        let mut messages = queued(1);
        messages[0].author = Principal::system(reason);

        let html = pending_region("DF-1", "Linus", "queued", &messages, false, None).into_string();
        assert!(html.contains(prefix), "{html}");
        assert!(html.contains("1 queued"), "{html}");
        assert!(!html.contains(reason), "{html}");
        assert!(!html.contains("Edit queued message"), "{html}");
        assert!(!html.contains("Remove queued message"), "{html}");
    }
}

#[test]
fn queued_stack_prefixes_no_human_author() {
    let html = pending_region("DF-1", "Linus", "queued", &queued(1), false, None).into_string();
    assert!(html.contains("message 0"), "{html}");
    assert!(!html.contains("Peter"), "{html}");
    assert!(!html.contains(": </span>"), "{html}");
}

#[test]
fn queued_stack_prefixes_agent_authors_with_the_persona_display_name_without_edit_actions() {
    let mut messages = queued(1);
    messages[0].author = Principal::agent(
        df_core::PersonaId::new(),
        "Linus",
        None,
        messages[0].task_id,
    );

    let html = pending_region("DF-1", "Reviewer", "queued", &messages, false, None).into_string();
    assert!(
        html.contains("<span class=\"text-xs text-faint\">Linus: </span>"),
        "{html}"
    );
    assert!(html.contains("1 queued"), "{html}");
    assert!(!html.contains("Edit queued message"), "{html}");
    assert!(!html.contains("Remove queued message"), "{html}");
}

fn queued(n: usize) -> Vec<TaskMessageRow> {
    (0..n)
        .map(|i| TaskMessageRow {
            id: df_core::PromptId::new(),
            task_id: df_core::TaskId::new(),
            author: Principal::human(df_core::UserId::new(), "Peter"),
            content: vec![df_proto::transcript::v1::ContentBlock {
                kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                    df_proto::transcript::v1::TextContent {
                        text: format!("message {i}"),
                    },
                )),
                hidden: false,
            }],
            queued_at: Timestamp::UNIX_EPOCH,
            delivered_run_id: None,
            delivered_at: None,
            forwarded_at: None,
            forge_event: None,
        })
        .collect()
}

#[test]
fn runs_are_closed_by_default_with_count_and_keep_the_given_order() {
    let html = runs_region("DF-1", &[run(3), run(2), run(1)], None).into_string();
    assert!(
        html.contains("<details") && !html.contains("<details open"),
        "{html}"
    );
    assert!(html.contains("<summary"), "{html}");
    assert!(html.contains(">Runs<") && html.contains(">3<"), "{html}");
    assert!(
        html.find("#3").unwrap() < html.find("#1").unwrap(),
        "{html}"
    );
    assert!(
        !runs_region("DF-1", &[], None)
            .into_string()
            .contains("<details")
    );
    // The list is the scrolling element of the runs area from `md`.
    let ul = &html[html.find("<ul").unwrap()..];
    let ul = &ul[..ul.find('>').unwrap()];
    assert!(
        ul.contains("md:overflow-y-auto") && ul.contains("md:min-h-0"),
        "{ul}"
    );
}

#[test]
fn run_cards_keep_their_id_when_the_order_changes() {
    let runs: Vec<RunView> = (1..=4).map(run).collect();
    let by_number = |n: i64| runs.iter().find(|r| r.number == n).unwrap();
    for order in [[3, 2, 1].as_slice(), [4, 3, 2, 1].as_slice()] {
        let list: Vec<RunView> = order.iter().map(|n| by_number(*n).clone()).collect();
        let html = runs_region("DF-1", &list, None).into_string();
        for r in &list {
            let id = format!("id=\"run-card-{}\"", r.id);
            assert_eq!(html.matches(&id).count(), 1, "{html}");
            // The id belongs to the card of its own run.
            let card = &html[html.find(&id).unwrap()..];
            let card = &card[..card.find("</li>").unwrap()];
            assert!(card.contains(&format!(">#{}<", r.number)), "{card}");
        }
    }
}

#[test]
fn queued_stack_has_a_header_and_a_list_capped_at_three_rows() {
    let seven = queued(7);
    let html = pending_region("DF-1", "Linus", "running", &seven, false, None).into_string();
    assert!(
        html.contains("7 queued · Linus reads them at the next turn"),
        "{html}"
    );
    assert!(
        html.contains("max-h-[calc(3*2.75rem+2*0.25rem)]")
            && html.contains("lg:max-h-[calc(3*2.25rem+2*0.25rem)]")
            && html.contains("overflow-y-auto"),
        "{html}"
    );
    let ended = pending_region("DF-1", "Linus", "failed", &queued(1), false, None).into_string();
    assert!(
        ended.contains("1 queued · sent with the next run"),
        "{ended}"
    );
    let none = pending_region("DF-1", "Linus", "running", &[], false, None).into_string();
    assert!(!none.contains("queued"), "{none}");
}

#[test]
fn queued_stack_shows_hidden_text_only_with_the_option() {
    use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
    let block = |text: &str, hidden| ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: text.into() })),
        hidden,
    };
    let mut message = queued(1).remove(0);
    message.content = vec![block("visible text", false), block("secret text", true)];
    let off =
        pending_region("DF-1", "Linus", "running", &[message.clone()], false, None).into_string();
    assert!(
        off.contains("visible text") && !off.contains("secret text"),
        "{off}"
    );
    let on = pending_region("DF-1", "Linus", "running", &[message], true, None).into_string();
    assert!(on.contains("secret text"), "{on}");
    // The hidden part is labelled and muted, as in the transcript.
    assert!(
        on.contains("data-hidden") && on.contains("Instruction for the agent"),
        "{on}"
    );
    assert!(
        !off.contains("data-hidden") && !off.contains("Instruction"),
        "{off}"
    );
}

#[test]
fn queued_stack_skips_messages_with_only_hidden_blocks_unless_the_option_is_on() {
    use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
    let mut hidden_only = queued(1).remove(0);
    hidden_only.content = vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent {
            text: "secret text".into(),
        })),
        hidden: true,
    }];
    let shown = queued(1).remove(0);
    let both = [hidden_only, shown];
    let off = pending_region("DF-1", "Linus", "running", &both, false, None).into_string();
    assert!(
        off.contains("1 queued") && !off.contains("secret text"),
        "{off}"
    );
    assert_eq!(off.matches("data-queued-id").count(), 1, "{off}");
    let on = pending_region("DF-1", "Linus", "running", &both, true, None).into_string();
    assert!(on.contains("2 queued"), "{on}");
    assert_eq!(on.matches("data-queued-id").count(), 2, "{on}");
    let only = pending_region("DF-1", "Linus", "running", &both[..1], false, None).into_string();
    assert!(
        !only.contains("queued") && !only.contains("data-queued-id"),
        "{only}"
    );
}

#[test]
fn counts_are_compact_from_1000() {
    let shown: Vec<String> = [
        0, 999, 1000, 9437, 9999, 10_000, 182_345, 999_999, 1_000_000, 1_234_567, 12_000_000,
    ]
    .into_iter()
    .map(compact_count)
    .collect();
    assert_eq!(
        shown,
        [
            "0", "999", "1.0k", "9.4k", "9.9k", "10k", "182k", "999k", "1.0M", "1.2M", "12M"
        ]
    );
}

#[test]
fn requests_without_usage_are_counted_below_the_token_table() {
    let mut r = run(3);
    r.unknown_usage_requests = 2;
    let html = run_card("DF-1", &r, None).into_string();
    assert!(html.contains("2 requests without usage"), "{html}");
    assert!(
        !html.contains("no token usage recorded"),
        "the run did make requests: {html}"
    );
    r.unknown_usage_requests = 1;
    assert!(
        run_card("DF-1", &r, None)
            .into_string()
            .contains("1 request without usage")
    );
    r.unknown_usage_requests = 0;
    assert!(
        !run_card("DF-1", &r, None)
            .into_string()
            .contains("without usage")
    );
}

#[test]
fn run_card_links_to_the_divider_and_lists_usage_in_three_columns() {
    let mut r = run(3);
    r.usage = vec![df_db::task_views::ModelUsage {
        model_id: "anthropic/claude-sonnet-5-5-20261001".into(),
        input_tokens: 9437,
        output_tokens: 182_345,
        cache_read_tokens: 1_234_567,
        cache_write_tokens: 12,
    }];
    let html = run_card("DF-1", &r, None).into_string();
    assert!(html.contains("<a href=\"#run-3\""), "{html}");
    assert!(
        html.contains("details = false"),
        "closes the overlaying panel: {html}"
    );
    // The model on its own line, truncated, with the full name as title.
    assert!(
        html.contains("class=\"truncate")
            && html.contains("title=\"anthropic/claude-sonnet-5-5-20261001\""),
        "{html}"
    );
    // The model comes before the column header, not between header and numbers.
    let model = html
        .find(">anthropic/claude-sonnet-5-5-20261001<")
        .expect("model");
    let header = html.find(">in<").expect("header");
    assert!(model < header, "{html}");
    for cell in [
        "title=\"9437\">9.4k<",
        "title=\"182345\">182k<",
        "title=\"1234567 / 12\">1.2M / 12<",
    ] {
        assert!(html.contains(cell), "{cell}: {html}");
    }
}

/// The stream replaces `#composer-action` on every task refresh, and Alpine drops an
/// `x-ref` inside it with the old element. The shortcut therefore looks up the Send button
/// when the key is pressed.
#[test]
fn the_send_shortcut_finds_the_button_the_stream_swapped_in() {
    let area = message_area("DF-1", "Linus", TaskStatus::Running, &[], &[], false).into_string();
    let action = composer_action("DF-1", true).into_string();
    assert!(
        !area.contains("$refs.send") && !action.contains("x-ref"),
        "{area}"
    );
    let textarea = crate::testkit::tag_around(&area, "id=\"message\"");
    for key in ["ctrl", "meta"] {
        assert!(
            textarea.contains(&format!("x-on:keydown.{key}.enter.prevent"))
                && textarea.contains("#composer-action [aria-label=Send]"),
            "{textarea}"
        );
    }
}

/// A second Ctrl+Enter while the first send is in flight is dropped, not queued behind it:
/// the draft is only cleared by the first response, so a queued send would repeat it. The
/// queue lives on the message box, because the stream replaces the Send button (sending
/// requeues the task) while the request is still in flight.
#[test]
fn a_second_send_while_one_is_in_flight_is_dropped() {
    let area = message_area("DF-1", "Linus", TaskStatus::Waiting, &[], &[], false).into_string();
    assert!(area.contains("id=\"message\""), "{area}");
    for interruptible in [false, true] {
        let action = composer_action("DF-1", interruptible).into_string();
        let send = crate::testkit::tag_around(&action, "aria-label=\"Send\"");
        assert!(send.contains("hx-sync=\"#message:drop\""), "{send}");
    }
}

#[test]
fn send_save_and_create_use_the_round_action_classes() {
    let area = message_area("DF-1", "Linus", TaskStatus::Queued, &[], &[], false).into_string();
    assert!(
        area.contains(&format!("class=\"{ROUND_ACTION_CLASS}\"")),
        "{area}"
    );
    assert!(
        area.contains(&format!("class=\"{ROUND_PILL_CLASS}\"")),
        "{area}"
    );
    let choices = Choices {
        projects: vec![("p".into(), "DF".into())],
        personas: vec![("u".into(), "Linus".into())],
    };
    let composer =
        composer_form(&TaskForm::default(), &FormErrors::default(), &choices).into_string();
    assert!(
        composer.contains(&format!("class=\"{ROUND_ACTION_CLASS}\"")),
        "{composer}"
    );
}

#[test]
fn composer_without_projects_says_what_to_do() {
    let none = Choices {
        projects: vec![],
        personas: vec![],
    };
    let html = composer_dialog(&TaskForm::default(), &FormErrors::default(), &none).into_string();
    assert!(html.contains("Create a project and a persona first."));
    assert!(!html.contains("<form id=\"composer-form\""));
}

#[test]
fn composer_keeps_the_branch_as_a_json_string_and_shows_the_error_outside_the_popover() {
    let choices = Choices {
        projects: vec![("p1".into(), "DF - Dark Factory".into())],
        personas: vec![("u1".into(), "Linus".into())],
    };
    let values = TaskForm {
        branch: " renovate/\"x\" ".into(),
        ..TaskForm::default()
    };
    let mut errors = FormErrors::default();
    errors.add("branch", "renovate/x is held by DF-13.");

    let html = composer_form(&values, &errors, &choices).into_string();

    assert!(
        html.contains("&quot;branch&quot;:&quot;renovate/\\&quot;x\\&quot;&quot;"),
        "{html}"
    );
    assert!(
        html.contains("aria-label=\"Remove existing branch\""),
        "{html}"
    );
    let error = html.find("renovate/x is held by DF-13.").unwrap();
    assert!(
        error < html.find("aria-label=\"Linked issue and branch\"").unwrap(),
        "{html}"
    );
}

#[test]
fn the_task_head_is_a_focus_target_named_by_a_screen_reader_only_heading() {
    let task = crate::testkit::task_detail("DF-21");

    let html = head_region(&task, None).into_string();

    // The frame is tested once, in `components::detail_head`.
    assert!(
        html.starts_with("<header id=\"task-head\" tabindex=\"-1\""),
        "{html}"
    );
    assert!(
        html.contains(&format!(
            "<h2 id=\"detail-title\" class=\"sr-only\">DF-21 {}</h2>",
            task.title
        )),
        "{html}"
    );
}

#[test]
fn a_task_row_selects_itself_and_hands_focus_to_the_head_after_loading() {
    let filters = Filters::new(None, None, None);

    let html = rows_of(vec![list_item("DF-21")], &filters);

    // Marked and shown (phone) once its task loaded: a failed load leaves the list, the open
    // task's row and the focus on the row as they are.
    assert!(!html.contains("x-on:click"), "{html}");
    assert!(
        html.contains("x-on:htmx:after:swap.document=\"selected($el, $event)\""),
        "{html}"
    );
    assert!(
        !html.contains("querySelectorAll") && !html.contains("dataset"),
        "{html}"
    );
    assert!(html.contains("<ul id=\"task-rows\" data-rows>"), "{html}");
    assert!(
        html.contains("data-focus-after-swap=\"#task-head\""),
        "{html}"
    );
}

fn call(
    server: &str,
    tool: &str,
    outcome: McpCallOutcome,
    minute: i64,
    millis: i64,
    sizes: (i64, i64),
) -> McpCallRecord {
    McpCallRecord {
        call_id: uuid::Uuid::now_v7(),
        run_id: uuid::Uuid::now_v7(),
        server_name: server.into(),
        tool: tool.into(),
        outcome,
        started_at: Timestamp::from_second(1_800_000_000 + minute * 60).unwrap(),
        duration_ms: millis,
        request_bytes: sizes.0,
        response_bytes: sizes.1,
    }
}

#[test]
fn a_run_with_calls_lists_them_oldest_first_with_outcome_badges_and_sizes() {
    let mut with_calls = run(8);
    with_calls.mcp_calls = vec![
        call(
            "wiki",
            "tools/list",
            McpCallOutcome::Timeout,
            0,
            15_000,
            (0, 0),
        ),
        call(
            "context7",
            "resolve-library-id",
            McpCallOutcome::Ok,
            1,
            1_200,
            (400, 3_100),
        ),
        call("wiki", "search", McpCallOutcome::Rejected, 4, 0, (200, 0)),
    ];

    let html = runs_region("DF-1", &[with_calls], None).into_string();

    assert!(html.contains("Tool calls (MCP)"), "{html}");
    let section = &html[html.find("Tool calls (MCP)").unwrap()..];
    assert!(section.contains(">3</span>"), "the count: {section}");
    let order: Vec<usize> = [
        "wiki · tools/list",
        "context7__resolve-library-id",
        "wiki__search",
    ]
    .iter()
    .map(|label| section.find(label).expect(label))
    .collect();
    assert!(order.is_sorted(), "{section}");
    assert!(
        section.contains("15.0 s · –  → –") || section.contains("15.0 s · – → –"),
        "{section}"
    );
    assert!(section.contains("1.2 s · 0.4 KB → 3.1 KB"), "{section}");
    assert!(section.contains("0 ms · 0.2 KB → –"), "{section}");
    for (badge, tone) in [
        ("timeout", "status-limited"),
        ("ok", "status-completed"),
        ("rejected", "status-failed"),
    ] {
        assert!(
            section.contains(&format!("{tone}-bg text-{tone}-fg\">{badge}</span>")),
            "{badge}: {section}"
        );
    }
}

#[test]
fn a_run_without_calls_has_no_tool_call_section() {
    let html = runs_region("DF-1", &[run(1)], None).into_string();

    assert!(!html.contains("Tool calls (MCP)"), "{html}");
}

#[test]
fn a_rejected_name_without_a_server_shows_as_the_agent_sent_it() {
    let mut with_calls = run(8);
    with_calls.mcp_calls = vec![call("", "plain", McpCallOutcome::Rejected, 1, 0, (10, 0))];

    let html = runs_region("DF-1", &[with_calls], None).into_string();

    let section = &html[html.find("Tool calls (MCP)").unwrap()..];
    assert!(section.contains("plain"), "{section}");
    assert!(!section.contains("__plain"), "{section}");
}
