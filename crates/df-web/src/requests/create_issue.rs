//! The card of a `create-issue` request: the exact title, the labels and the body in one tab box,
//! Preview (rendered, the default) and Markdown (the stored text). Both views come from the stored
//! payload; the decision actions stay outside the box. Everything is untrusted agent text.

use df_db::create_issue::{IssuePayload, created_issue, issue_error, issue_payload};
use df_db::requests::{RequestRow, RequestState};
use maud::{Markup, html};

use super::{CardData, chip};
use crate::markdown;

/// Comment placeholder of the card: the note goes to the agent, never to the issue.
pub(super) const COMMENT_PLACEHOLDER: &str = "Note to the agent on denial (optional)";

const TAB_CLASS: &str = "-mb-px min-h-9 rounded-t-md border border-transparent px-3 text-sm text-muted hover:text-ink \
     aria-selected:border-line aria-selected:border-b-panel aria-selected:bg-panel aria-selected:font-medium aria-selected:text-ink";
const PANEL_CLASS: &str = "max-h-80 min-w-0 overflow-y-auto";

/// Title, reason and the tab box of the body.
pub(super) fn body(card: &CardData, id: &str) -> Markup {
    let row = &card.row;
    let issue = issue_payload(&row.payload).unwrap_or(IssuePayload {
        title: String::new(),
        body: String::new(),
        labels: Vec::new(),
    });
    let (preview, raw) = (format!("{id}-preview"), format!("{id}-markdown"));
    html! {
        p class="min-w-0 break-words text-[15px] font-semibold" { (issue.title) }
        @if !row.reason.is_empty() {
            div class="md-body md-request min-w-0 break-words text-sm [&_p]:my-1" { (markdown::render(&row.reason)) }
        }
        div x-data="tabs('preview', false, false)" class="overflow-hidden rounded-lg border border-line bg-panel" {
            div class="flex items-end gap-2 border-b border-line bg-sunk px-2 pt-1.5" {
                div class="flex min-w-0 flex-1 flex-wrap items-center gap-1 pb-1.5" {
                    @for label in &issue.labels { (chip(label)) }
                }
                div role="tablist" aria-label="Issue text" x-on:keydown="move($event)" class="flex shrink-0" {
                    (tab("preview", "Preview", &preview, true))
                    (tab("markdown", "Markdown", &raw, false))
                }
            }
            div role="tabpanel" id=(preview) aria-labelledby={(preview) "-tab"} x-show="tab === 'preview'" class={(PANEL_CLASS) " p-3"} {
                div class="md-body md-request break-words text-sm [&_p]:my-1" { (markdown::render(&issue.body)) }
            }
            div role="tabpanel" id=(raw) aria-labelledby={(raw) "-tab"} x-show="tab === 'markdown'" x-cloak class=(PANEL_CLASS) {
                pre class="m-0 whitespace-pre-wrap break-words p-3 font-mono text-xs text-ink" { code { (issue.body) } }
            }
        }
    }
}

fn tab(id: &str, label: &str, panel: &str, selected: bool) -> Markup {
    html! {
        button type="button" role="tab" id={(panel) "-tab"} data-tab=(id) aria-controls=(panel)
            aria-selected=(selected) tabindex=(if selected { "0" } else { "-1" })
            ":aria-selected"={"tab === '" (id) "'"} ":tabindex"={"tab === '" (id) "' ? 0 : -1"}
            x-on:click={"tab = '" (id) "'"} class=(TAB_CLASS) { (label) }
    }
}

/// What was decided, by whom and, once the job recorded it, which issue came out of it.
pub(super) fn outcome(row: &RequestRow) -> Markup {
    let by = match &row.decided_by {
        Some(df_core::Principal::Human { display_name, .. }) => format!(" by {display_name}"),
        _ => String::new(),
    };
    html! {
        @match row.state {
            RequestState::Approved => {
                "Approved" (by)
                @if let Some(issue) = created_issue(row) {
                    " · created issue "
                    @if issue.url.starts_with("https://") || issue.url.starts_with("http://") {
                        a href=(issue.url) target="_blank" rel="noopener" class="font-medium text-accent-text hover:underline" { "#" (issue.number) }
                    } @else {
                        "#" (issue.number)
                    }
                } @else if let Some(error) = issue_error(row) {
                    " · the issue could not be created: " (error)
                }
            }
            RequestState::Denied => {
                "Denied" (by)
                @if let Some(comment) = row.comment() { ": " (comment) }
            }
            _ => { (super::outcome(row)) }
        }
    }
}
