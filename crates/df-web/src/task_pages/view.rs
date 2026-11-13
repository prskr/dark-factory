//! Markup of the task workspace: list, transcript header, details, runs, pending messages and
//! the composer. Handlers live in the parent module; the live-update regions (`#task-head`,
//! `#task-details`, `#runs`, `#pending`, `#task-rows`) are re-rendered by `crate::sse`.

use chrono::{DateTime, Utc};
use df_core::{RunState, StatusSet, TITLE_MAX_CHARS, TaskSort, TaskStatus, TitleSource};
use df_db::TaskMessageRow;
use df_db::task_views::{ProjectFilterItem, RunView, TaskDetail, TaskListItem};
use maud::{Markup, html};

use super::resume::{ResumeNote, clock_label};
use crate::components::QueuedState;
use crate::components::forms::{INPUT_CLASS, MenuItem};
use crate::components::transcript::queued_message;
use crate::components::{
    ButtonKind, DialogStyle, EmptyPanel, EmptySize, FormErrors, ICON_BUTTON_CLASS, Icon, Tone,
    action_class, attached_badge, chip, definition_list, details_panel_with_heading, dialog_close,
    dialog_with, field_error, filter_chip, form_error, harness_label, icon, icon_menu,
    phone_profile_menu, search_menu, section, status_badge, tone_badge,
};
use crate::components::{ROUND_ACTION_CLASS, ROUND_PILL_CLASS};
use crate::layout::Shell;
use crate::transcript::block_parts;

const CHIP_SELECT: &str =
    "min-h-11 rounded-[10px] border border-line bg-panel px-3 text-sm text-muted lg:min-h-8";
/// The persona picker of the composer sits in the card without a border of its own.
const PERSONA_SELECT: &str =
    "min-h-11 rounded-full bg-transparent px-3 text-sm text-ink lg:min-h-8";
/// A native modal `<dialog>`, centered from `md` and anchored to the bottom on a phone.
const DIALOG_CLASS: &str = "fixed inset-x-0 bottom-0 m-0 mt-auto w-full max-w-none overflow-visible bg-transparent p-3 text-ink backdrop:bg-black/50 md:inset-0 md:m-auto md:h-fit md:max-w-md md:p-0";
/// Ctrl/Cmd+Enter in the message box: Save while editing a queued message, otherwise the Send
/// button that `#composer-action` holds at that moment.
const SEND_SHORTCUT: &str = "(editing ? $refs.save : document.querySelector('#composer-action [aria-label=Send]'))?.click()";

/// The status filter of the task list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum StatusFilter {
    #[default]
    Active,
    Waiting,
    Done,
    All,
    /// Only reachable by link (project counters): no chip, all chips unselected.
    Queued,
    /// Only reachable by link: running and rate limited.
    Running,
}

impl StatusFilter {
    /// The filters that have a chip.
    pub(crate) const CHIPS: [Self; 4] = [Self::Active, Self::Waiting, Self::Done, Self::All];
    const VALUES: [Self; 6] = [
        Self::Active,
        Self::Waiting,
        Self::Done,
        Self::All,
        Self::Queued,
        Self::Running,
    ];

    /// Parses the `status` query parameter; unknown values mean the default.
    pub(crate) fn parse(value: Option<&str>) -> Self {
        Self::VALUES
            .into_iter()
            .find(|f| Some(f.as_str()) == value)
            .unwrap_or_default()
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::All => "all",
            Self::Queued => "queued",
            Self::Running => "running",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Done => "Done",
            Self::All => "All",
            Self::Queued => "Queued",
            Self::Running => "Running",
        }
    }

    pub(crate) const fn group(self) -> Option<StatusSet> {
        match self {
            Self::Active => Some(StatusSet::Active),
            Self::Waiting => Some(StatusSet::Waiting),
            Self::Done => Some(StatusSet::Done),
            Self::All => None,
            Self::Queued => Some(StatusSet::Queued),
            Self::Running => Some(StatusSet::Running),
        }
    }
}

/// The filters of the task list; they live in the URL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Filters {
    pub status: StatusFilter,
    pub project: Option<String>,
    pub sort: TaskSort,
}

impl Filters {
    pub(crate) fn new(status: Option<&str>, project: Option<&str>, sort: Option<&str>) -> Self {
        Self {
            status: StatusFilter::parse(status),
            project: project.filter(|p| !p.is_empty()).map(str::to_string),
            sort: sort.map(TaskSort::from_query).unwrap_or_default(),
        }
    }

    pub(crate) fn to_db(&self) -> df_db::task_views::TaskFilter {
        df_db::task_views::TaskFilter {
            group: self.status.group(),
            project_key: self.project.clone(),
            sort: self.sort,
        }
    }

