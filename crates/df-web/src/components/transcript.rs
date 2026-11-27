//! Items of the task transcript. The folding of events into items lives in `crate::transcript`;
//! these functions only render. Every item's outer element carries `id`, the stable target of
//! live updates.

use jiff::Timestamp;
use maud::{Markup, html};

use super::actions::ICON_BUTTON_CLASS;
use super::icons::{Icon, icon};
use super::persona_avatar::{AvatarSize, PersonaLabel, persona_avatar};
use super::shell::{avatar, initials};
use crate::htmx::Region;
use crate::markdown;

/// Time shown under a bubble.
fn clock(at: Timestamp) -> String {
    at.strftime("%H:%M").to_string()
}

/// A message of a board member: a bubble on the right with name and time below it. The avatar is
/// shown for the last message of a group only (CSS, `data-author`). `parts` are `(text, hidden)`;
/// the visible text is Markdown (like an agent message, typed line breaks kept), hidden parts are
/// plain text, muted and labeled below it.
pub fn user_message(
    id: &str,
    prompt_id: &str,
    author: &str,
    at: Timestamp,
    parts: &[(String, bool)],
) -> Markup {
    let visible = parts
        .iter()
        .filter(|(_, hidden)| !hidden)
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    html! {
        div id=(id) data-prompt-id=(prompt_id) data-author="human" class="flex items-end justify-end gap-2" {
            div class="min-w-0 max-w-[85%] md:max-w-[75%]" {
                div class="break-words rounded-2xl rounded-br-md bg-bubble-me px-3 py-2 text-ink" {
                    @if !visible.is_empty() {
                        div class="md-body md-user" { (markdown::render_with_hard_breaks(&visible)) }
                    }
                    @for (text, _) in parts.iter().filter(|(_, hidden)| *hidden) {
                        div data-hidden class="mt-2 border-t border-line pt-2 text-sm text-muted" {
                            div class="text-xs text-faint" { "Instruction for the agent" }
                            (text)
                        }
                    }
                }
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
    author: &PersonaLabel,
    at: Timestamp,
    body: Markup,
    markdown: bool,
) -> Markup {
    let name = if author.display_name.is_empty() {
        "Agent"
    } else {
        &author.display_name
    };
    html! {
        div id=(id) data-author="agent" class="flex items-end gap-2" {
            (persona_avatar(author.id, name, AvatarSize::Transcript))
            div class="min-w-0 max-w-[85%] md:max-w-[75%]" {
                div class="break-words rounded-2xl rounded-bl-md bg-bubble-agent px-3 py-2 text-on-bubble-agent" {
                    @if markdown {
                        div class="md-body" { (body) }
                    } @else {
                        div class="whitespace-pre-wrap" { (body) }
                    }
                }
                div class="mt-1 text-xs text-faint" {
                    (name)
                    @if !author.handle.is_empty() { " " span class="text-faint" { "@" (author.handle) } }
                    " · "
                    time datetime=(at.to_string()) { (clock(at)) }
                }
            }
        }
    }
}

fn meta(author: &str, at: Timestamp, class: &str) -> Markup {
    html! {
        div class={"mt-1 text-xs text-faint" @if !class.is_empty() { " " (class) }} {
            @if !author.is_empty() { (author) " · " }
            time datetime=(at.to_string()) { (clock(at)) }
        }
    }
}

/// The agent's reasoning: a row in the layout of a tool row, collapsed to the brain icon,
/// "Thinking" and the first non-empty line of `text`. The opened body is the whole thought, plain
/// text while it streams and rendered Markdown once its turn has ended (`complete`).
pub fn thought(id: &str, text: &str, complete: bool) -> Markup {
    let preview = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    html! {
        // `data-tool`: the stylesheet draws the lines between consecutive rows.
        details id=(id) data-author="agent" data-tool class="group ml-11" {
            summary class="flex min-h-11 cursor-pointer items-center gap-2 text-sm text-muted lg:min-h-8" {
                span class="text-muted" { (icon(Icon::Brain)) }
                span class="shrink-0 text-muted" { "Thinking" }
                span class="min-w-0 flex-1 truncate text-xs text-faint" { (preview) }
                span class="text-faint transition-transform group-open:rotate-180" { (icon(Icon::ChevronDown)) }
            }
            div class="mb-3 rounded-lg bg-sunk p-3 text-sm" {
                @if complete {
                    div class="md-body md-tool" { (markdown::render(text)) }
                } @else {
                    div class="whitespace-pre-wrap break-words" { (text) }
                }
            }
        }
    }
}

/// The start of a run, as a divider with the run's start time.
pub fn run_divider(id: &str, number: i64, started_at: Timestamp) -> Markup {
    html! {
        div id=(id) role="separator" class="flex items-center gap-3 py-2 text-xs text-faint" {
            span class="h-px flex-1 bg-line" {}
            span { "Run #" (number) " started · " time datetime=(started_at.to_string()) { (started_at.strftime("%b %d %H:%M").to_string()) } }
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
/// messages offer edit and remove; forwarded ones only say where they stand. `author_prefix` is
/// the name of an agent or system author, shown as a prefix; human authors have none.
pub fn queued_message(
    key: &str,
    id: &str,
    author_prefix: Option<&str>,
    parts: &[(String, bool)],
    queued_at: Timestamp,
    state: QueuedState,
) -> Markup {
    let editable = state == QueuedState::Editable;
    let text = parts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    html! {
        div id={"queued-" (id)} data-queued-id=(id)
            ":class"={"editing === '" (id) "' ? 'border-accent-text bg-accent-soft' : 'border-line-strong bg-app'"}
            class="flex shrink-0 items-center gap-2 rounded-[10px] border border-dashed px-3 h-11 lg:h-9" {
            div class="min-w-0 flex-1 truncate text-sm" title=(text) {
                @if let Some(author) = author_prefix {
                    span class="text-xs text-faint" { (author) ": " }
                }
                @for (part, hidden) in parts {
                    @if *hidden {
                        // Inline: the row is one truncated line, so no rule or padding as in the bubble.
                        span data-hidden class="text-muted" {
                            span class="text-xs text-faint" { "Instruction for the agent: " }
                            (part)
                        }
                    } @else {
                        (part)
                    }
                    " "
                }
            }
            div class="shrink-0 text-xs text-faint" {
                time datetime=(queued_at.to_string()) { (clock(queued_at)) }
                @match state {
                    QueuedState::Editable => {}
                    QueuedState::WaitingForDelivery => { " · waiting for delivery" }
                    QueuedState::NotDelivered => { " · not delivered, sent with the next run" }
                }
            }
            @if editable {
                button type="button" aria-label="Edit queued message" data-text=(&text)
                    "x-on:click"={"if (!editing) saved = draft; editing = '" (id) "'; draft = $el.dataset.text; $nextTick(() => $refs.message.focus())"}
                    class=(ICON_BUTTON_CLASS) { (icon(Icon::Pencil)) }
                button type="button" aria-label="Remove queued message"
                    hx-delete={(crate::layout::item_path(key)) "/messages/" (id)} hx-target=(Region::Pending.selector()) hx-swap="outerMorph"
                    "x-on:click"={"if (editing === '" (id) "') { editing = null; draft = saved }"}
                    class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
            }
        }
    }
}

/// Where a tool call stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolStatus {
    #[default]
    Unspecified,
    Pending,
    InProgress,
    Completed,
    Failed,
    /// A status this build does not know.
    Unknown,
}

impl ToolStatus {
    /// The word the row shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    /// The call ended, successfully or not: its output is final.
    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    pub const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }

    /// The call has not ended yet.
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}

/// What a tool-call row shows.
pub struct ToolRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub kind: &'a str,
    pub status: ToolStatus,
    pub locations: &'a [String],
    /// The details: input, output, diffs, result.
    pub blocks: &'a [ToolBlock],
    pub truncated: bool,
}

/// One section of the details of a tool call, under a small uppercase `label` (and the `caption`,
/// for example the path of a diff). `parts` are ready-made blocks: highlighted code, rendered
/// Markdown or plain text.
pub struct ToolBlock {
    pub label: &'static str,
    pub caption: Option<String>,
    pub parts: Vec<Markup>,
}

/// A tool call: one line, expandable to its input and output.
pub fn tool_row(row: &ToolRow<'_>) -> Markup {
    let status_class = match row.status {
        ToolStatus::Completed => "text-status-completed-fg",
        ToolStatus::Failed => "text-status-failed-fg",
        _ => "text-faint",
    };
    html! {
        // `data-tool`: the stylesheet draws the lines between consecutive rows.
        details id=(row.id) data-author="agent" data-tool class="group ml-11" {
            summary class="flex min-h-11 cursor-pointer items-center gap-2 text-sm text-muted lg:min-h-8" {
                span class="text-muted" { (icon(Icon::Wrench)) }
                span class="min-w-0 flex-1 truncate font-mono text-xs" { (row.title) }
                span class="text-xs text-faint" { (row.kind) }
                span class={"text-xs " (status_class)} { (row.status.label()) }
                span class="text-faint transition-transform group-open:rotate-180" { (icon(Icon::ChevronDown)) }
            }
            // The sunk panel, as under an opened thought.
            div class="mb-3 space-y-3 rounded-lg bg-sunk p-3 text-sm" {
                @for location in row.locations { div class="font-mono text-xs text-faint" { (location) } }
                @for block in row.blocks { (tool_block(block)) }
                @if row.truncated { div class="text-xs text-status-waiting-fg" { "Truncated" } }
            }
        }
    }
}

fn tool_block(block: &ToolBlock) -> Markup {
    html! {
        div {
            div class="mb-1 flex items-baseline gap-2 text-[11px] font-semibold uppercase tracking-wide text-faint" {
                (block.label)
                @if let Some(caption) = &block.caption {
                    span class="font-mono text-xs font-normal normal-case tracking-normal" { (caption) }
                }
            }
            // `md-body` styles the code blocks and the Markdown output; `md-tool` the inline code.
            div class="md-body md-tool" {
                @for part in &block.parts { (part) }
            }
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

/// A notice from the runtime or the control plane: a one-line row like a tool call, expandable
/// to the full message. The level only tints the code.
pub fn notice(id: &str, level: &str, code: &str, message: &str) -> Markup {
    let code_class = match level {
        "error" => "text-danger",
        "warning" => "text-status-waiting-fg",
        _ => "text-muted",
    };
    html! {
        // `data-system`: the stylesheet draws the lines between consecutive rows (with `data-tool`).
        details id=(id) data-author="agent" data-system class="group ml-11" {
            summary class="flex min-h-11 cursor-pointer items-center gap-2 text-sm text-muted lg:min-h-8" {
                span class="text-muted" { (icon(Icon::BotMessage)) }
                span class={"shrink-0 font-mono text-xs " (code_class)} { (code) }
                span class="min-w-0 flex-1 truncate text-xs" { (message) }
                span class="text-faint transition-transform group-open:rotate-180" { (icon(Icon::ChevronDown)) }
            }
            div class="whitespace-pre-wrap break-words pb-3 text-sm" { (message) }
        }
    }
}

/// A one-line system row in the notice layout: forge activity (push, pull request) and the
/// interrupted-turn row. It is tinted with the accent (`danger` for a failed push). `link` is
/// the label and URL of a pull request; the label is only a link for an `http(s)` URL.
pub fn system_row(
    id: &str,
    glyph: Icon,
    danger: bool,
    lead: &str,
    link: Option<(&str, &str)>,
    tail: &str,
) -> Markup {
    let tint = if danger {
        "text-danger"
    } else {
        "text-accent-text"
    };
    html! {
        div id=(id) data-author="agent" data-system class={"ml-11 flex min-h-11 items-center gap-2 text-sm lg:min-h-8 " (tint)} {
            (icon(glyph))
            span class="min-w-0 flex-1 truncate" {
                (lead)
                @if let Some((label, url)) = link { (safe_link(label, url, "underline")) }
                (tail)
            }
        }
    }
}

/// A received webhook event, in the notice layout: the webhook icon and the link in the strong
/// accent color, the text around it muted. `link` is the label and URL of the PR or CI check;
/// the label is only a link for an `http(s)` URL.
pub fn webhook_row(id: &str, lead: &str, link: Option<(&str, &str)>, tail: &str) -> Markup {
    html! {
        div id=(id) data-author="agent" data-system class="ml-11 flex min-h-11 items-center gap-2 text-sm text-muted lg:min-h-8" {
            span class="text-accent-text" { (icon(Icon::Webhook)) }
            span class="min-w-0 flex-1 truncate" {
                (lead)
                @if let Some((label, url)) = link { (safe_link(label, url, "text-accent-text underline")) }
                (tail)
            }
        }
    }
}

/// `label` as a link to `url` in a new tab, but only for an `http(s)` URL; any other URL (an
/// agent or a forge can name one) leaves the label as plain text.
fn safe_link(label: &str, url: &str, class: &str) -> Markup {
    html! {
        @if url.starts_with("https://") || url.starts_with("http://") {
            a href=(url) class=(class) rel="noopener noreferrer" target="_blank" { (label) }
        } @else {
            (label)
        }
    }
}

/// What the head of a tool call group shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupHead {
    pub calls: usize,
    pub failed: usize,
    /// A call of the group is in progress.
    pub running: bool,
}

/// A streak of tool calls: a wrapper with the head and the container of the nested rows
/// (`items`). The wrapper holds the open state (Alpine) and is never morphed by live updates;
/// they replace the head (`<id>-head`) and append to `<id>-items`. A group of one call hides its
/// head and shows the row like a plain tool row (stylesheet, `data-single`).
pub fn tool_group(id: &str, head: GroupHead, items: &Markup) -> Markup {
    html! {
        // `data-tool`: the stylesheet draws the lines around the group; the rows inside it have none.
        div id=(id) data-author="agent" data-tool data-tool-group class="ml-11"
            x-data="{ open: false }" ":data-open"="open ? 'true' : null" {
            (tool_group_head(id, head))
            div id={(id) "-items"} data-group-items class="[&>*]:ml-0" { (items) }
        }
    }
}

/// The head row of the group `id`: wrench, "N tool calls", the failed count, the running
/// indicator and the chevron, as the button that opens the group.
pub fn tool_group_head(id: &str, head: GroupHead) -> Markup {
    html! {
        div id={(id) "-head"} data-group-head data-single[head.calls < 2] {
            // The open state is Alpine's: `aria-expanded` is skipped when morphing (`HTMX_CONFIG`).
            button type="button" "x-on:click"="open = !open" ":aria-expanded"="open"
                class="flex min-h-11 w-full cursor-pointer items-center gap-2 text-left text-sm text-muted lg:min-h-8" {
                span { (icon(Icon::Wrench)) }
                span { (head.calls) " tool calls" }
                @if head.failed > 0 {
                    span class="text-xs text-status-failed-fg" { "\u{b7} " (head.failed) " failed" }
                }
                span class="min-w-0 flex-1" {}
                @if head.running {
                    span class="flex items-center gap-1 text-xs text-status-running-fg" {
                        span class="animate-spin [&>svg]:size-3.5" { (icon(Icon::Loader)) }
                        "running"
                    }
                }
                span data-group-chevron class="text-faint transition-transform" { (icon(Icon::ChevronDown)) }
            }
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

    use crate::testkit::tag_around;

    fn at() -> Timestamp {
        "2026-10-23T10:42:00Z".parse().unwrap()
    }

    #[test]
    fn times_show_utc_clock_text_and_an_rfc3339_datetime_attribute() {
        let divider = run_divider("r1", 3, at()).into_string();
        assert!(
            divider.contains("<time datetime=\"2026-10-23T10:42:00Z\">Oct 23 10:42</time>"),
            "{divider}"
        );
        assert_eq!(clock(at()), "10:42");
    }

    #[test]
    fn messages_escape_their_text_and_keep_their_ids() {
        let user =
            user_message("ev-1", "p1", "Peter", at(), &[("<b>x</b>".into(), false)]).into_string();
        assert!(user.contains("id=\"ev-1\"") && user.contains("data-prompt-id=\"p1\""));
        assert!(user.contains("&lt;b&gt;x&lt;/b&gt;") && !user.contains("<b>x"));
    }

    #[test]
    fn a_user_message_is_markdown_with_highlighted_code_and_kept_line_breaks() {
        let text = "one\ntwo\n\n```rust\nfn main() {}\n```";
        let html = user_message("ev-1", "p1", "Peter", at(), &[(text.into(), false)]).into_string();

        let body = &html[html.find("md-body").expect("markdown body")..];
        assert!(body.contains("one<br />\ntwo"), "{body}");
        let pre = &body[body.find("<pre class=\"hl-code\">").expect("pre block")..];
        assert!(
            pre[..pre.find("</pre>").unwrap()].contains("<span class=\"hl-"),
            "{pre}"
        );
        assert!(!html.contains("whitespace-pre-wrap"), "{html}");
        // The bubble is light in both themes: inline code takes the panel color there.
        assert!(tag_around(&html, "md-body").contains("md-user"), "{html}");
    }

    #[test]
    fn a_user_message_keeps_hidden_parts_plain_text() {
        let parts = [
            ("visible".to_string(), false),
            ("**not bold** `x`".to_string(), true),
        ];
        let html = user_message("ev-1", "p1", "Peter", at(), &parts).into_string();

        let hidden = &html[html.find("data-hidden").unwrap()..];
        assert!(hidden.contains("**not bold** `x`"), "{hidden}");
        assert!(
            !hidden.contains("<strong>") && !hidden.contains("<code>"),
            "{hidden}"
        );
        let body = &html[html.find("md-body").unwrap()..];
        assert!(
            body.find("</div>").unwrap() < body.find("data-hidden").unwrap(),
            "the hidden part is outside the Markdown body: {html}"
        );
    }

    #[test]
    fn the_queued_stack_stays_plain_text() {
        let held = queued_message(
            "DF-1",
            "abc",
            None,
            &[("**bold** `x`".into(), false)],
            at(),
            QueuedState::Editable,
        )
        .into_string();
        assert!(
            held.contains("**bold** `x`") && !held.contains("md-body"),
            "{held}"
        );
    }

    #[test]
    fn user_message_is_a_right_aligned_bubble_with_meta_below_and_an_avatar() {
        let html =
            user_message("ev-1", "p1", "Peter Test", at(), &[("hi".into(), false)]).into_string();
        assert!(
            html.contains("data-author=\"human\"") && html.contains("justify-end"),
            "{html}"
        );
        assert!(html.contains("bg-bubble-me"));
        let bubble = html.find("hi</p>").unwrap();
        let meta = html.find("Peter Test · <time").unwrap();
        assert!(bubble < meta, "name and time below the bubble");
        assert!(html.contains(">10:42<") && html.contains("class=\"avatar "));
        assert!(html.contains(">PT<"));
    }

    #[test]
    fn hidden_parts_are_muted_and_labeled_below_the_visible_text() {
        let parts = [
            ("desc".to_string(), false),
            ("call <tool>".to_string(), true),
        ];
        let html = user_message("ev-1", "p1", "Peter", at(), &parts).into_string();
        let label = html.find("Instruction for the agent").expect("label");
        assert!(html.find("desc").unwrap() < label, "{html}");
        assert!(
            html.contains("data-hidden") && html.contains("text-muted"),
            "{html}"
        );
        assert!(html.contains("call &lt;tool&gt;"), "{html}");

        let plain = user_message("ev-1", "p1", "Peter", at(), &parts[..1]).into_string();
        assert!(!plain.contains("Instruction for the agent"), "{plain}");
    }

    #[test]
    fn agent_message_is_a_left_bubble_and_markdown_sits_in_md_body() {
        let streaming =
            agent_message("msg-1", &"Linus".into(), at(), html! { "partial" }, false).into_string();
        assert!(
            streaming.contains("data-author=\"agent\"") && streaming.contains("bg-bubble-agent"),
            "{streaming}"
        );
        assert!(streaming.contains("whitespace-pre-wrap") && !streaming.contains("md-body"));
        assert!(
            streaming.find("class=\"avatar").unwrap() < streaming.find("bg-bubble-agent").unwrap()
        );
        let done = agent_message("msg-1", &"Linus".into(), at(), html! { p { "done" } }, true)
            .into_string();
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
        let held = queued_message(
            "DF-1",
            "abc",
            None,
            &[("later <b>".into(), false)],
            at(),
            QueuedState::Editable,
        )
        .into_string();
        assert!(
            held.contains("later &lt;b&gt;") && held.contains("<time"),
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
        for label in ["Edit queued message", "Remove queued message"] {
            let button = tag_around(&held, &format!("aria-label=\"{label}\""));
            assert!(button.contains(ICON_BUTTON_CLASS), "{button}");
        }
        assert!(
            held.contains("editing === 'abc'"),
            "highlighted while edited"
        );
        // The edit highlight must not lose against the static classes (CSS order, not class order).
        let class = &held[held.find("class=\"flex shrink-0").unwrap()..];
        let class = &class[..class[7..].find('"').unwrap() + 8];
        assert!(
            !class.contains("bg-panel") && !class.contains("border-line"),
            "{class}"
        );
        // The board's single-line item: fixed height, truncated text with the full text in `title`.
        assert!(
            class.contains("h-11 lg:h-9")
                && held.contains("truncate")
                && held.contains("title=\"later &lt;b&gt;\""),
            "{held}"
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
            None,
            &[("later".into(), false)],
            at(),
            QueuedState::WaitingForDelivery,
        )
        .into_string();
        assert!(sent.contains("waiting for delivery") && !sent.contains("sent to the agent"));
        let ended = queued_message(
            "DF-1",
            "abc",
            None,
            &[("later".into(), false)],
            at(),
            QueuedState::NotDelivered,
        )
        .into_string();
        assert!(
            ended.contains("not delivered, sent with the next run"),
            "{ended}"
        );
        assert!(!ended.contains("Edit queued message") && !ended.contains("hx-delete"));
        assert!(!sent.contains("Edit queued message") && !sent.contains("hx-delete"));
    }

    #[test]
    fn tool_row_summary_is_a_touch_target_and_shows_blocks() {
        let blocks = vec![ToolBlock {
            label: "Input",
            caption: None,
            parts: vec![html! { "{}" }],
        }];
        let html = tool_row(&ToolRow {
            id: "ev-2",
            title: "Run tests",
            kind: "execute",
            status: ToolStatus::Failed,
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
    fn tool_row_has_the_wrench_in_front_and_a_turning_chevron_at_the_end() {
        let html = tool_row(&ToolRow {
            id: "ev-2",
            title: "Run tests",
            kind: "execute",
            status: ToolStatus::Completed,
            locations: &[],
            blocks: &[],
            truncated: false,
        })
        .into_string();
        let wrench = html
            .find(&icon(Icon::Wrench).into_string())
            .expect("wrench");
        let chevron = html
            .find(&icon(Icon::ChevronDown).into_string())
            .expect("chevron");
        assert!(wrench < html.find("Run tests").unwrap(), "{html}");
        assert!(html.find("completed").unwrap() < chevron, "{html}");
        assert!(
            html.contains(
                "<details id=\"ev-2\" data-author=\"agent\" data-tool class=\"group ml-11\""
            ),
            "{html}"
        );
        assert!(html.contains("group-open:rotate-180"), "{html}");
    }

    #[test]
    fn notice_is_an_expandable_row_with_the_bot_icon_and_a_tinted_code() {
        let html = notice(
            "n",
            "error",
            "runtime.outbox_discarded",
            "long <b>message</b>",
        )
        .into_string();
        assert!(
            html.starts_with(
                "<details id=\"n\" data-author=\"agent\" data-system class=\"group ml-11\""
            ),
            "{html}"
        );
        let bot = html
            .find(&icon(Icon::BotMessage).into_string())
            .expect("icon");
        assert!(
            bot < html.find("runtime.outbox_discarded").unwrap(),
            "{html}"
        );
        assert!(
            html.contains("text-danger") && !html.contains("bg-"),
            "{html}"
        );
        assert!(html.contains("group-open:rotate-180"), "{html}");
        // Summary (truncated) and body both carry the escaped message.
        assert_eq!(
            html.matches("long &lt;b&gt;message&lt;/b&gt;").count(),
            2,
            "{html}"
        );
        assert!(
            notice("n", "warning", "c", "m")
                .into_string()
                .contains("text-status-waiting-fg")
        );
        assert!(
            notice("n", "info", "c", "m")
                .into_string()
                .contains("text-muted")
        );
        assert!(unreadable("u").into_string().contains("Unreadable"));
        assert!(
            plan("p", &[("pending".into(), "do".into())])
                .into_string()
                .contains("[pending]")
        );
    }

    #[test]
    fn a_thought_is_a_tool_row_with_the_brain_a_preview_and_a_chevron() {
        let text = "\n  Check the failing test first\nthen the rest";
        let html = thought("th-1", text, false).into_string();

        assert!(
            html.starts_with(
                "<details id=\"th-1\" data-author=\"agent\" data-tool class=\"group ml-11\""
            ),
            "{html}"
        );
        let summary = &html[..html.find("</summary>").unwrap()];
        let brain = summary
            .find(&icon(Icon::Brain).into_string())
            .expect("brain");
        let label = summary.find("Thinking").expect("label");
        let preview = summary
            .find("Check the failing test first")
            .expect("preview");
        let chevron = summary
            .find(&icon(Icon::ChevronDown).into_string())
            .expect("chevron");
        assert!(
            brain < label && label < preview && preview < chevron,
            "{html}"
        );
        assert!(
            !summary.contains("then the rest"),
            "only the first line: {summary}"
        );
        assert!(summary.contains("truncate") && html.contains("group-open:rotate-180"));
        // The open body sits in the sunk panel of an opened tool row, plain while streaming.
        assert!(
            html.contains("bg-sunk") && html.contains("whitespace-pre-wrap"),
            "{html}"
        );
        assert!(!html.contains("md-body"), "{html}");
    }

    #[test]
    fn a_finished_thought_body_is_markdown() {
        let html = thought("th-1", "x", true).into_string();

        assert!(
            html.contains("md-body") && !html.contains("whitespace-pre-wrap"),
            "{html}"
        );
    }

    #[test]
    fn a_tool_group_head_counts_calls_failures_and_the_running_call() {
        let head = |calls, failed, running| {
            tool_group_head(
                "grp-ev-1",
                GroupHead {
                    calls,
                    failed,
                    running,
                },
            )
            .into_string()
        };

        let plain = head(12, 0, false);
        assert!(plain.contains("12 tool calls"), "{plain}");
        assert!(
            !plain.contains("failed") && !plain.contains("running"),
            "{plain}"
        );
        assert!(plain.contains("aria-hidden") && plain.contains(":aria-expanded=\"open\""));
        assert!(plain.contains("<button type=\"button\""), "{plain}");

        let failed = head(12, 2, false);
        let tint = &failed[failed.find("text-status-failed-fg").expect("failed tint")..];
        assert!(tint.contains("2 failed"), "{failed}");

        let running = head(3, 0, true);
        let spinner = running.find("animate-spin").expect("spinner");
        assert!(running.contains("text-status-running-fg") && running.contains("running"));
        let chevron = running.find("data-group-chevron").unwrap();
        assert!(spinner < chevron, "the chevron is last: {running}");
        assert!(
            running.find(&icon(Icon::Wrench).into_string()).unwrap()
                < running.find("3 tool calls").unwrap()
        );
    }

    #[test]
    fn a_group_of_one_call_marks_its_head_single() {
        let one = tool_group_head(
            "g",
            GroupHead {
                calls: 1,
                failed: 0,
                running: false,
            },
        )
        .into_string();
        let two = tool_group_head(
            "g",
            GroupHead {
                calls: 2,
                failed: 0,
                running: false,
            },
        )
        .into_string();

        assert!(one.contains("data-single"), "{one}");
        assert!(!two.contains("data-single"), "{two}");
    }

    #[test]
    fn a_tool_group_wrapper_holds_the_open_state_and_the_ids_live_updates_target() {
        let head = GroupHead {
            calls: 2,
            failed: 0,
            running: false,
        };
        let html = tool_group("grp-ev-1", head, &html! { div id="ev-1" {} }).into_string();

        assert!(
            html.starts_with("<div id=\"grp-ev-1\" data-author=\"agent\" data-tool data-tool-group class=\"ml-11\""),
            "{html}"
        );
        assert!(html.contains("x-data=\"{ open: false }\""), "{html}");
        assert!(html.contains("id=\"grp-ev-1-head\"") && html.contains("id=\"grp-ev-1-items\""));
        assert!(html.find("-head").unwrap() < html.find("-items").unwrap());
        assert!(
            html.contains("[&amp;&gt;*]:ml-0"),
            "nested rows are indented by the container: {html}"
        );
    }

    #[test]
    fn a_webhook_row_has_the_webhook_icon_and_an_accented_underlined_link() {
        let html = webhook_row(
            "ev-3",
            "alice requested changes on PR ",
            Some(("#41", "https://forge.example/pr/41")),
            "",
        )
        .into_string();

        assert!(
            html.contains("data-system")
                && html.contains("ml-11")
                && html.contains("data-author=\"agent\""),
            "{html}"
        );
        assert!(html.contains(&icon(Icon::Webhook).into_string()), "{html}");
        let link = tag_around(&html, "href=\"https://forge.example/pr/41\"");
        assert!(
            link.contains("text-accent-text") && link.contains("underline"),
            "{link}"
        );
        assert!(html.contains("alice requested changes on PR <a "), "{html}");
    }

    #[test]
    fn a_webhook_row_links_only_http_urls() {
        for url in ["javascript:alert(1)", "", "ftp://x"] {
            let html =
                webhook_row("ev-3", "CI check ", Some(("test", url)), " failed on x").into_string();

            assert!(
                !html.contains("<a ") && !html.contains("javascript"),
                "{html}"
            );
            assert!(html.contains("CI check test failed on x"), "{html}");
        }
    }

    #[test]
    fn tool_details_label_their_blocks_in_small_uppercase() {
        let blocks = vec![ToolBlock {
            label: "Diff",
            caption: Some("src/lib.rs".into()),
            parts: vec![html! { "d" }],
        }];
        let html = tool_row(&ToolRow {
            id: "ev-2",
            title: "Edit",
            kind: "edit",
            status: ToolStatus::Completed,
            locations: &[],
            blocks: &blocks,
            truncated: false,
        })
        .into_string();

        let label = tag_around(&html, "Diff");
        assert!(label.contains("uppercase"), "{label}");
        assert!(
            html.contains("src/lib.rs") && html.contains("bg-sunk"),
            "{html}"
        );
    }
}
