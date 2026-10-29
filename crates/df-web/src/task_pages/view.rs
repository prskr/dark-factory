//! Markup of the task workspace: list, transcript header, details, runs, pending messages and
//! the composer. Handlers live in the parent module; the live-update regions (`#task-head`,
//! `#task-details`, `#runs`, `#pending`, `#task-rows`) are re-rendered by `crate::sse`.

use chrono::{DateTime, Utc};
use df_core::{Principal, StatusSet, TITLE_MAX_CHARS, TaskKey, TaskStatus, TitleSource};
use df_db::TaskMessageRow;
use df_db::task_views::{ProjectFilterItem, RunView, TaskDetail, TaskListItem};
use maud::{Markup, html};

use crate::components::QueuedState;
use crate::components::forms::{INPUT_CLASS, MenuItem};
use crate::components::transcript::queued_message;
use crate::components::{
    ButtonKind, DialogStyle, EmptyPanel, EmptySize, FormErrors, ICON_BUTTON_CLASS, Icon, Tone,
    action_class, chip, definition_list, details_panel, dialog_close, dialog_with, field_error,
    filter_chip, form_error, harness_label, icon, profile_menu, search_menu, section, status_badge,
    tone_badge,
};
use crate::components::{ROUND_ACTION_CLASS, ROUND_PILL_CLASS};
use crate::transcript::block_parts;

const CHIP_SELECT: &str =
    "min-h-11 rounded-[10px] border border-line bg-panel px-3 text-sm text-muted lg:min-h-8";
/// The persona picker of the composer sits in the card without a border of its own.
const PERSONA_SELECT: &str =
    "min-h-11 rounded-full bg-transparent px-3 text-sm text-ink lg:min-h-8";

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
}

impl Filters {
    pub(crate) fn new(status: Option<&str>, project: Option<&str>) -> Self {
        Self {
            status: StatusFilter::parse(status),
            project: project.filter(|p| !p.is_empty()).map(str::to_string),
        }
    }