    /// The query string (with `?`) of these filters; defaults are left out.
    pub(crate) fn query(&self) -> String {
        let mut parts = Vec::new();
        if self.status != StatusFilter::default() {
            parts.push(format!("status={}", self.status.as_str()));
        }
        if let Some(project) = &self.project {
            parts.push(format!("project={}", url_encode(project)));
        }
        if self.sort != TaskSort::default() {
            parts.push(format!("sort={}", self.sort.as_query()));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }

    /// The composer's URL; the current project filter is its default project.
    pub(crate) fn compose_href(&self) -> String {
        match &self.project {
            Some(project) => format!("/tasks/compose?project={}", url_encode(project)),
            None => "/tasks/compose".to_string(),
        }
    }

    fn with_status(&self, status: StatusFilter) -> Self {
        Self {
            status,
            ..self.clone()
        }
    }

    fn with_project(&self, project: Option<&str>) -> Self {
        Self {
            project: project.map(str::to_string),
            ..self.clone()
        }
    }

    fn with_sort(&self, sort: TaskSort) -> Self {
        Self {
            sort,
            ..self.clone()
        }
    }
}

/// The rows of the task list; replaced as a whole when a task changes. `current` is the open
/// task. Clicking a row swaps the transcript and details of that task in without touching the
/// list; `aria-current` is moved by the click and ignored by the live morph (see `layout`).
pub(crate) fn task_rows(
    items: &[TaskListItem],
    current: Option<&str>,
    filters: &Filters,
) -> Markup {
    let query = filters.query();
    html! {
        ul id="task-rows" {
            @for t in items {
                @let href = format!("/tasks/{}{}", t.key, query);
                @let status = status_of(&t.status);
                @let resume_at = shown_resume_time(status, t.resume_at);
                li id={"row-" (t.key)} {
                    a href=(href) hx-get=(href) hx-target="#ws-detail" hx-select="#ws-detail" hx-swap="outerHTML" hx-push-url="true"
                        aria-current=[(current == Some(t.key.as_str())).then_some("page")]
                        "x-on:click"="document.body.dataset.view = 'task'; details = false; $root.querySelectorAll('#task-rows [aria-current]').forEach(e => e.removeAttribute('aria-current')); $el.setAttribute('aria-current', 'page')"
                        class="block min-h-11 border-b border-line border-l-[3px] border-l-transparent py-3.5 pr-5 pl-[17px] hover:bg-sunk aria-[current=page]:border-l-accent aria-[current=page]:bg-accent-soft" {
                        div class="flex items-center gap-2" {
                            span class="font-mono text-xs text-faint" { (t.key) }
                            @if let Some(status) = status { (status_badge(status)) } @else { (tone_badge(Tone::Queued, &t.status)) }
                            time datetime=(t.updated_at.to_rfc3339()) class="ml-auto font-mono text-xs text-faint" { (short_time(t.updated_at)) }
                        }
                        div title=(t.title) class={"mt-0.5 truncate text-[15px] font-medium" @if t.title_source == TitleSource::Placeholder { " italic text-muted" }} { (t.title) }
                        @if let Some(at) = resume_at {
                            div class="mt-0.5 truncate text-xs text-muted" { "Resumes at " (clock_label(at, Utc::now())) }
                        } @else if !t.status_line.is_empty() { div class="mt-0.5 truncate text-xs text-muted" { (t.status_line) } }
                        div class="mt-1 text-xs text-faint" { (t.project_name) " · " (t.persona_name) }
                    }
                }
            }
            @if items.is_empty() { li { (empty_list(filters)) } }
        }
    }
}

/// The resume time a task row shows: only for a status that waits for it.
fn shown_resume_time(
    status: Option<TaskStatus>,
    resume_at: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    resume_at.filter(|_| status.is_some_and(|s| s.waits_for_resume()))
}

/// What the task list says when no task matches the filters.
fn empty_list(filters: &Filters) -> Markup {
    // The way out is every task: no status and no project filter.
    let filtered = filters.status != StatusFilter::All || filters.project.is_some();
    let title = match (filters.status, &filters.project) {
        (StatusFilter::All, None) => "No tasks yet".to_string(),
        (StatusFilter::All, Some(_)) => "No tasks".to_string(),
        (status, _) => format!("No {} tasks", status.label().to_lowercase()),
    };
    let sentence = match filters.status {
        StatusFilter::Active => "Queued, running and rate-limited tasks show up here.",
        StatusFilter::Waiting => "Tasks that wait for you or hit a limit show up here.",
        StatusFilter::Done => "Completed, cancelled and failed tasks show up here.",
        StatusFilter::Queued => "Tasks waiting for a run show up here.",
        StatusFilter::Running => "Running and rate-limited tasks show up here.",
        StatusFilter::All => "Create a task to get started.",
    };
    EmptyPanel {
        size: EmptySize::List,
        icon: None,
        title: &title,
        text: sentence,
        phone_hint: Some("Tap New task to start one."),
        action: if filtered {
            html! { a href="/tasks?status=all" class="mt-1 inline-flex min-h-11 items-center text-sm font-medium text-accent-text underline" { "Show all tasks" } }
        } else {
            html! {}
        },
    }
    .render()
}

/// The list column: title row with the project menu, status chips, the live list and its stream.
/// `shell` is the signed-in board member and version, shown as the profile menu on phones.
pub(crate) fn list_column(
    items: &[TaskListItem],
    projects: &[ProjectFilterItem],
    filters: &Filters,
    current: Option<&str>,
    shell: &Shell,
) -> Markup {
    let base = current.map_or_else(|| "/tasks".to_string(), |key| format!("/tasks/{key}"));
    let href = |f: &Filters| format!("{base}{}", f.query());
    let selected = filters
        .project
        .as_deref()
        .and_then(|key| projects.iter().find(|p| p.key == key));
    let menu_label = selected.map_or("All projects", |p| p.name.as_str());
    let mut menu: Vec<MenuItem> = vec![MenuItem {
        href: href(&filters.with_project(None)),
        label: "All projects".into(),
        hint: String::new(),
        current: filters.project.is_none(),
    }];
    menu.extend(projects.iter().map(|p| MenuItem {
        href: href(&filters.with_project(Some(&p.key))),
        label: p.name.clone(),
        hint: format!("{} · {}", p.key, p.task_count),
        current: filters.project.as_deref() == Some(p.key.as_str()),
    }));
    let sorts: Vec<MenuItem> = TaskSort::ALL
        .into_iter()
        .map(|sort| MenuItem {
            href: href(&filters.with_sort(sort)),
            label: sort.label().into(),
            hint: String::new(),
            current: filters.sort == sort,
        })
        .collect();
    let events = format!("/tasks/events{}", filters.query());
    html! {
        section class="ws-list" aria-label="Tasks" {
            // From 768 px the list collapses to this strip (`listCollapsed` of the workspace).
            div class="ws-list-strip flex-col items-center py-3" {
                button type="button" aria-label="Expand task list" "x-on:click"="setListCollapsed(false)" class=(ICON_BUTTON_CLASS) { (icon(Icon::PanelLeft)) }
            }
            div class="ws-list-full space-y-3 border-b border-line p-4" {
                div class="flex items-center justify-between gap-2" {
                    h1 class="text-lg font-semibold" { "Tasks" }
                    div class="ml-auto flex items-center gap-1" {
                        (icon_menu(Icon::ArrowDownUp, filters.sort.label(), &sorts))
                        div class="hidden md:block" { (search_menu(menu_label, &menu)) }
                    }
                    button type="button" aria-label="Collapse task list" "x-on:click"="setListCollapsed(true)" class={(ICON_BUTTON_CLASS) " max-md:hidden"} { (icon(Icon::PanelLeft)) }
                    (phone_profile_menu(shell))
                }
                div class="flex flex-wrap items-center gap-2" {
                    @for status in StatusFilter::CHIPS {
                        (filter_chip(status.label(), &href(&filters.with_status(status)), filters.status == status))
                    }
                    // Phones have no project menu: the filter set from the Projects screen shows
                    // here and can be removed.
                    @if let Some(key) = &filters.project {
                        @let name = selected.map_or(key.as_str(), |p| p.name.as_str());
                        a href=(href(&filters.with_project(None))) aria-label=(format!("Remove project filter {name}"))
                            class="inline-flex min-h-11 items-center gap-1 rounded-full bg-accent-soft pl-3 pr-2 text-sm text-accent-text md:hidden" {
                            (name) (icon(Icon::Close))
                        }
                    }
                }
            }
            div class="ws-list-full min-h-0 flex-1 overflow-y-auto pb-22 md:pb-0" {
                div hx-sse:connect=(events) hx-swap="none" {}
                (task_rows(items, current, filters))
            }
        }
    }
}

/// Key, title with inline rename, status and the details button; updated live.
pub(crate) fn head_region(task: &TaskDetail, error: Option<&str>) -> Markup {
    let action = format!("/tasks/{}/title", task.key);
    html! {
        header id="task-head" class="border-b border-line bg-panel px-2 py-2 lg:px-4" {
            div class="flex items-center gap-1" {
                a href="/tasks" aria-label="Back to tasks" class={(ICON_BUTTON_CLASS) " md:hidden"} { (icon(Icon::ChevronLeft)) }
                div class="min-w-0 flex-1" x-data="{ editing: false }" {
                    div class="flex items-center gap-2" {
                        span class="shrink-0 font-mono text-sm text-faint" { (task.key) }
                        button type="button" x-show="!editing" "x-on:click"="editing = true; $nextTick(() => $refs.title.focus())"
                            aria-label="Rename task" class="group flex min-h-11 min-w-0 items-center gap-1 rounded-lg px-1 text-left text-base font-semibold hover:bg-sunk lg:min-h-9" {
                            span class="truncate" { (title_text(&task.title, task.title_source)) }
                            span class="text-faint" { (icon(Icon::Pencil)) }
                        }
                        form x-show="editing" x-cloak hx-post=(action) hx-target="#task-head" hx-swap="outerMorph"
                            "x-on:keydown.escape"="editing = false; $refs.title.value = $refs.title.defaultValue"
                            "x-on:title-saved.window"="editing = false"
                            class="flex min-w-0 flex-1 flex-wrap items-center gap-2" {
                            label class="sr-only" for="title" { "Title" }
                            input id="title" name="title" type="text" value=(task.title) maxlength=(TITLE_MAX_CHARS) required x-ref="title"
                                class={(INPUT_CLASS) " min-w-0 flex-1"};
                            button type="submit" class=(action_class(ButtonKind::Primary)) { "Save" }
                            button type="button" "x-on:click"="editing = false; $refs.title.value = $refs.title.defaultValue"
                                class=(action_class(ButtonKind::Ghost)) { "Cancel" }
                        }
                    }
                    (field_error(error))
                }
                (status_badge(task.status))
                button type="button" aria-label="Details" "x-on:click"="details = true; sheet = 'half'" class={(ICON_BUTTON_CLASS) " xl:hidden"} { (icon(Icon::PanelRight)) }
            }
        }
    }
}

/// Whether the task ended (`failed`, `completed`): its queued messages go out with the next run.
fn task_ended(status: &str) -> bool {
    matches!(status, "failed" | "completed")
}

/// Where a queued message stands: held back, forwarded and waiting, or forwarded to a task that
/// ended (`failed`, `completed`), where the next run sends it again.
fn queued_state(message: &TaskMessageRow, task_status: &str) -> QueuedState {
    if message.is_editable() {
        QueuedState::Editable
    } else if task_ended(task_status) {
        QueuedState::NotDelivered
    } else {
        QueuedState::WaitingForDelivery
    }
}

/// The queued message stack above the message input: undelivered messages,
/// oldest first. Each is in one of three [`QueuedState`]s: held back (`Editable`: edit and
/// remove), forwarded and `WaitingForDelivery`, or forwarded to a task that ended
/// (`NotDelivered`: `task_status` is `failed` or `completed`, the next run sends it again). The
/// two forwarded states offer no actions. `error` is shown above the stack. Updated live.
///
/// With messages, a header reads "N queued · `persona` reads them at the next turn" ("N queued ·
/// sent with the next run" when the task ended) above a list of single-line rows that shows at
/// most three of them and scrolls inside itself. Without messages there is no header.
pub(crate) fn pending_region(
    key: &str,
    persona: &str,
    task_status: &str,
    messages: &[TaskMessageRow],
    show_hidden: bool,
    error: Option<&str>,
) -> Markup {
    // A message of only hidden blocks has nothing to show without the option: no row, no count.
    let queued: Vec<(&TaskMessageRow, Vec<(String, bool)>)> = messages
        .iter()
        .map(|m| (m, block_parts(&m.content, show_hidden)))
        .filter(|(_, parts)| !parts.is_empty())
        .collect();
    let padding = if queued.is_empty() && error.is_none() {
        ""
    } else {
        " px-3 pt-3"
    };
    let reader = if task_ended(task_status) {
        "sent with the next run".to_string()
    } else {
        format!("{persona} reads them at the next turn")
    };
    html! {
        div id="pending" class={"flex flex-col gap-2" (padding)} {
            (field_error(error))
            @if !queued.is_empty() {
                p class="text-xs text-muted" { (queued.len()) " queued · " (reader) }
                // Three rows of the queued item (`h-11 lg:h-9`) and two 4 px gaps; the list scrolls beyond.
                div class="flex max-h-[calc(3*2.75rem+2*0.25rem)] flex-col gap-1 overflow-y-auto lg:max-h-[calc(3*2.25rem+2*0.25rem)]" {
                    @for (m, parts) in &queued {
                        (queued_message(key, &m.id.to_string(), m.author.non_human_name(), parts, m.queued_at, queued_state(m, task_status)))
                    }
                }
            }
        }
    }
}

/// The queued stack and the message input below the transcript. One Alpine scope holds the
/// draft and the edit state, so the stack's buttons and the input share them (design D3).
pub(super) fn message_area(
    key: &str,
    persona: &str,
    task_status: TaskStatus,
    runs: &[RunView],
    pending: &[TaskMessageRow],
    show_hidden: bool,
) -> Markup {
    let messages = format!("/tasks/{key}/messages");
    html! {
        // The state lives outside every region the stream swaps, so the draft survives
        // reconnects. The server clears it with `message-queued` (sent) and `message-saved`
        // (edit saved); Cancel restores the draft from before the edit.
        div class="border-t border-line bg-panel"
            x-data="{ draft: '', saved: '', editing: null }"
            "x-on:message-queued.window"="draft = ''"
            "x-on:message-saved.window"="editing = null; draft = ''" {
            (pending_region(key, persona, task_status.as_str(), pending, show_hidden, None))
            form class="p-3" {
                div x-show="editing" x-cloak class="mb-2 flex items-center justify-between gap-2 rounded-lg bg-accent-soft px-3 text-sm text-accent-text" {
                    span { "Editing a queued message" }
                    button type="button" "x-on:click"="editing = null; draft = saved" class=(action_class(ButtonKind::Ghost)) { "Cancel" }
                }
                label for="message" class="sr-only" { "Message" }
                // One rounded box holds the text and the round action, as on the canvas.
                div id="message-box" class="group flex items-end gap-3 rounded-2xl border border-line bg-panel py-2.5 pr-2.5 pl-4 focus-within:border-line-strong" {
                    // The stream replaces `#composer-action`, which drops an `x-ref` inside it:
                    // the shortcut looks up the Send button when the key is pressed.
                    textarea id="message" name="message" rows="2" placeholder="Message the agent" x-model="draft" x-ref="message"
                        "x-on:keydown.ctrl.enter.prevent"=(SEND_SHORTCUT)
                        "x-on:keydown.meta.enter.prevent"=(SEND_SHORTCUT)
                        class="min-h-11 w-full min-w-0 flex-1 resize-none bg-transparent py-2 text-base text-ink placeholder:text-faint focus-visible:outline-none" {}
                    (composer_action(key, interruptible(runs)))
                    button type="button" x-ref="save" x-show="editing" x-cloak aria-label="Save"
                        hx-patch={(messages) "/_"} ":hx-patch"={"'" (messages) "/' + editing"}
                        hx-include="#message" hx-target="#pending" hx-swap="outerMorph"
                        class=(ROUND_PILL_CLASS) { (icon(Icon::Check)) "Save" }
                }
            }
        }
    }
}

/// Whether the message box offers Interrupt: a turn is running, which is the run state `running`
/// (not `awaiting_input`, and not `finalizing`, where the run is ending).
pub(crate) fn interruptible(runs: &[RunView]) -> bool {
    runs.iter().any(|r| r.state == RunState::Running)
}

/// The round action of the message box, updated live: Send; while `interruptible` is true
/// (computed by [`interruptible`]) and the message box is empty it is an Interrupt button instead
/// (design D3, change `interrupt-turn`). The box is empty while the textarea shows its
/// placeholder, so this is CSS only and needs no draft state. "No turn is running" appears in
/// the `#pending` region.
pub(crate) fn composer_action(key: &str, interruptible: bool) -> Markup {
    let messages = format!("/tasks/{key}/messages");
    let interrupt = format!("/tasks/{key}/interrupt");
    let send_class = if interruptible {
        format!("{ROUND_ACTION_CLASS} group-has-[textarea:placeholder-shown]:hidden")
    } else {
        ROUND_ACTION_CLASS.to_string()
    };
    html! {
        span id="composer-action" class="contents" {
            @if interruptible {
                button type="button" aria-label="Interrupt" hx-post=(interrupt) hx-target="#pending" hx-swap="outerMorph"
                    class="hidden size-11 shrink-0 items-center justify-center rounded-full bg-ink text-panel hover:brightness-95 group-has-[textarea:placeholder-shown]:inline-flex"
                    x-show="!editing" { (icon(Icon::Stop)) }
            }
            // Drop a second send while one is in flight: it would repeat the draft, which only
            // the first response clears. The queue lives on `#message` because the stream
            // replaces this button while the request is in flight (sending requeues the task).
            button type="button" x-show="!editing" aria-label="Send"
                hx-post=(messages) hx-include="#message" hx-target="#pending" hx-swap="outerMorph"
                hx-sync="#message:drop"
                class=(send_class) { (icon(Icon::Send)) }
        }
    }
}

/// What the cancel confirmation tells about the task.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct CancelNote {
    /// Number of the active run, if there is one.
    pub run: Option<i64>,
    /// Undelivered messages that cancelling discards.
    pub queued: usize,
    /// Whether the active run can be stopped: it is not ending already.
    pub stoppable: bool,
}

