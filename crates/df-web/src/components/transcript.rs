//! Items of the task transcript. The folding of events into items lives in `crate::transcript`;
//! these functions only render. Every item's outer element carries `id`, the stable target of
//! live updates.

use chrono::{DateTime, Utc};
use maud::{Markup, html};

use super::actions::ICON_BUTTON_CLASS;
use super::icons::{Icon, icon};
use super::shell::{avatar, initials};

/// Time shown under a bubble.
fn clock(at: DateTime<Utc>) -> String {
    at.format("%H:%M").to_string()
}

/// A message of a board member: a bubble on the right with name and time below it. The avatar is
/// shown for the last message of a group only (CSS, `data-author`).
pub fn user_message(
    id: &str,
    prompt_id: &str,
    author: &str,
    at: DateTime<Utc>,
    text: &str,
) -> Markup {
    html! {
        div id=(id) data-prompt-id=(prompt_id) data-author="human" class="flex items-end justify-end gap-2" {
            div class="min-w-0 max-w-[85%] md:max-w-[75%]" {
                div class="whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-bubble-me px-3 py-2 text-ink" { (text) }
                (meta(author, at, "text-right"))
            }
            (avatar(&initials(author)))
        }
    }
}

/// A message of the agent: a bubble on the left, plain text while it streams, rendered Markdown
/// (`body`) when done.
pub fn agent_message(
    id: &str,
    author: &str,
    at: DateTime<Utc>,
    body: Markup,
    markdown: bool,
) -> Markup {
    html! {
        div id=(id) data-author="agent" class="flex items-end gap-2" {
            (avatar(&initials(author)))
            div class="min-w-0 max-w-[85%] md:max-w-[75%]" {
                div class="break-words rounded-2xl rounded-bl-md bg-bubble-agent px-3 py-2 text-on-bubble-agent" {
                    @if markdown {
                        div class="md-body" { (body) }
                    } @else {
                        div class="whitespace-pre-wrap" { (body) }
                    }
                }
                (meta(author, at, ""))
            }
        }
    }
}

fn meta(author: &str, at: DateTime<Utc>, class: &str) -> Markup {
    html! {
        div class={"mt-1 text-xs text-faint" @if !class.is_empty() { " " (class) }} {
            @if !author.is_empty() { (author) " · " }
            time datetime=(at.to_rfc3339()) { (clock(at)) }
        }
    }
}

/// The agent's reasoning, collapsed.
pub fn thought(id: &str, text: &str) -> Markup {
    html! {
        details id=(id) data-author="agent" class="ml-11 text-faint" {
            summary class="flex min-h-11 cursor-pointer items-center text-sm lg:min-h-8" { "Thinking" }
            div class="whitespace-pre-wrap break-words text-sm" { (text) }
        }
    }
}

/// The start of a run, as a divider with the run's start time.
pub fn run_divider(id: &str, number: i64, started_at: DateTime<Utc>) -> Markup {
    html! {
        div id=(id) role="separator" class="flex items-center gap-3 py-2 text-xs text-faint" {
            span class="h-px flex-1 bg-line" {}
            span { "Run #" (number) " started · " time datetime=(started_at.to_rfc3339()) { (started_at.format("%b %d %H:%M")) } }
            span class="h-px flex-1 bg-line" {}
        }
    }
}

/// Where a message of the queued stack stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedState {
    /// Held back: can still be edited or removed.
    Editable,
    /// Forwarded to the runtime but not yet delivered.
    WaitingForDelivery,
    /// Forwarded, but the task ended (`failed` or `completed`): the next run sends it again.
    NotDelivered,
}

/// A message the agent has not received yet, in the stack above the message input. Held-back
/// messages offer edit and remove; forwarded ones only say where they stand.
pub fn queued_message(
    key: &str,
    id: &str,
    text: &str,
    queued_at: DateTime<Utc>,
    state: QueuedState,
) -> Markup {
    let editable = state == QueuedState::Editable;
    html! {
        div id={"queued-" (id)} data-queued-id=(id)
            ":class"={"editing === '" (id) "' ? 'border-accent-text bg-accent-soft' : 'border-line-strong bg-app'"}
            class="flex items-start gap-2 rounded-[10px] border border-dashed px-3 py-2" {
            div class="min-w-0 flex-1" {
                div class="line-clamp-3 whitespace-pre-wrap break-words text-sm" { (text) }
                div class="mt-0.5 text-xs text-faint" {
                    "Queued " time datetime=(queued_at.to_rfc3339()) { (clock(queued_at)) }
                    @match state {
                        QueuedState::Editable => {}
                        QueuedState::WaitingForDelivery => { " · waiting for delivery" }
                        QueuedState::NotDelivered => { " · not delivered, sent with the next run" }
                    }
                }
            }
            @if editable {
                button type="button" aria-label="Edit queued message" data-text=(text)
                    "x-on:click"={"if (!editing) saved = draft; editing = '" (id) "'; draft = $el.dataset.text; $nextTick(() => $refs.message.focus())"}
                    class=(ICON_BUTTON_CLASS) { (icon(Icon::Pencil)) }
                button type="button" aria-label="Remove queued message"
                    hx-delete={"/tasks/" (key) "/messages/" (id)} hx-target="#pending" hx-swap="outerMorph"
                    "x-on:click"={"if (editing === '" (id) "') { editing = null; draft = saved }"}
                    class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
            }
        }
    }
}

