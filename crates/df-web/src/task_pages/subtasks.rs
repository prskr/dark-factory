//! The Subtasks tab of the details column and the done/total chip (`task-ui`, "Details tabs",
//! "Subtask progress on task rows"; design D9).

use df_core::{TaskStatus, TitleSource};
use df_db::task_views::{SubtaskItem, SubtaskProgress};
use maud::{Markup, html};

use super::view::status_name;
use crate::components::Tone;
use crate::htmx::Region;

/// The `done/total` chip of a task with subtasks (done: `in_review` or `completed`; total: not
/// `cancelled`); nothing without any.
pub(crate) fn chip(progress: SubtaskProgress) -> Markup {
    html! {
        @if progress.total > 0 {
            span data-subtasks={(progress.done) "/" (progress.total)}
                title={(progress.done) " of " (progress.total) " subtasks done"}
                class="inline-flex shrink-0 items-center rounded-full bg-sunk px-2 py-0.5 font-mono text-xs text-muted" {
                (progress.done) "/" (progress.total)
            }
        }
    }
}

/// The percentage of `done` in `total`, rounded; 0 without subtasks.
fn percent(progress: SubtaskProgress) -> i64 {
    if progress.total <= 0 {
        return 0;
    }
    (progress.done * 100 + progress.total / 2) / progress.total
}

/// A ring in the tone of the status, filled for a task that is done.
fn status_ring(status: TaskStatus) -> Markup {
    let tone = Tone::of_status(status);
    let fill = if status == TaskStatus::Completed {
        tone.dot_class()
    } else {
        "bg-transparent"
    };
    html! {
        span role="img" aria-label=(status_name(status)) title=(status_name(status))
            class={"mt-0.5 size-4 shrink-0 rounded-full border-2 " (tone.ring_class()) " " (fill)} {}
    }
}

/// The Subtasks tab: how far the subtasks are, then one row per direct subtask. The element is
/// always there (hidden by the stylesheet unless its tab is open) so that live updates find it
/// when the first subtask appears.
pub(crate) fn tab_panel(subtasks: &[SubtaskItem]) -> Markup {
    let tabbed = !subtasks.is_empty();
    let progress = SubtaskProgress::of(subtasks.iter().map(|s| s.status));
    html! {
        div id=(Region::PanelSubtasks.id()) role=[tabbed.then_some("tabpanel")] aria-labelledby=[tabbed.then_some("tab-subtasks")]
            class="ws-details-full ws-subtasks min-h-0 flex-1 overflow-y-auto" {
            @if tabbed {
                div class="space-y-2 border-b border-line px-5 py-4" {
                    div class="flex items-baseline justify-between text-sm" {
                        span class="font-medium" { (progress.done) " of " (progress.total) " done" }
                        span class="font-mono text-xs text-muted" { (percent(progress)) "%" }
                    }
                    div role="progressbar" aria-label="Subtasks done" aria-valuemin="0" aria-valuemax="100"
                        aria-valuenow=(percent(progress)) class="h-1.5 overflow-hidden rounded-full bg-sunk" {
                        div class="h-full rounded-full bg-status-completed-fg" style={"width: " (percent(progress)) "%"} {}
                    }
                }
                ul class="divide-y divide-line" {
                    @for subtask in subtasks { (row(subtask)) }
                }
            }
        }
    }
}

fn row(subtask: &SubtaskItem) -> Markup {
    let href = format!("/tasks/{}", subtask.key);
    html! {
        li {
            a href=(href) class="flex min-h-11 items-start gap-3 px-5 py-3 hover:bg-sunk" {
                (status_ring(subtask.status))
                span class="flex min-w-0 flex-1 flex-col" {
                    span title=(subtask.title)
                        class={"truncate text-sm font-medium" @if subtask.title_source == TitleSource::Placeholder { " italic text-muted" }} { (subtask.title) }
                    span class="truncate text-xs text-muted" {
                        span class="font-mono" { (subtask.key) } " · " (status_name(subtask.status))
                    }
                }
                (chip(subtask.subtasks))
            }
        }
    }
}
