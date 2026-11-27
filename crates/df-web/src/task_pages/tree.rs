//! The task list as a tree: top-level rows that open to their direct subtasks, the list scoped to
//! one task with its breadcrumb, and the cues of a row (`task-ui`, "Top-level tasks in the task
//! list", "Opening a task's subtasks", "List scoped to a subtask", "Status cues on task rows",
//! "Needs-you dot"; `task-list-tree` design D1 to D6). The page and the live stream load and
//! render the list through the same two functions.

use std::collections::HashMap;

use df_core::{TaskId, TaskKey, TaskStatus, TitleSource};
use df_db::PgPool;
use df_db::task_views::{Ancestor, ListScope, TaskFilter, TaskListItem, ancestors};
use jiff::Timestamp;
use maud::{Markup, html};

use super::resume::clock_label;
use super::view::{Filters, empty_list, shown_resume_time, status_of};
use crate::components::{
    AvatarSize, Icon, Tone, icon, needs_you_dot, persona_avatar, short_time, status_badge,
    tone_badge,
};
use crate::htmx::Region;

/// What the task list shows: the rows of the level, the subtasks of the open rows, and for a
/// scoped list the tasks above the scoped one.
#[derive(Debug, Default)]
pub(crate) struct TaskList {
    /// The top-level tasks that match the filters; for a scoped list the scoped task alone.
    pub(super) items: Vec<TaskListItem>,
    /// The direct subtasks of the open rows (or of the scoped task), by the key of their parent.
    pub(super) subtasks: HashMap<TaskKey, Vec<TaskListItem>>,
    /// The tasks above the scoped task, the parent first.
    pub(super) ancestors: Vec<Ancestor>,
    /// The list is scoped to one task (`parent` in the URL) that exists.
    pub(super) scoped: bool,
}

impl TaskList {
    /// A list of rows without open rows.
    #[cfg(test)]
    pub(super) fn of(items: Vec<TaskListItem>) -> Self {
        Self {
            items,
            ..Self::default()
        }
    }
}

/// Loads the list for `filters`: the scoped task with its subtasks when `parent` names a task,
/// otherwise the filtered rows plus the subtasks of every open row that is a listed task with
/// subtasks (other keys are dropped). Link-only status filters list flat rows and open nothing.
pub(crate) async fn load_list(
    pool: &PgPool,
    filters: &Filters,
) -> Result<TaskList, df_db::DbError> {
    let tree_filter = |scope| TaskFilter {
        scope: Some(scope),
        sort: filters.sort,
        ..TaskFilter::default()
    };
    if let Some(key) = &filters.parent {
        let scoped =
            df_db::task_views::list_task_items(pool, &tree_filter(ListScope::Task(key.clone())))
                .await?;
        if let Some(task) = scoped.into_iter().next() {
            let id = TaskId::from_uuid(task.id);
            let subtasks =
                df_db::task_views::list_task_items(pool, &tree_filter(ListScope::SubtasksOf(id)))
                    .await?;
            return Ok(TaskList {
                ancestors: ancestors(pool, id).await?,
                subtasks: HashMap::from([(task.key.clone(), subtasks)]),
                items: vec![task],
                scoped: true,
            });
        }
    }
    let items = df_db::task_views::list_task_items(pool, &filters.to_db()).await?;
    let mut subtasks = HashMap::new();
    if filters.status.lists_tree() {
        for key in &filters.open {
            let Some(task) = items.iter().find(|t| &t.key == key && t.has_subtasks) else {
                continue;
            };
            let listed = df_db::task_views::list_task_items(
                pool,
                &tree_filter(ListScope::SubtasksOf(TaskId::from_uuid(task.id))),
            )
            .await?;
            subtasks.insert(key.clone(), listed);
        }
    }
    Ok(TaskList {
        items,
        subtasks,
        ancestors: Vec::new(),
        scoped: false,
    })
}