/// What a tool-call row shows.
pub struct ToolRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub kind: &'a str,
    pub status: &'a str,
    pub locations: &'a [String],
    /// Labelled code blocks: input, output, diffs, result.
    pub blocks: &'a [(String, String)],
    pub truncated: bool,
}

/// A tool call: one line, expandable to its input and output.
pub fn tool_row(row: &ToolRow<'_>) -> Markup {
    let status_class = match row.status {
        "completed" => "text-status-completed-fg",
        "failed" => "text-status-failed-fg",
        _ => "text-faint",
    };
    html! {
        details id=(row.id) data-author="agent" class="ml-11" {
            summary class="flex min-h-11 cursor-pointer items-center gap-2 text-sm text-muted lg:min-h-8" {
                span class="min-w-0 flex-1 truncate font-mono text-xs" { (row.title) }
                span class="text-xs text-faint" { (row.kind) }
                span class={"text-xs " (status_class)} { (row.status) }
            }
            div class="space-y-2 pb-3 text-sm" {
                @for location in row.locations { div class="font-mono text-xs text-faint" { (location) } }
                @for (label, text) in row.blocks { (code_block(label, text)) }
                @if row.truncated { div class="text-xs text-status-waiting-fg" { "Truncated" } }
            }
        }
    }
}

/// A labelled block of preformatted text that scrolls inside itself.
pub fn code_block(label: &str, text: &str) -> Markup {
    html! {
        div {
            div class="text-xs text-faint" { (label) }
            pre class="overflow-x-auto rounded bg-app p-2 font-mono text-xs" { code { (text) } }
        }
    }
}

/// The plan of a turn as `(status, content)` entries.
pub fn plan(id: &str, entries: &[(String, String)]) -> Markup {
    html! {
        div id=(id) class="rounded-lg border border-line p-3" {
            div class="mb-1 text-xs uppercase text-faint" { "Plan" }
            ul class="space-y-1 text-sm" {
                @for (status, content) in entries {
                    li { span class="mr-2 text-faint" { "[" (status) "]" } (content) }
                }
            }
        }
    }
}

/// The end of a turn with its token usage lines.
pub fn turn_end(id: &str, stop_reason: &str, detail: &str, usage: &[String]) -> Markup {
    html! {
        div id=(id) class="border-t border-line pt-2 font-mono text-xs text-faint" {
            "Turn ended: " (stop_reason)
            @if !detail.is_empty() { " - " (detail) }
            @for line in usage { div { (line) } }
        }
    }
}

/// A notice from the runtime or the control plane.
pub fn notice(id: &str, level: &str, code: &str, message: &str) -> Markup {
    let class = match level {
        "error" => "bg-danger-soft text-danger",
        "warning" => "bg-status-waiting-bg text-status-waiting-fg",
        _ => "bg-sunk text-muted",
    };
    html! {
        div id=(id) class={"rounded-lg px-3 py-2 text-sm " (class)} {
            span class="mr-2 font-mono text-xs" { (code) }
            (message)
        }
    }
}