    pub(crate) fn to_db(&self) -> df_db::task_views::TaskFilter {
        df_db::task_views::TaskFilter {
            group: self.status.group(),
            project_key: self.project.clone(),
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
                li id={"row-" (t.key)} {
                    a href=(href) hx-get=(href) hx-target="#ws-detail" hx-select="#ws-detail" hx-swap="outerHTML" hx-push-url="true"
                        aria-current=[(current == Some(t.key.as_str())).then_some("page")]
                        "x-on:click"="document.body.dataset.view = 'task'; details = false; $root.querySelectorAll('#task-rows [aria-current]').forEach(e => e.removeAttribute('aria-current')); $el.setAttribute('aria-current', 'page')"
                        class="block min-h-11 border-b border-line border-l-[3px] border-l-transparent py-3.5 pr-5 pl-[17px] hover:bg-sunk aria-[current=page]:border-l-accent aria-[current=page]:bg-accent-soft" {
                        div class="flex items-center gap-2" {
                            span class="font-mono text-xs text-faint" { (t.key) }
                            @if let Some(status) = status_of(&t.status) { (status_badge(status)) } @else { (tone_badge(Tone::Queued, &t.status)) }
                            time datetime=(t.updated_at.to_rfc3339()) class="ml-auto font-mono text-xs text-faint" { (short_time(t.updated_at)) }
                        }
                        div title=(t.title) class={"mt-0.5 truncate text-[15px] font-medium" @if t.title_source == TitleSource::Placeholder { " italic text-muted" }} { (t.title) }
                        @if !t.status_line.is_empty() { div class="mt-0.5 truncate text-xs text-muted" { (t.status_line) } }
                        div class="mt-1 text-xs text-faint" { (t.project_name) " · " (t.persona_name) }
                    }
                }
            }
            @if items.is_empty() { li { (empty_list(filters)) } }
        }
    }
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
        StatusFilter::Done => "Completed and failed tasks show up here.",
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
/// `user` is the signed-in board member, shown as the profile menu on phones.
pub(crate) fn list_column(
    items: &[TaskListItem],
    projects: &[ProjectFilterItem],
    filters: &Filters,
    current: Option<&str>,
    user: &str,
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
    let events = format!("/tasks/events{}", filters.query());
    html! {
        section class="ws-list" aria-label="Tasks" {
            div class="space-y-3 border-b border-line p-4" {
                div class="flex items-center justify-between gap-2" {
                    h1 class="text-lg font-semibold" { "Tasks" }
                    div class="hidden md:block" { (search_menu(menu_label, &menu)) }
                    (profile_menu(user))
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
            div class="min-h-0 flex-1 overflow-y-auto" {
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

/// The queued message stack above the message input: undelivered messages of board members,
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
        .filter(|m| matches!(m.author, Principal::Human { .. }))
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
                        (queued_message(key, &m.id.to_string(), parts, m.queued_at, queued_state(m, task_status)))
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
    task_status: &str,
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
            (pending_region(key, persona, task_status, pending, show_hidden, None))
            form class="p-3" {
                div x-show="editing" x-cloak class="mb-2 flex items-center justify-between gap-2 rounded-lg bg-accent-soft px-3 text-sm text-accent-text" {
                    span { "Editing a queued message" }
                    button type="button" "x-on:click"="editing = null; draft = saved" class=(action_class(ButtonKind::Ghost)) { "Cancel" }
                }
                label for="message" class="sr-only" { "Message" }
                // One rounded box holds the text and the round action, as on the canvas.
                div id="message-box" class="flex items-end gap-3 rounded-2xl border border-line bg-panel py-2.5 pr-2.5 pl-4 focus-within:border-line-strong" {
                    textarea id="message" name="message" rows="2" placeholder="Message the agent" x-model="draft" x-ref="message"
                        "x-on:keydown.ctrl.enter.prevent"="(editing ? $refs.save : $refs.send).click()"
                        "x-on:keydown.meta.enter.prevent"="(editing ? $refs.save : $refs.send).click()"
                        class="min-h-11 w-full min-w-0 flex-1 resize-none bg-transparent py-2 text-base text-ink placeholder:text-faint focus-visible:outline-none" {}
                    button type="button" x-ref="send" x-show="!editing" aria-label="Send"
                        hx-post=(messages) hx-include="#message" hx-target="#pending" hx-swap="outerMorph"
                        class=(ROUND_ACTION_CLASS) { (icon(Icon::Send)) }
                    button type="button" x-ref="save" x-show="editing" x-cloak aria-label="Save"
                        hx-patch={(messages) "/_"} ":hx-patch"={"'" (messages) "/' + editing"}
                        hx-include="#message" hx-target="#pending" hx-swap="outerMorph"
                        class=(ROUND_PILL_CLASS) { (icon(Icon::Check)) "Save" }
                }
            }
        }
    }
}

/// Status line, links, project, persona and branch; updated live. It also renders the narrow
/// strip the details collapse to: status dot, links to the pull request and issue (icons with
/// tooltips) and the control that shows the details again.
pub(crate) fn details_region(task: &TaskDetail) -> Markup {
    let branch = task
        .key
        .parse::<TaskKey>()
        .map(|k| k.task_branch())
        .unwrap_or_default();
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
                        span aria-hidden="true" class={"size-2 shrink-0 rounded-full " (Tone::of_status(task.status).dot_class())} {}
                        @if task.status_line.is_empty() { (status_name(task.status)) } @else { span class="break-words" { (task.status_line) } }
                    }
                }))
                (section("Links", html! {
                    @if pr.is_some() || issue.is_some() {
                        div class="flex flex-col gap-2 text-sm" {
                            @if let Some(url) = &task.pr_url {
                                (external_link(url, &format!("PR{}", task.pr_number.map(|n| format!(" #{n}")).unwrap_or_default()), task.pr_title.as_deref(), Icon::PullRequest))
                            }
                            @if let Some((url, label)) = &issue { (external_link(url, label, None, Icon::Issue)) }
                        }
                    } @else {
                        p class="text-sm text-muted" { "No pull request or issue yet." }
                    }
                }))
                div class="border-b border-line px-5 py-4" {
                    (definition_list(&[
                        ("Status", status_select(task)),
                        ("Project", html! { (task.project_name) " " (chip(&task.project_key)) }),
                        ("Persona", html! { (task.persona_name) " " span class="font-normal text-muted" { "· " (harness_label(&task.persona_harness)) } }),
                        ("Branch", html! { span class="font-mono text-xs" { (branch) } }),
                    ]))
                }
                @if !task.description.is_empty() {
                    details class="border-b border-line px-5 py-4 text-sm" {
                        summary class="flex min-h-11 cursor-pointer items-center text-xs font-medium uppercase tracking-wide text-faint" { "Description" }
                        div class="line-clamp-3 whitespace-pre-wrap break-words text-muted" title=(task.description) { (task.description) }
                    }
                }
            }
        }
    }
}