/// The rows of the task list; replaced as a whole when a task changes. `current` is the open
/// task. Clicking a row swaps the transcript and details of that task in without touching the
/// list; once that load succeeded the phone shows the task pane and `aria-current` moves, which
/// the live morph ignores (see `layout`).
pub(crate) fn task_rows(list: &TaskList, current: Option<&str>, filters: &Filters) -> Markup {
    let tree = filters.status.lists_tree() && !list.scoped;
    html! {
        ul id=(Region::TaskRows.id()) data-rows {
            @for t in &list.items {
                @let subtasks = list.subtasks.get(&t.key);
                @let control = (tree && t.has_subtasks)
                    .then_some(Control::Disclosure { open: subtasks.is_some() });
                (task_row(t, Level::Top, control, current, filters))
                @if let Some(subtasks) = subtasks {
                    @for (at, s) in subtasks.iter().enumerate() {
                        @let last = at + 1 == subtasks.len();
                        (task_row(s, Level::Sub { last }, s.has_subtasks.then_some(Control::Scope), current, filters))
                    }
                }
            }
            @if list.items.is_empty() { li { (empty_list(filters)) } }
        }
    }
}

/// The header of a scoped list: "Tasks", every ancestor from the top-level task down, and the
/// scoped task (design D4, "Breadcrumb links"). `base` is the page's path.
pub(super) fn breadcrumb(list: &TaskList, filters: &Filters, base: &str) -> Markup {
    let link = |f: &Filters| format!("{base}{}", f.query());
    let scoped_key = list.items.first().map(|t| t.key.to_string());
    let top = list.ancestors.len().saturating_sub(1);
    html! {
        nav aria-label="Breadcrumb" class="min-w-0 flex-1" {
            ol class="flex flex-wrap items-center gap-x-1.5 text-sm text-muted" {
                li { a href=(link(&filters.unscoped_with_open(None))) class="inline-flex min-h-11 items-center hover:text-ink lg:min-h-8" { "Tasks" } }
                @for (at, ancestor) in list.ancestors.iter().enumerate().rev() {
                    @let target = if at == top { filters.unscoped_with_open(Some(&ancestor.key)) } else { filters.scoped_to(&ancestor.key) };
                    li class="flex items-center gap-1.5" {
                        span aria-hidden="true" { "›" }
                        a href=(link(&target)) class="inline-flex min-h-11 items-center hover:text-ink lg:min-h-8" { (ancestor.key.to_string()) }
                    }
                }
                li class="flex items-center gap-1.5" {
                    span aria-hidden="true" { "›" }
                    span aria-current="page" class="font-semibold text-ink" { (scoped_key.unwrap_or_default()) }
                }
            }
        }
    }
}

/// The control in a row's left gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// Opens or closes the subtasks of a top-level row.
    Disclosure { open: bool },
    /// Scopes the list to a listed subtask that has subtasks.
    Scope,
}

/// How deep a row sits: top-level (or scoped) task, or a subtask below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Top,
    Sub { last: bool },
}

/// The link of a control: the current page with the changed query. It is relative, so that the
/// rows of the live stream (which doesn't know the open task) keep it right.
fn control_href(filters: &Filters) -> String {
    match filters.query().as_str() {
        "" => "?".to_string(),
        query => query.to_string(),
    }
}