/// An event that could not be decoded.
pub fn unreadable(id: &str) -> Markup {
    html! { div id=(id) class="text-sm text-faint" { "Unreadable transcript event" } }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 23, 10, 42, 0).unwrap()
    }

    #[test]
    fn messages_escape_their_text_and_keep_their_ids() {
        let user = user_message("ev-1", "p1", "Peter", at(), "<b>x</b>").into_string();
        assert!(user.contains("id=\"ev-1\"") && user.contains("data-prompt-id=\"p1\""));
        assert!(user.contains("&lt;b&gt;x&lt;/b&gt;") && !user.contains("<b>x"));
    }

    #[test]
    fn user_message_is_a_right_aligned_bubble_with_meta_below_and_an_avatar() {
        let html = user_message("ev-1", "p1", "Peter Test", at(), "hi").into_string();
        assert!(
            html.contains("data-author=\"human\"") && html.contains("justify-end"),
            "{html}"
        );
        assert!(html.contains("bg-bubble-me"));
        let bubble = html.find("hi</div>").unwrap();
        let meta = html.find("Peter Test · <time").unwrap();
        assert!(bubble < meta, "name and time below the bubble");
        assert!(html.contains(">10:42<") && html.contains("class=\"avatar "));
        assert!(html.contains(">PT<"));
    }

    #[test]
    fn agent_message_is_a_left_bubble_and_markdown_sits_in_md_body() {
        let streaming =
            agent_message("msg-1", "Linus", at(), html! { "partial" }, false).into_string();
        assert!(
            streaming.contains("data-author=\"agent\"") && streaming.contains("bg-bubble-agent"),
            "{streaming}"
        );
        assert!(streaming.contains("whitespace-pre-wrap") && !streaming.contains("md-body"));
        assert!(
            streaming.find("class=\"avatar").unwrap() < streaming.find("bg-bubble-agent").unwrap()
        );
        let done =
            agent_message("msg-1", "Linus", at(), html! { p { "done" } }, true).into_string();
        assert!(done.contains("md-body") && done.contains("Linus · <time"));
    }

    #[test]
    fn run_divider_names_the_run_and_its_start() {
        let html = run_divider("run-2", 2, at()).into_string();
        assert!(
            html.contains("id=\"run-2\"") && html.contains("Run #2 started"),
            "{html}"
        );
        assert!(html.contains("Oct 23 10:42") && html.contains("role=\"separator\""));
    }

    #[test]
    fn queued_messages_offer_edit_and_remove_only_while_held_back() {
        let held =
            queued_message("DF-1", "abc", "later <b>", at(), QueuedState::Editable).into_string();
        assert!(
            held.contains("later &lt;b&gt;") && held.contains("Queued <time"),
            "{held}"
        );
        assert!(
            held.contains("aria-label=\"Edit queued message\"")
                && held.contains("data-text=\"later &lt;b&gt;\"")
        );
        assert!(
            held.contains("hx-delete=\"/tasks/DF-1/messages/abc\"")
                && held.contains("aria-label=\"Remove queued message\"")
        );
        assert!(
            held.contains("editing === 'abc'"),
            "highlighted while edited"
        );
        // The edit highlight must not lose against the static classes (CSS order, not class order).
        let class = &held[held.find("class=\"flex items-start").unwrap()..];
        let class = &class[..class[7..].find('"').unwrap() + 8];
        assert!(
            !class.contains("bg-panel") && !class.contains("border-line"),
            "{class}"
        );
        // The mockup: a dashed strong border, 10 px radius and the app background.
        assert!(
            class.contains("border-dashed") && class.contains("rounded-[10px]"),
            "{class}"
        );
        let state = &held[held.find(":class=").unwrap()..];
        assert!(
            state.contains("border-line-strong bg-app") && !state.contains("bg-panel"),
            "{state}"
        );
        let sent = queued_message(
            "DF-1",
            "abc",
            "later",
            at(),
            QueuedState::WaitingForDelivery,
        )
        .into_string();
        assert!(sent.contains("waiting for delivery") && !sent.contains("sent to the agent"));
        let ended =
            queued_message("DF-1", "abc", "later", at(), QueuedState::NotDelivered).into_string();
        assert!(
            ended.contains("not delivered, sent with the next run"),
            "{ended}"
        );
        assert!(!ended.contains("Edit queued message") && !ended.contains("hx-delete"));
        assert!(!sent.contains("Edit queued message") && !sent.contains("hx-delete"));
    }

    #[test]
    fn tool_row_summary_is_a_touch_target_and_shows_blocks() {
        let blocks = vec![("Input".to_string(), "{}".to_string())];
        let html = tool_row(&ToolRow {
            id: "ev-2",
            title: "Run tests",
            kind: "execute",
            status: "failed",
            locations: &["src/lib.rs:3".to_string()],
            blocks: &blocks,
            truncated: true,
        })
        .into_string();
        assert!(html.contains("<summary class=\"flex min-h-11"), "{html}");
        assert!(html.contains("text-status-failed-fg") && html.contains("Truncated"));
        assert!(html.contains("Input") && html.contains("src/lib.rs:3"));
    }

    #[test]
    fn notice_levels_use_distinct_tokens() {
        assert!(
            notice("n", "error", "c", "m")
                .into_string()
                .contains("bg-danger-soft")
        );
        assert!(
            notice("n", "warning", "c", "m")
                .into_string()
                .contains("bg-status-waiting-bg")
        );
        assert!(
            notice("n", "info", "c", "m")
                .into_string()
                .contains("bg-sunk")
        );
        assert!(unreadable("u").into_string().contains("Unreadable"));
        assert!(
            turn_end("t", "end_turn", "", &["a: 1 in".to_string()])
                .into_string()
                .contains("a: 1 in")
        );
        assert!(
            plan("p", &[("pending".into(), "do".into())])
                .into_string()
                .contains("[pending]")
        );
    }
}