/// The status of the details list: a select like the project filter whose menu shows the current
/// status (checked) and "Mark done"; a status that does not allow it shows the plain name.
fn status_select(task: &TaskDetail) -> Markup {
    let name = status_name(task.status);
    if !task.status.can_mark_done() {
        return html! { (name) };
    }
    let done = format!("/tasks/{}/done", task.key);
    html! {
        div class="relative" x-data="{ open: false }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="true" ":aria-expanded"="open" "x-on:click"="open = !open"
                class="inline-flex min-h-11 items-center gap-1 rounded-[10px] border border-line bg-panel px-3 text-sm font-medium hover:bg-sunk lg:min-h-8" {
                (name) (icon(Icon::ChevronDown))
            }
            div x-show="open" x-cloak class="absolute left-0 z-20 mt-1 w-48 rounded-xl border border-line bg-panel p-2 shadow-lg" {
                ul {
                    li class="flex min-h-11 items-center justify-between gap-2 rounded-lg px-2 text-sm font-medium text-accent-text lg:min-h-9" aria-current="true" {
                        span { (name) } (icon(Icon::Check))
                    }
                    li {
                        button type="button" hx-post=(done) hx-target="#task-details" hx-swap="outerMorph"
                            class="flex min-h-11 w-full items-center rounded-lg px-2 text-left text-sm font-normal text-ink hover:bg-sunk lg:min-h-9" { "Mark done" }
                    }
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
                class="inline-flex size-9 items-center justify-center rounded-lg text-accent-text hover:bg-sunk" { (icon(glyph)) }
        }
    } else {
        html! {}
    }
}

/// The runs of the task as cards (`#n`, status, start time, one line of detail and the token
/// table per model); updated live.
pub(crate) fn runs_region(runs: &[RunView], persona_model: Option<&str>) -> Markup {
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
                        @for run in runs { li id={"run-card-" (run.id)} class="space-y-2 rounded-lg border border-line p-3 text-sm" { (run_card(run, persona_model)) } }
                    }
                }
            }
        }
    }
}

/// Longest detail shown without a disclosure, in characters.
const DETAIL_LINE_CHARS: usize = 80;