fn control_link(task_key: &TaskKey, control: Control, filters: &Filters) -> Markup {
    let key = task_key.to_string();
    let (target, label, expanded) = match control {
        Control::Disclosure { open } => (
            filters.toggled(task_key),
            format!("{} subtasks of {key}", if open { "Hide" } else { "Show" }),
            Some(open),
        ),
        Control::Scope => (
            filters.scoped_to(task_key),
            format!("Open subtasks of {key}"),
            None,
        ),
    };
    let href = control_href(&target);
    let glyph = if expanded == Some(true) {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    // The 44 px target is centred on the row's 20 px key line (below 14 px of padding on a
    // top-level row, 12 px on a subtask row) and its 16 px chevron ends 8 px before the key.
    // Disclosure controls sit on top-level rows, scope controls on subtask rows.
    let place = match control {
        Control::Disclosure { .. } => "left-0 top-0.5",
        Control::Scope => "left-5 top-0",
    };
    // A disclosure control stays in its row; the scoped list shows the task on top without a
    // control, so focus goes to its row link.
    let focus = match control {
        Control::Disclosure { .. } => format!("#control-{key}"),
        Control::Scope => format!("#row-{key} > a"),
    };
    html! {
        a id={"control-" (key)} href=(href) hx-get=(href) hx-target=(Region::TaskList.selector()) hx-select=(Region::TaskList.selector())
            hx-swap="outerHTML" hx-push-url="true" data-focus-after-swap=(focus)
            aria-label=(label) aria-expanded=[expanded.map(|open| open.to_string())]
            class={"absolute " (place) " inline-flex size-11 items-center justify-center text-faint hover:text-ink"} {
            (icon(glyph))
        }
    }
}

/// The cues of a row (design D5): the dot before the key, the own badge after it, and whether
/// the title is struck through.
struct Cues {
    dot: Option<Markup>,
    badge: Option<Markup>,
    struck: bool,
    /// "N subtasks need you" when only tasks below need you.
    below_text: Option<String>,
}

fn cues(t: &TaskListItem) -> Cues {
    let status = status_of(&t.status);
    let below_text = (!t.needs_you && t.needs_you_below > 0).then(|| match t.needs_you_below {
        1 => "1 subtask needs you".to_string(),
        n => format!("{n} subtasks need you"),
    });
    let dot = if t.needs_you {
        Some(needs_you_dot(None))
    } else {
        below_text.as_deref().map(|text| needs_you_dot(Some(text)))
    };
    let badge = if t.needs_you {
        None
    } else {
        match status {
            Some(TaskStatus::Running | TaskStatus::Completed) => None,
            Some(status) => Some(status_badge(status)),
            None => Some(tone_badge(Tone::Queued, &t.status)),
        }
    };
    Cues {
        dot,
        badge,
        struck: status == Some(TaskStatus::Completed),
        below_text,
    }
}

fn task_row(
    t: &TaskListItem,
    level: Level,
    control: Option<Control>,
    current: Option<&str>,
    filters: &Filters,
) -> Markup {
    let key = t.key.to_string();
    let href = format!("/tasks/{key}{}", filters.query());
    let resume_at = shown_resume_time(status_of(&t.status), t.resume_at);
    let cues = cues(t);
    let sub = matches!(level, Level::Sub { .. });
    let divider = match level {
        Level::Top | Level::Sub { last: true } => " border-b border-line",
        Level::Sub { last: false } => "",
    };
    let li_class = if sub {
        "relative before:absolute before:inset-y-0 before:left-4 before:w-px before:bg-line"
    } else {
        "relative"
    };
    let link_class = format!(
        "block min-h-11 border-l-[3px] border-l-transparent pr-5 hover:bg-sunk aria-[current=page]:border-l-accent aria-[current=page]:bg-accent-soft{divider} {}",
        if sub {
            "py-3 pl-[55px]"
        } else {
            "py-3.5 pl-[35px]"
        }
    );
    html! {
        li id={"row-" (key)} class=(li_class) {
            a href=(href) hx-get=(href) hx-target=(Region::WsDetail.selector()) hx-select=(Region::WsDetail.selector()) hx-swap="outerHTML" hx-push-url="true"
                data-focus-after-swap=(Region::TaskHead.selector())
                aria-current=[(current == Some(key.as_str())).then_some("page")]
                "x-on:htmx:after:swap.document"="selected($el, $event)"
                class=(link_class) {
                div class="flex items-center gap-2" {
                    @if let Some(dot) = &cues.dot { (dot) }
                    span class="font-mono text-xs text-faint" { (key) }
                    @if let Some(badge) = &cues.badge { (badge) }
                    (super::subtasks::chip(t.subtasks))
                    div class="ml-auto flex items-center gap-1.5" {
                        (persona_avatar(Some(t.persona_id), &t.persona_display_name, AvatarSize::Row))
                        time datetime=(t.updated_at.to_string()) class="font-mono text-xs text-faint" { (short_time(t.updated_at)) }
                    }
                }
                div title=(t.title) class={"mt-0.5 truncate " @if sub { "text-sm" } @else { "text-[15px]" } " font-medium" @if t.title_source == TitleSource::Placeholder { " italic text-muted" } @if cues.struck { " line-through" }} { (t.title) }
                @if let Some(at) = resume_at {
                    div class="mt-0.5 truncate text-xs text-muted" { "Resumes at " (clock_label(at, Timestamp::now())) }
                } @else if let Some(text) = &cues.below_text {
                    div class="mt-0.5 truncate text-xs text-muted" { (text) }
                } @else if !t.status_line.is_empty() { div class="mt-0.5 truncate text-xs text-muted" { (t.status_line) } }
                @if !sub { div class="mt-1 text-xs text-faint" { (t.project_name) " · " (t.persona_display_name) } }
            }
            @if let Some(control) = control { (control_link(&t.key, control, filters)) }
        }
    }
}
