//! Creator, parent and the Dependencies section of the task details.

use df_core::{Principal, TaskStatus};
use df_db::task_views::{Dependencies, TaskDetail};
use maud::{Markup, html};

use crate::components::{Icon, Tone, icon};

/// The creator of a task: a board member by name, an agent as `KEY · persona` with the key of
/// the creating task linked.
pub(super) fn creator(task: &TaskDetail) -> Markup {
    match &task.created_by {
        Principal::Human { display_name, .. } => html! { (display_name) },
        Principal::Agent { display_name, .. } => html! {
            @if let Some(key) = &task.creator_task_key {
                a href={"/tasks/" (key)} class="text-accent-text hover:underline" { (key) } " · "
            }
            (display_name)
        },
        Principal::System { .. } => html! { "System" },
    }
}

/// The dependencies of the task and the tasks that need it, in a collapsible section like the
/// runs; updated live. It is open when the page loads while the task awaits its dependencies
/// (`open` is in `morphIgnore`, so live updates keep the reader's choice) and absent for a task
/// that is in no dependency.
pub(crate) fn dependencies_region(deps: &Dependencies, status: TaskStatus) -> Markup {
    let count = deps.waits_for.len() + deps.needed_by.len();
    html! {
        div id="dependencies" class="ws-details-full" {
            @if count > 0 {
                details class="group space-y-2.5 border-b border-line px-5 py-4"
                    open[status == TaskStatus::AwaitingDependencies] {
                    summary class="flex min-h-11 cursor-pointer list-none items-center gap-1 text-xs font-medium uppercase tracking-wide text-faint lg:min-h-8 [&::-webkit-details-marker]:hidden" {
                        span class="group-open:hidden" { (icon(Icon::ChevronRight)) }
                        span class="hidden group-open:inline" { (icon(Icon::ChevronDown)) }
                        "Dependencies"
                        span class="ml-1 font-mono normal-case" { (count) }
                    }
                    @if !deps.waits_for.is_empty() {
                        h3 class="pt-1 text-xs font-medium text-muted" { "Waits for" }
                        ul class="space-y-2" {
                            @for d in &deps.waits_for {
                                (dependency_row(
                                    html! {
                                        @if d.satisfied { span class="size-2 shrink-0 rounded-full bg-status-completed-fg" {} }
                                        @else { span class="size-2 shrink-0 rounded-full border border-faint" {} }
                                    },
                                    &d.key, d.kind, &d.title,
                                    if d.satisfied { "satisfied" } else { "waiting" },
                                ))
                            }
                        }
                    }
                    @if !deps.needed_by.is_empty() {
                        h3 class="pt-1 text-xs font-medium text-muted" { "Needed by" }
                        ul class="space-y-2" {
                            @for d in &deps.needed_by {
                                (dependency_row(
                                    html! { span class={"size-2 shrink-0 rounded-full " (Tone::of_status(d.status).dot_class())} {} },
                                    &d.key, d.kind, &d.title,
                                    &super::view::status_name(d.status),
                                ))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One dependency or dependant: a dot, `KEY · kind` with the key linked, the title and a state
/// on the right.
fn dependency_row(
    dot: Markup,
    key: &str,
    kind: df_core::DependencyKind,
    title: &str,
    state: &str,
) -> Markup {
    html! {
        li class="flex min-h-11 items-center gap-2.5 rounded-lg border border-line px-3 text-sm" {
            (dot)
            span class="flex min-w-0 flex-1 flex-col" {
                span class="font-medium" {
                    a href={"/tasks/" (key)} class="text-accent-text hover:underline" { (key) }
                    " · " (kind.as_str())
                }
                span class="truncate text-xs text-muted" title=(title) { (title) }
            }
            span class="shrink-0 text-xs text-muted" { (state) }
        }
    }
}