fn run_card(run: &RunView, persona_model: Option<&str>) -> Markup {
    html! {
        div class="flex items-center gap-2" {
            // The divider of the run in the transcript; the click also closes the details where
            // they overlay the transcript (no effect from 1280 px, where they are a column).
            a href={"#run-" (run.number)} "x-on:click"="details = false"
                class="font-mono text-sm text-accent-text hover:underline" { "#" (run.number) }
            (tone_badge(Tone::of_run(run.state), run.state.as_str()))
            @if let Some(outcome) = &run.outcome { span class="text-xs text-muted" { (outcome_text(outcome)) } }
            time datetime=(run.started_at.to_rfc3339()) class="ml-auto font-mono text-xs text-faint" { (short_time(run.started_at)) }
        }
        @if let Some(retry) = run.retry_after { p class="text-xs text-status-limited-fg" { "retry at " (format_time(retry)) } }
        @if !run.detail.is_empty() { (run_detail(&run.detail)) }
        @if let Some(ended) = run.ended_at { p class="font-mono text-xs text-faint" { "ended " (format_time(ended)) } }
        @if run.usage.is_empty() {
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
pub(super) fn details_column(task: &TaskDetail, runs: &[RunView]) -> Markup {
    details_panel(
        "Details",
        html! { (details_region(task)) (runs_region(runs, task.persona_model.as_deref())) },
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
            x-data={"{ issue: '" (values.issue_number.trim().chars().filter(char::is_ascii_digit).collect::<String>()) "', linking: false }"}
            x-init="(() => { try { const d = JSON.parse(localStorage.getItem('df.composer') ?? '{}'); const pick = (el, v) => { if (!el.dataset.explicit && v && [...el.options].some(o => o.value === v)) el.value = v }; pick($refs.project, d.project); pick($refs.persona, d.persona) } catch (e) {} })()"
            "x-on:submit"="(() => { try { localStorage.setItem('df.composer', JSON.stringify({ project: $refs.project.value, persona: $refs.persona.value })) } catch (e) {} })()"
            "x-on:keydown.ctrl.enter.prevent"="$el.requestSubmit()" "x-on:keydown.meta.enter.prevent"="$el.requestSubmit()" {
            (form_error(errors))
            div class="flex flex-wrap items-center gap-2 px-1" {
                (choice_select(CHIP_SELECT, "Project", "project", &choices.projects, project, !values.project.is_empty(), errors))
                span x-show="issue" x-cloak class="inline-flex items-center gap-1 rounded-[10px] bg-accent-soft pl-3 text-xs text-accent-text" {
                    "Issue " span class="font-mono" { "#" span x-text="issue" {} }
                    button type="button" aria-label="Remove linked issue" "x-on:click"="issue = ''" class="inline-flex size-11 items-center justify-center lg:size-6" { (icon(Icon::Close)) }
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
                // The linked issue: a popover below the card (inline on phones, where the sheet
                // leaves no room below it). The field name stays `issue_number`.
                div x-show="linking" x-cloak role="group" aria-label="Linked issue"
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
                }
                div class="mt-2 flex items-center gap-2" {
                    button type="button" "x-on:click"="linking = !linking" ":aria-expanded"="linking"
                        ":class"="linking ? 'border-accent-text bg-accent-soft text-accent-text' : 'border-line text-muted hover:bg-sunk'"
                        aria-label="Link issue" title="Link issue"
                        class="inline-flex size-11 shrink-0 items-center justify-center rounded-full border lg:size-9" { (icon(Icon::Plus)) }
                    div class="ml-auto" { (choice_select(PERSONA_SELECT, "Assignee", "persona", &choices.personas, persona, !values.persona.is_empty(), errors)) }
                    button type="submit" aria-label="Create task" title="Create task"
                        class=(ROUND_ACTION_CLASS) { (icon(Icon::ArrowUp)) }
                }
            }
        }
    }
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
fn status_name(status: TaskStatus) -> String {
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
mod tests {
    use super::*;

    #[test]
    fn filters_parse_and_serialize_without_defaults() {
        assert_eq!(Filters::new(None, None).query(), "");
        assert_eq!(Filters::new(Some("nonsense"), Some("")).query(), "");
        let f = Filters::new(Some("done"), Some("WEB"));
        assert_eq!(f.query(), "?status=done&project=WEB");
        assert_eq!(f.status.group(), Some(StatusSet::Done));
        assert_eq!(StatusFilter::parse(Some("all")).group(), None);
        assert_eq!(Filters::new(None, Some("A B")).query(), "?project=A+B");
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
        assert!(html.contains("issue: '7'"), "{html}");
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
            state: df_core::RunState::Succeeded,
            outcome: None,
            detail: String::new(),
            retry_after: None,
            started_at: DateTime::UNIX_EPOCH,
            ended_at: None,
            usage: vec![],
        }
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
                queued_at: DateTime::UNIX_EPOCH,
                delivered_run_id: None,
                delivered_at: None,
                forwarded_at: None,
            })
            .collect()
    }

    #[test]
    fn runs_are_closed_by_default_with_count_and_keep_the_given_order() {
        let html = runs_region(&[run(3), run(2), run(1)], None).into_string();
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
        assert!(!runs_region(&[], None).into_string().contains("<details"));
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
            let html = runs_region(&list, None).into_string();
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
        let ended =
            pending_region("DF-1", "Linus", "failed", &queued(1), false, None).into_string();
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
        let off = pending_region("DF-1", "Linus", "running", &[message.clone()], false, None)
            .into_string();
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
        let only =
            pending_region("DF-1", "Linus", "running", &both[..1], false, None).into_string();
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
    fn run_card_links_to_the_divider_and_lists_usage_in_three_columns() {
        let mut r = run(3);
        r.usage = vec![df_db::task_views::ModelUsage {
            model_id: "anthropic/claude-sonnet-5-5-20261001".into(),
            input_tokens: 9437,
            output_tokens: 182_345,
            cache_read_tokens: 1_234_567,
            cache_write_tokens: 12,
        }];
        let html = run_card(&r, None).into_string();
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

    #[test]
    fn send_save_and_create_use_the_round_action_classes() {
        let area = message_area("DF-1", "Linus", "queued", &[], false).into_string();
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
        let html =
            composer_dialog(&TaskForm::default(), &FormErrors::default(), &none).into_string();
        assert!(html.contains("Create a project and a persona first."));
        assert!(!html.contains("<form id=\"composer-form\""));
    }
}