impl CancelNote {
    pub(crate) fn new(runs: &[RunView], pending: &[TaskMessageRow]) -> Self {
        Self {
            run: runs
                .iter()
                .find(|r| !r.state.is_terminal())
                .map(|r| r.number),
            queued: pending.len(),
            stoppable: runs.iter().any(|r| r.state.can_stop()),
        }
    }

    /// "Stops run #2, discards 2 queued messages and closes the pending requests of this task."
    fn effects(&self) -> String {
        let mut parts = Vec::new();
        if let Some(run) = self.run {
            parts.push(format!("stops run #{run}"));
        }
        match self.queued {
            0 => {}
            1 => parts.push("discards 1 queued message".to_string()),
            n => parts.push(format!("discards {n} queued messages")),
        }
        parts.push("closes the pending requests of this task".to_string());
        let last = parts.pop().unwrap_or_default();
        let mut text = if parts.is_empty() {
            last
        } else {
            format!("{} and {last}", parts.join(", "))
        };
        if let Some(first) = text.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        text.push('.');
        text
    }
}

/// Status line, links, project, persona and branch; updated live. It also renders the narrow
/// strip the details collapse to: status dot, links to the pull request and issue (icons with
/// tooltips) and the control that shows the details again. `error` is shown below the details
/// list.
pub(crate) fn details_region(
    task: &TaskDetail,
    cancel: &CancelNote,
    resume: &ResumeNote,
    error: Option<&str>,
) -> Markup {
    let branch = task.branch.clone();
    let pr = task.pr_url.as_deref().map(|url| {
        (
            url,
            format!(
                "Pull request{}",
                task.pr_number.map(|n| format!(" #{n}")).unwrap_or_default()
            ),
        )
    });
    let issue = issue_url(task)
        .zip(task.issue_number)
        .map(|(url, number)| (url, format!("Issue #{number}")));
    // "Now": the resume time while the task waits for one, else the status line.
    let (now_tone, now_text) = match resume.at {
        Some(at) => (
            Tone::Limited,
            html! {
                "Resumes at " time datetime=(at.to_rfc3339()) title=(format_time(at)) { (clock_label(at, Utc::now())) }
                " · " (resume.reason)
            },
        ),
        None => (
            Tone::of_status(task.status),
            html! { @if task.status_line.is_empty() { (status_name(task.status)) } @else { (task.status_line) } },
        ),
    };
    let mut properties = vec![
        ("Status", status_select(task, cancel)),
        (
            "Project",
            html! { a href={"/projects/" (task.project_key)} class="text-accent-text underline" { (task.project_name) " " (chip(&task.project_key)) } },
        ),
        (
            "Persona",
            html! { a href="/personas" class="text-accent-text underline" { (task.persona_name) " " span class="font-normal text-muted" { "· " (harness_label(&task.persona_harness)) } } },
        ),
        (
            "Branch",
            html! { span class="font-mono text-xs" { (branch) } },
        ),
        ("Created by", super::dependencies::creator(task)),
    ];
    if let (Some(key), Some(title)) = (&task.parent_key, &task.parent_title) {
        properties.push((
            "Parent",
            html! { a href={"/tasks/" (key)} class="text-accent-text hover:underline" { (key) } " " span class="font-normal text-muted" { (title) } },
        ));
    }
    if resume.count > 0 {
        properties.push((
            "Resumes",
            html! { (resume.count) " of " (resume.max) " automatic" },
        ));
    }
    html! {
        div id="task-details" class="md:shrink-0" {
            div class="ws-strip flex-col items-center gap-3 p-2" {
                button type="button" class=(ICON_BUTTON_CLASS) aria-label="Show details" title="Show details" "x-on:click"="setCollapsed(false)" { (icon(Icon::PanelRight)) }
                span class={"size-3 rounded-full " (Tone::of_status(task.status).dot_class())}
                    role="img" aria-label=(format!("Status: {}", status_name(task.status))) title=(status_name(task.status)) {}
                @if let Some((url, label)) = &pr { (strip_link(url, label, Icon::PullRequest)) }
                @if let Some((url, label)) = &issue { (strip_link(url, label, Icon::Issue)) }
            }
            div class="ws-details-full" {
                (section("Now", html! {
                    p class="flex items-center gap-2 font-medium" {
                        span aria-hidden="true" class={"size-2 shrink-0 rounded-full " (now_tone.dot_class())} {}
                        span class="break-words" { (now_text) }
                    }
                }))
                (section("Links", html! {
                    @if pr.is_some() || issue.is_some() {
                        div class="flex flex-col gap-2 text-sm" {
                            @if let Some(url) = &task.pr_url {
                                div class="flex items-center gap-2" {
                                    (external_link(url, &format!("PR{}", task.pr_number.map(|n| format!(" #{n}")).unwrap_or_default()), task.pr_title.as_deref(), Icon::PullRequest))
                                    @if task.pr_attached { (attached_badge()) }
                                }
                            }
                            @if let Some((url, label)) = &issue { (external_link(url, label, None, Icon::Issue)) }
                        }
                    } @else {
                        p class="text-sm text-muted" { "No pull request or issue yet." }
                    }
                }))
                div class="border-b border-line px-5 py-4" {
                    (definition_list(&properties))
                    @if error.is_some() { div class="mt-3" { (field_error(error)) } }
                }
                (description_section(&task.description))
            }
        }
    }
}

