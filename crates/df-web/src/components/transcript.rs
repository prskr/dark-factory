//! Items of the task transcript. The folding of events into items lives in `crate::transcript`;
//! these functions only render. Every item's outer element carries `id`, the stable target of
//! live updates.

use maud::{Markup, html};

/// A message of a board member.
pub fn user_message(id: &str, prompt_id: &str, author: &str, text: &str) -> Markup {
    html! {
        div id=(id) data-prompt-id=(prompt_id) class="min-w-0 self-end rounded-2xl bg-accent-soft px-3 py-2 md:max-w-[80%]" {
            div class="text-xs text-accent-text" { (author) }
            div class="whitespace-pre-wrap break-words" { (text) }
        }
    }
}

/// A message of the agent: plain text while it streams, rendered Markdown (`body`) when done.
pub fn agent_message(id: &str, body: Markup, markdown: bool) -> Markup {
    html! {
        div id=(id) class="min-w-0 break-words" {
            @if markdown {
                div class="md-body" { (body) }
            } @else {
                div class="whitespace-pre-wrap" { (body) }
            }
        }
    }
}

/// The agent's reasoning, collapsed.
pub fn thought(id: &str, text: &str) -> Markup {
    html! {
        details id=(id) class="text-faint" {
            summary class="flex min-h-11 cursor-pointer items-center text-sm" { "Thinking" }
            div class="whitespace-pre-wrap break-words text-sm" { (text) }
        }
    }
}

/// A message that is queued but not yet delivered to the agent.
pub fn pending_message(id: &str, text: &str) -> Markup {
    html! {
        div id=(id) class="min-w-0 self-end rounded-2xl border border-dashed border-accent-text px-3 py-2 opacity-70 md:max-w-[80%]" {
            div class="text-xs text-accent-text" { "Pending" }
            div class="whitespace-pre-wrap break-words" { (text) }
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
        details id=(row.id) class="rounded-lg border border-line bg-sunk" {
            summary class="flex min-h-11 cursor-pointer items-center gap-2 px-3 text-sm" {
                span class="min-w-0 flex-1 truncate font-mono text-xs" { (row.title) }
                span class="text-xs text-faint" { (row.kind) }
                span class={"text-xs " (status_class)} { (row.status) }
            }
            div class="space-y-2 px-3 pb-3 text-sm" {
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

    #[test]
    fn messages_escape_their_text_and_keep_their_ids() {
        let user = user_message("ev-1", "p1", "Peter", "<b>x</b>").into_string();
        assert!(user.contains("id=\"ev-1\"") && user.contains("data-prompt-id=\"p1\""));
        assert!(user.contains("&lt;b&gt;x&lt;/b&gt;") && !user.contains("<b>x"));
        let pending = pending_message("pending-1", "later").into_string();
        assert!(pending.contains("Pending") && pending.contains("border-dashed"));
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