/// The collapsible description of the details: closed, only the header with a chevron; open, the
/// first three lines of the text, the whole text as tooltip. Nothing for an empty description.
fn description_section(description: &str) -> Markup {
    html! {
        @if !description.is_empty() {
            details class="group border-b border-line px-5 py-4 text-sm" {
                summary class="flex min-h-11 cursor-pointer list-none items-center gap-1 text-xs font-medium uppercase tracking-wide text-faint [&::-webkit-details-marker]:hidden" {
                    span class="group-open:hidden" { (icon(Icon::ChevronRight)) }
                    span class="hidden group-open:inline" { (icon(Icon::ChevronDown)) }
                    "Description"
                }
                div class="line-clamp-3 whitespace-pre-wrap break-words text-muted" title=(description) { (description) }
            }
        }
    }
}

/// The status of the details list: a select like the project filter whose menu shows the current
/// status (checked), "Mark done" (disabled while the task works), "Reset workspace…" (disabled
/// while a run is active) and "Cancel task…"; the last two open their confirmation. A completed
/// task offers only "Reset workspace…". Opening the menu makes the phone sheet full height, so
/// the menu below the select fits on the screen.
fn status_select(task: &TaskDetail, cancel: &CancelNote) -> Markup {
    let name = status_name(task.status);
    let closed = !task.status.can_mark_done() && !task.status.can_cancel();
    let active_run = cancel.run.is_some();
    let done = format!("/tasks/{}/done", task.key);
    let stop = format!("/tasks/{}/stop", task.key);
    let item = "flex min-h-11 w-full items-center rounded-lg px-2 text-left text-sm font-normal lg:min-h-9";
    html! {
        div class="relative" x-data="{ open: false }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="true" ":aria-expanded"="open" "x-on:click"="open = !open; if (open) sheet = 'full'"
                class="inline-flex min-h-11 items-center gap-1 rounded-[10px] border border-line bg-panel px-3 text-sm font-medium hover:bg-sunk lg:min-h-8" {
                (name) (icon(Icon::ChevronDown))
            }
            div x-show="open" x-cloak class="absolute left-0 z-20 mt-1 w-48 rounded-xl border border-line bg-panel p-2 shadow-lg" {
                ul {
                    li class="flex min-h-11 items-center justify-between gap-2 rounded-lg px-2 text-sm font-medium text-accent-text lg:min-h-9" aria-current="true" {
                        span { (name) } (icon(Icon::Check))
                    }
                    @if !closed {
                        li {
                            @if task.status.can_mark_done() {
                                button type="button" hx-post=(done) hx-target="#task-details" hx-swap="outerMorph"
                                    class={(item) " text-ink hover:bg-sunk"} { "Mark done" }
                            } @else {
                                button type="button" disabled class={(item) " cursor-not-allowed text-faint"} { "Mark done" }
                            }
                        }
                    }
                    @if cancel.stoppable {
                        li {
                            button type="button" hx-post=(stop) hx-swap="none"
                                class={(item) " text-ink hover:bg-sunk"} { "Stop run" }
                        }
                    }
                    li {
                        @if active_run {
                            button type="button" disabled
                                class="flex min-h-11 w-full cursor-not-allowed flex-col items-start justify-center rounded-lg px-2 py-1 text-left text-sm font-normal text-faint lg:min-h-9" {
                                "Reset workspace…"
                                span class="text-xs text-muted" { "Stop the run first" }
                            }
                        } @else {
                            button type="button" "x-on:click"="open = false; $refs.resetDialog.showModal()"
                                class={(item) " text-ink hover:bg-sunk"} { "Reset workspace…" }
                        }
                    }
                    @if task.status.can_cancel() {
                        li {
                            button type="button" "x-on:click"="open = false; $refs.cancelDialog.showModal()"
                                class={(item) " text-danger hover:bg-sunk"} { "Cancel task…" }
                        }
                    }
                }
            }
            @if !active_run { (reset_dialog(task)) }
            @if task.status.can_cancel() { (cancel_dialog(task, cancel)) }
        }
    }
}

/// The confirmation of "Reset workspace…", built like [`cancel_dialog`]; it names what is lost
/// (design D2).
fn reset_dialog(task: &TaskDetail) -> Markup {
    let action = format!("/tasks/{}/reset-workspace", task.key);
    html! {
        dialog x-ref="resetDialog" aria-labelledby="reset-title"
            "x-on:click"="$event.target === $el && $el.close()" class=(DIALOG_CLASS) {
            div class="rounded-2xl border border-line bg-panel p-5 shadow-lg" {
                h2 id="reset-title" class="text-lg font-semibold" { "Reset workspace of " (task.key) "?" }
                p class="mt-3 text-sm font-normal max-md:text-[15px]" {
                    "This deletes the task's volume with its clone and build caches. The next run clones the repository again and restores the WIP snapshot if there is one."
                }
                p class="mt-3 text-sm font-normal max-md:text-[15px]" {
                    "Lost for good: commits that were not pushed and uncommitted changes without a WIP snapshot."
                }
                div class="mt-5 flex flex-col-reverse gap-2 md:flex-row md:justify-end" {
                    button type="button" "x-on:click"="$refs.resetDialog.close()" class=(action_class(ButtonKind::Secondary)) { "Keep workspace" }
                    button type="button" hx-post=(action) hx-target="#task-details" hx-swap="outerMorph"
                        "x-on:click"="$refs.resetDialog.close()" class=(action_class(ButtonKind::Danger)) { "Reset workspace" }
                }
            }
        }
    }
}

/// The confirmation of "Cancel task…": a native modal `<dialog>`, centered from `md` and anchored
/// to the bottom with stacked full-width buttons (danger first) on a phone. It belongs to the
/// status menu's Alpine scope (`$refs.cancelDialog`).
fn cancel_dialog(task: &TaskDetail, cancel: &CancelNote) -> Markup {
    let action = format!("/tasks/{}/cancel", task.key);
    let kept = [
        task.pr_number.map(|n| format!("PR #{n}")),
        task.issue_number.map(|n| format!("issue #{n}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" and ");
    html! {
        dialog x-ref="cancelDialog" aria-labelledby="cancel-title"
            "x-on:click"="$event.target === $el && $el.close()" class=(DIALOG_CLASS) {
            div class="rounded-2xl border border-line bg-panel p-5 shadow-lg" {
                h2 id="cancel-title" class="text-lg font-semibold" { "Cancel " (task.key) "?" }
                p class="mt-3 text-sm" {
                    (cancel.effects())
                    @if kept.is_empty() { " The pull request and the issue stay as they are." }
                    @else { " " (kept) " stay as they are." }
                }
                p class="mt-3 text-sm text-muted" { "A new message reopens the task." }
                div class="mt-5 flex flex-col-reverse gap-2 md:flex-row md:justify-end" {
                    button type="button" "x-on:click"="$refs.cancelDialog.close()" class=(action_class(ButtonKind::Secondary)) { "Keep task" }
                    button type="button" hx-post=(action) hx-target="#task-details" hx-swap="outerMorph"
                        "x-on:click"="$refs.cancelDialog.close()" class=(action_class(ButtonKind::Danger)) { "Cancel task" }
                }
            }
        }
    }
}

/// An icon link of the collapsed details strip; the label is its name and tooltip.
fn strip_link(url: &str, label: &str, glyph: Icon) -> Markup {
    if url.starts_with("https://") || url.starts_with("http://") {
        html! {
            a href=(url) aria-label=(label) title=(label) rel="noopener noreferrer" target="_blank"
                class={(ICON_BUTTON_CLASS) " text-accent-text!"} { (icon(glyph)) }
        }
    } else {
        html! {}
    }
}

/// The runs of the task as cards (`#n`, status, start time, one line of detail and the token
/// table per model); updated live.
pub(crate) fn runs_region(task_key: &str, runs: &[RunView], persona_model: Option<&str>) -> Markup {
    html! {
        div id="runs" class="ws-details-full ws-runs" {
            @if !runs.is_empty() {
                // Closed by default; `open` is in `morphIgnore`, so live updates keep the reader's choice.
                details class="group space-y-2.5 px-5 py-4 md:flex md:min-h-0 md:flex-1 md:flex-col" {
                    summary class="flex min-h-11 cursor-pointer list-none items-center gap-1 text-xs font-medium uppercase tracking-wide text-faint lg:min-h-8 [&::-webkit-details-marker]:hidden" {
                        span class="group-open:hidden" { (icon(Icon::ChevronRight)) }
                        span class="hidden group-open:inline" { (icon(Icon::ChevronDown)) }
                        "Runs"
                        span class="ml-1 font-mono normal-case" { (runs.len()) }
                    }
                    ul class="space-y-2 md:min-h-0 md:flex-1 md:overflow-y-auto" {
                        // The id keeps a card (and an open disclosure in it) with its run when morphing.
                        @for run in runs { li id={"run-card-" (run.id)} class="space-y-2 rounded-lg border border-line p-3 text-sm" { (run_card(task_key, run, persona_model)) } }
                    }
                }
            }
        }
    }
}

/// Longest detail shown without a disclosure, in characters.
const DETAIL_LINE_CHARS: usize = 80;

fn run_card(task_key: &str, run: &RunView, persona_model: Option<&str>) -> Markup {
    html! {
        div class="flex items-center gap-2" {
            // The divider of the run in the transcript; the click also closes the details where
            // they overlay the transcript (no effect from 1280 px, where they are a column).
            a href={"#run-" (run.number)} "x-on:click"="details = false"
                class="font-mono text-sm text-accent-text hover:underline" { "#" (run.number) }
            (tone_badge(Tone::of_run(run.state), run.state.as_str()))
            @if let Some(outcome) = &run.outcome { span class="text-xs text-muted" { (outcome_text(outcome)) } }
            time datetime=(run.started_at.to_rfc3339()) class="ml-auto font-mono text-xs text-faint" { (short_time(run.started_at)) }
            // A run that is already ending cannot be stopped again.
            @if run.state.can_stop() {
                button type="button" hx-post={"/tasks/" (task_key) "/stop"} hx-swap="none"
                    class="inline-flex min-h-11 items-center justify-center rounded-[10px] border border-line bg-panel px-3 text-sm font-medium text-ink hover:bg-sunk lg:min-h-8" { "Stop run" }
            }
        }
        @if let Some(retry) = run.retry_after { p class="text-xs text-status-limited-fg" { "retry at " (format_time(retry)) } }
        @if !run.detail.is_empty() { (run_detail(&run.detail)) }
        @if let Some(ended) = run.ended_at { p class="font-mono text-xs text-faint" { "ended " (format_time(ended)) } }
        @if run.usage.is_empty() && run.unknown_usage_requests == 0 {
            p class="text-xs text-muted" { (persona_model.unwrap_or("default model")) " · no token usage recorded" }
        }
        // Per model: the name on its own line, then its counts under the column header; the
        // exact values are the titles.
        @for u in &run.usage {
            div {
                p class="truncate pt-1 font-mono text-xs text-muted" title=(u.model_id) { (u.model_id) }
                table class="w-full table-fixed border-collapse font-mono text-xs text-muted" {
                    thead { tr class="text-faint" {
                        th scope="col" class="text-center font-medium" { "in" }
                        th scope="col" class="text-center font-medium" { "out" }
                        th scope="col" class="text-center font-medium" title="read / write" { "cache" }
                    } }
                    tbody { tr {
                        td class="text-center" title=(u.input_tokens) { (compact_count(u.input_tokens)) }
                        td class="text-center" title=(u.output_tokens) { (compact_count(u.output_tokens)) }
                        td class="text-center" title={(u.cache_read_tokens) " / " (u.cache_write_tokens)} {
                            (compact_count(u.cache_read_tokens)) " / " (compact_count(u.cache_write_tokens))
                        }
                    } }
                }
            }
        }
        @if run.unknown_usage_requests > 0 {
            p class="pt-1 text-xs text-muted" {
                (run.unknown_usage_requests)
                @if run.unknown_usage_requests == 1 { " request without usage" } @else { " requests without usage" }
            }
        }
    }
}

/// A token count for a narrow column: exact below 1,000, then `9.4k`, `182k`, `1.2M` and `12M`
/// (cut, not rounded, so `999999` never reads `1000k`).
fn compact_count(n: i64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 10_000 {
        format!("{}.{}k", n / 1_000, n % 1_000 / 100)
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else if n < 10_000_000 {
        format!("{}.{}M", n / 1_000_000, n % 1_000_000 / 100_000)
    } else {
        format!("{}M", n / 1_000_000)
    }
}

/// One line of the detail (a failure reason or error); longer or multi-line text keeps its full
/// form in a disclosure.
fn run_detail(detail: &str) -> Markup {
    let first = detail.lines().next().unwrap_or_default();
    if detail.lines().count() <= 1 && detail.chars().count() <= DETAIL_LINE_CHARS {
        return html! { p class="break-words text-muted" { (detail) } };
    }
    html! {
        details class="text-muted" {
            summary class="cursor-pointer truncate" { (first) }
            p class="mt-1 whitespace-pre-wrap break-words" { (detail) }
        }
    }
}

/// The details panel: bottom sheet, slide-over or column depending on the width.
pub(super) fn details_column(
    task: &TaskDetail,
    deps: &df_db::task_views::Dependencies,
    runs: &[RunView],
    cancel: &CancelNote,
    resume: &ResumeNote,
    artifacts: &[crate::artifacts::ArtifactCard],
) -> Markup {
    let tabbed = !artifacts.is_empty();
    details_panel_with_heading(
        "Details",
        crate::artifacts::details_heading(artifacts.len(), false),
        html! {
            // The stylesheet hides this panel while the Artifacts tab is open.
            div id="panel-overview" role=[tabbed.then_some("tabpanel")] aria-labelledby=[tabbed.then_some("tab-overview")] class="ws-overview" {
                (details_region(task, cancel, resume, None)) (super::dependencies::dependencies_region(deps, task.status)) (runs_region(&task.key, runs, task.persona_model.as_deref()))
            }
            (crate::artifacts::tab_panel(&task.key, artifacts))
        },
    )
}

/// Values of the composer form.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub(crate) struct TaskForm {
    pub description: String,
    pub project: String,
    pub persona: String,
    pub issue_number: String,
    /// An existing branch to work on instead of a new `factory/task/<KEY>` branch.
    pub branch: String,
}

/// Projects and personas to choose from, as `(id, label)`.
pub(crate) struct Choices {
    pub projects: Vec<(String, String)>,
    pub personas: Vec<(String, String)>,
}

/// The composer form. The first project/persona of the lists apply unless the values or the
/// browser's last choice say otherwise (`x-init`).
pub(super) fn composer_form(values: &TaskForm, errors: &FormErrors, choices: &Choices) -> Markup {
    let project = if values.project.is_empty() {
        choices
            .projects
            .first()
            .map(|(id, _)| id.as_str())
            .unwrap_or_default()
    } else {
        values.project.as_str()
    };
    let persona = if values.persona.is_empty() {
        choices
            .personas
            .first()
            .map(|(id, _)| id.as_str())
            .unwrap_or_default()
    } else {
        values.persona.as_str()
    };
    html! {
        form id="composer-form" class="space-y-2" hx-post="/tasks" hx-target="this" hx-swap="outerHTML"
            x-data={"{ issue: '" (values.issue_number.trim().chars().filter(char::is_ascii_digit).collect::<String>()) "', branch: " (json_string(values.branch.trim())) ", linking: false }"}
            x-init="(() => { try { const d = JSON.parse(localStorage.getItem('df.composer') ?? '{}'); const pick = (el, v) => { if (!el.dataset.explicit && v && [...el.options].some(o => o.value === v)) el.value = v }; pick($refs.project, d.project); pick($refs.persona, d.persona) } catch (e) {} })()"
            "x-on:submit"="(() => { try { localStorage.setItem('df.composer', JSON.stringify({ project: $refs.project.value, persona: $refs.persona.value })) } catch (e) {} })()"
            "x-on:keydown.ctrl.enter.prevent"="$el.requestSubmit()" "x-on:keydown.meta.enter.prevent"="$el.requestSubmit()" {
            (form_error(errors))
            div class="flex flex-wrap items-center gap-2 px-1" {
                (choice_select(CHIP_SELECT, "Project", "project", &choices.projects, project, !values.project.is_empty(), errors))
                span x-show="issue" x-cloak class="inline-flex items-center gap-1 rounded-[10px] bg-accent-soft pl-3 text-xs text-accent-text" {
                    "Issue " span class="font-mono" { "#" span x-text="issue" {} }
                    button type="button" aria-label="Remove linked issue" "x-on:click"="issue = ''" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
                }
                span x-show="branch" x-cloak class="inline-flex items-center gap-1 rounded-[10px] bg-accent-soft pl-3 text-xs text-accent-text" {
                    (icon(Icon::Git)) "Branch " span class="font-mono" x-text="branch" {}
                    button type="button" aria-label="Remove existing branch" "x-on:click"="branch = ''" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
                }
                div class="ml-auto" { (dialog_close()) }
            }
            div class="relative rounded-2xl border border-line bg-panel p-3 shadow-lg focus-within:border-line-strong"
                "x-on:click.outside"="linking = false"
                "x-on:keydown.escape"="if (linking) { $event.preventDefault(); $event.stopPropagation(); linking = false }" {
                label for="description" class="sr-only" { "Description" }
                textarea id="description" name="description" rows="5" placeholder="What should the agent do?"
                    class="w-full resize-none bg-transparent text-base text-ink placeholder:text-faint focus-visible:outline-none" autofocus { (values.description) }
                (field_error(errors.get("description")))
                // The branch error stays visible with the popover closed.
                (field_error(errors.get("branch")))
                // The options: a popover below the card (inline on phones, where the sheet
                // leaves no room below it) with the linked issue, then the existing branch. The
                // field names are `issue_number` and `branch`.
                div x-show="linking" x-cloak role="group" aria-label="Linked issue and branch"
                    class="absolute top-[calc(100%-8px)] left-3 z-10 w-[280px] rounded-xl border border-line bg-panel p-3 shadow-lg max-md:static max-md:mt-2 max-md:w-full" {
                    label for="issue_number" class="text-[13px] font-medium text-muted" { "Linked issue" }
                    div class="mt-1.5 flex min-h-10 items-center overflow-hidden rounded-[10px] border border-line-strong focus-within:ring-1 focus-within:ring-line-strong" {
                        span aria-hidden="true" class="pr-1 pl-3 font-mono text-faint" { "#" }
                        input id="issue_number" name="issue_number" type="text" inputmode="numeric" x-model="issue"
                            class="min-w-0 flex-1 bg-transparent py-2 pr-3 font-mono text-sm text-ink focus-visible:outline-none";
                    }
                    p class="mt-1.5 text-xs text-faint" {
                        "An existing issue in "
                        span x-text="($refs.project.options[$refs.project.selectedIndex]?.text ?? '').split(' - ').slice(1).join(' - ')" { (project_name(choices, project)) }
                        "'s tracker that this task works on."
                    }
                    (field_error(errors.get("issue_number")))
                    label for="branch" class="mt-3 block text-[13px] font-medium text-muted" { "Existing branch" }
                    input id="branch" name="branch" type="text" x-model="branch" spellcheck="false" autocomplete="off"
                        class="mt-1.5 min-h-10 w-full rounded-[10px] border border-line-strong bg-transparent px-3 py-2 font-mono text-sm text-ink focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-line-strong";
                    p class="mt-1.5 text-xs text-faint" {
                        "Work on this branch instead of a new factory/task branch. An open pull request from it is attached."
                    }
                }
                div class="mt-2 flex items-center gap-2" {
                    button type="button" "x-on:click"="linking = !linking" ":aria-expanded"="linking"
                        ":class"="linking ? 'border-accent-text bg-accent-soft text-accent-text' : 'border-line text-muted hover:bg-sunk'"
                        aria-label="Task options" title="Task options"
                        class="inline-flex size-11 shrink-0 items-center justify-center rounded-full border lg:size-9" { (icon(Icon::Plus)) }
                    div class="ml-auto" { (choice_select(PERSONA_SELECT, "Assignee", "persona", &choices.personas, persona, !values.persona.is_empty(), errors)) }
                    button type="submit" aria-label="Create task" title="Create task"
                        class=(ROUND_ACTION_CLASS) { (icon(Icon::ArrowUp)) }
                }
            }
        }
    }
}

/// `value` as a JSON (and so JavaScript) string literal, for an Alpine expression.
fn json_string(value: &str) -> String {
    // A string always serializes.
    serde_json::to_string(value).unwrap_or_else(|_unreachable| "\"\"".to_owned())
}

/// The composer as a dialog without frame (bottom sheet on phones), or a hint when nothing can
/// be chosen yet.
pub(super) fn composer_dialog(values: &TaskForm, errors: &FormErrors, choices: &Choices) -> Markup {
    if choices.projects.is_empty() || choices.personas.is_empty() {
        let body = html! {
            div class="rounded-2xl border border-line bg-panel p-4 shadow-lg" {
                div class="flex justify-end" { (dialog_close()) }
                p class="py-2 text-sm text-muted" { "Create a project and a persona first." }
            }
        };
        return dialog_with("composer", "New task", DialogStyle::Bare, body);
    }
    dialog_with(
        "composer",
        "New task",
        DialogStyle::Bare,
        composer_form(values, errors, choices),
    )
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn short_time(time: DateTime<Utc>) -> String {
    time.format("%b %d %H:%M").to_string()
}

/// A task status as shown: without underscores.
pub(super) fn status_name(status: TaskStatus) -> String {
    status.as_str().replace('_', " ")
}

fn status_of(item: &str) -> Option<TaskStatus> {
    item.parse().ok()
}

/// The title as shown in lists and headers: a provisional title is italic.
fn title_text(title: &str, source: TitleSource) -> Markup {
    html! {
        @if source == TitleSource::Placeholder {
            span class="italic text-muted" title="Provisional title, the persona or you can set a better one" { (title) }
        } @else { (title) }
    }
}

fn format_time(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn external_link(url: &str, label: &str, subtitle: Option<&str>, glyph: Icon) -> Markup {
    // Forge URLs are rendered as links only for http(s); anything else stays inert text.
    if url.starts_with("https://") || url.starts_with("http://") {
        html! {
            a href=(url) class="flex min-h-11 items-center gap-2.5 rounded-lg border border-line px-3 text-ink hover:bg-sunk" rel="noopener noreferrer" target="_blank" {
                (icon(glyph))
                span class="flex min-w-0 flex-col" {
                    span class="font-medium" { (label) }
                    @if let Some(subtitle) = subtitle { span class="truncate text-xs text-muted" { (subtitle) } }
                }
            }
        }
    } else {
        html! { span { (label) } }
    }
}

fn issue_url(task: &TaskDetail) -> Option<String> {
    let number = task.issue_number?;
    let base = task.issue_tracker_url.as_deref().unwrap_or(&task.repo_url);
    let base = base.trim_end_matches('/').trim_end_matches(".git");
    Some(format!("{base}/issues/{number}"))
}

fn outcome_text(outcome: &str) -> String {
    outcome.replace('_', " ")
}

/// The name of the project with this id, from the `KEY - Name` labels of the choices.
fn project_name<'a>(choices: &'a Choices, id: &str) -> &'a str {
    choices
        .projects
        .iter()
        .find(|(value, _)| value == id)
        .and_then(|(_, label)| label.split_once(" - "))
        .map_or("this project", |(_, name)| name)
}

fn choice_select(
    class: &str,
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    explicit: bool,
    errors: &FormErrors,
) -> Markup {
    html! {
        div {
            label for=(name) class="sr-only" { (label) }
            // An explicit choice (query or submitted form) wins over the browser's last choice.
            select id=(name) name=(name) x-ref=(name) data-explicit=[explicit.then_some("true")] class=(class) {
                @for (value, text) in options { option value=(value) selected[value == selected] { (text) } }
            }
            (field_error(errors.get(name)))
        }
    }
}

#[cfg(test)]
mod tests;
