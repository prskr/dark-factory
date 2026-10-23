//! Markup of the task workspace: list, transcript header, details, runs, pending messages and
//! the composer. Handlers live in the parent module; the live-update regions (`#task-head`,
//! `#task-details`, `#runs`, `#pending`, `#task-rows`) are re-rendered by `crate::sse`.

use chrono::{DateTime, Utc};
use df_core::{StatusGroup, TITLE_MAX_CHARS, TaskKey, TaskStatus, TitleSource};
use df_db::TaskMessageRow;
use df_db::task_views::{ProjectFilterItem, RunView, TaskDetail, TaskListItem};
use maud::{Markup, html};

use crate::components::forms::{INPUT_CLASS, MenuItem};
use crate::components::transcript::pending_message;
use crate::components::{
    ButtonKind, FormErrors, ICON_BUTTON_CLASS, Icon, Tone, action_class, chip, definition_list,
    details_panel, dialog, empty_state, field_error, filter_chip, form_error, icon, search_menu,
    section, status_badge, tone_badge,
};
use crate::transcript::blocks_text;

const CHIP_SELECT: &str =
    "min-h-11 rounded-full border border-line bg-panel px-3 text-sm text-muted lg:min-h-8";

/// The status filter of the task list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusFilter {
    #[default]
    Active,
    Waiting,
    Done,
    All,
}

impl StatusFilter {
    pub const ALL: [Self; 4] = [Self::Active, Self::Waiting, Self::Done, Self::All];

    /// Parses the `status` query parameter; unknown values mean the default.
    pub fn parse(value: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| Some(f.as_str()) == value)
            .unwrap_or_default()
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::All => "all",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Done => "Done",
            Self::All => "All",
        }
    }

    pub const fn group(self) -> Option<StatusGroup> {
        match self {
            Self::Active => Some(StatusGroup::Active),
            Self::Waiting => Some(StatusGroup::Waiting),
            Self::Done => Some(StatusGroup::Done),
            Self::All => None,
        }
    }
}

/// The filters of the task list; they live in the URL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    pub status: StatusFilter,
    pub project: Option<String>,
}

impl Filters {
    pub fn new(status: Option<&str>, project: Option<&str>) -> Self {
        Self {
            status: StatusFilter::parse(status),
            project: project.filter(|p| !p.is_empty()).map(str::to_string),
        }
    }

    pub fn to_db(&self) -> df_db::task_views::TaskFilter {
        df_db::task_views::TaskFilter {
            group: self.status.group(),
            project_key: self.project.clone(),
        }
    }

    /// The query string (with `?`) of these filters; defaults are left out.
    pub fn query(&self) -> String {
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
    pub fn compose_href(&self) -> String {
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
pub fn task_rows(items: &[TaskListItem], current: Option<&str>, filters: &Filters) -> Markup {
    let query = filters.query();
    html! {
        ul id="task-rows" {
            @for t in items {
                @let href = format!("/tasks/{}{}", t.key, query);
                li id={"row-" (t.key)} {
                    a href=(href) hx-get=(href) hx-target="#ws-detail" hx-select="#ws-detail" hx-swap="outerHTML" hx-push-url="true"
                        aria-current=[(current == Some(t.key.as_str())).then_some("page")]
                        "x-on:click"="document.body.dataset.view = 'task'; details = false; $root.querySelectorAll('#task-rows [aria-current]').forEach(e => e.removeAttribute('aria-current')); $el.setAttribute('aria-current', 'page')"
                        class="block min-h-11 border-b border-line px-4 py-3 hover:bg-sunk aria-[current=page]:bg-accent-soft" {
                        div class="flex items-baseline justify-between gap-3" {
                            span class="font-mono text-xs text-faint" { (t.key) }
                            time datetime=(t.updated_at.to_rfc3339()) class="font-mono text-xs text-faint" { (short_time(t.updated_at)) }
                        }
                        div class={"mt-0.5 break-words text-sm font-medium" @if t.title_source == TitleSource::Placeholder { " italic text-muted" }} { (t.title) }
                        div class="mt-1.5 flex items-center gap-2" {
                            @if let Some(status) = status_of(&t.status) { (status_badge(status)) } @else { (tone_badge(Tone::Queued, &t.status)) }
                            @if !t.status_line.is_empty() { span class="min-w-0 truncate text-xs text-muted" { (t.status_line) } }
                        }
                        div class="mt-1 text-xs text-faint" { (t.project_name) " · " (t.persona_name) }
                    }
                }
            }
            @if items.is_empty() { li { (empty_state("No tasks match.")) } }
        }
    }
}

/// The list column: filters, the live list and its stream.
pub fn list_column(
    items: &[TaskListItem],
    projects: &[ProjectFilterItem],
    filters: &Filters,
    current: Option<&str>,
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
                h1 class="text-lg font-semibold" { "Tasks" }
                div class="flex flex-wrap items-center gap-2" {
                    @for status in StatusFilter::ALL {
                        (filter_chip(status.label(), &href(&filters.with_status(status)), filters.status == status))
                    }
                }
                (search_menu(menu_label, &menu))
            }
            div class="min-h-0 flex-1 overflow-y-auto" {
                div hx-sse:connect=(events) hx-swap="none" {}
                (task_rows(items, current, filters))
            }
        }
    }
}

/// Key, title with inline rename, status and the details button; updated live.
pub fn head_region(task: &TaskDetail, error: Option<&str>) -> Markup {
    let status = status_of(&task.status);
    let action = format!("/tasks/{}/title", task.key);
    html! {
        header id="task-head" class="border-b border-line bg-panel px-2 py-2 lg:px-4" {
            div class="flex items-center gap-1" {
                a href="/tasks" aria-label="Back to tasks" class={(ICON_BUTTON_CLASS) " lg:hidden"} { (icon(Icon::ChevronLeft)) }
                div class="min-w-0 flex-1" x-data="{ editing: false }" {
                    div class="flex items-center gap-2" {
                        span class="shrink-0 font-mono text-sm text-faint" { (task.key) }
                        button type="button" x-show="!editing" "x-on:click"="editing = true; $nextTick(() => $refs.title.focus())"
                            aria-label="Rename task" class="group flex min-h-11 min-w-0 items-center gap-1 rounded-lg px-1 text-left text-base font-semibold hover:bg-sunk lg:min-h-9" {
                            span class="truncate" { (title_text(&task.title, task.title_source)) }
                            span class="text-faint" { (icon(Icon::Pencil)) }
                        }
                        form x-show="editing" x-cloak hx-post=(action) hx-target="#task-head" hx-swap="outerMorph"
                            "x-on:keydown.escape"="editing = false" "x-on:title-saved.window"="editing = false"
                            class="flex min-w-0 flex-1 items-center gap-2" {
                            label class="sr-only" for="title" { "Title" }
                            input id="title" name="title" type="text" value=(task.title) maxlength=(TITLE_MAX_CHARS) required x-ref="title"
                                class=(INPUT_CLASS);
                            button type="submit" class=(action_class(ButtonKind::Primary)) { "Save" }
                        }
                    }
                    (field_error(error))
                }
                @if let Some(status) = status { (status_badge(status)) }
                button type="button" aria-label="Details" "x-on:click"="details = true" class={(ICON_BUTTON_CLASS) " xl:hidden"} { (icon(Icon::PanelRight)) }
            }
        }
    }
}

/// Queued messages the agent has not received yet.
pub fn pending_region(messages: &[TaskMessageRow], error: Option<&str>) -> Markup {
    html! {
        div id="pending" class="flex flex-col gap-2" {
            (field_error(error))
            @for m in messages { (pending_message(&format!("pending-{}", m.id), &blocks_text(&m.content))) }
        }
    }
}

/// The message input below the transcript.
pub fn message_form(key: &str) -> Markup {
    html! {
        // The draft lives in Alpine state outside every region the stream swaps, so it survives
        // reconnects. The server clears it with the `message-queued` trigger.
        form class="border-t border-line bg-panel p-3"
            x-data="{ draft: '' }" "x-on:message-queued.window"="draft = ''"
            hx-post={"/tasks/" (key) "/messages"} hx-target="#pending" hx-swap="outerMorph" {
            label for="message" class="sr-only" { "Message" }
            div class="flex items-end gap-2" {
                textarea id="message" name="message" rows="2" required placeholder="Message the agent" x-model="draft"
                    "x-on:keydown.ctrl.enter.prevent"="$el.form.requestSubmit()" "x-on:keydown.meta.enter.prevent"="$el.form.requestSubmit()"
                    class=(INPUT_CLASS) {}
                button type="submit" aria-label="Send" class={(action_class(ButtonKind::Primary)) " shrink-0"} { (icon(Icon::Send)) }
            }
        }
    }
}

/// Status line, links, project, persona and branch; updated live.
pub fn details_region(task: &TaskDetail) -> Markup {
    let branch = task
        .key
        .parse::<TaskKey>()
        .map(|k| k.task_branch())
        .unwrap_or_default();
    html! {
        div id="task-details" class="space-y-4" {
            @if !task.status_line.is_empty() { p class="break-words text-sm text-muted" { (task.status_line) } }
            @let has_pr = task.pr_url.is_some();
            @let issue = issue_url(task).zip(task.issue_number);
            @if has_pr || issue.is_some() {
                div class="flex flex-col gap-2 text-sm" {
                    @if let Some(url) = &task.pr_url {
                        (external_link(url, &format!("PR{}", task.pr_number.map(|n| format!(" #{n}")).unwrap_or_default()), Icon::PullRequest))
                    }
                    @if let Some((url, number)) = &issue { (external_link(url, &format!("Issue #{number}"), Icon::Issue)) }
                }
            }
            (definition_list(&[
                ("Project", html! { (task.project_name) " " (chip(&task.project_key)) }),
                ("Persona", html! { (task.persona_name) }),
                ("Branch", html! { span class="font-mono text-xs" { (branch) } }),
            ]))
            @if !task.description.is_empty() {
                details class="text-sm" {
                    summary class="flex min-h-11 cursor-pointer items-center text-muted" { "Description" }
                    div class="whitespace-pre-wrap break-words text-muted" { (task.description) }
                }
            }
        }
    }
}

/// The runs of the task with token counts; updated live.
pub fn runs_region(runs: &[RunView]) -> Markup {
    html! {
        div id="runs" {
            @if !runs.is_empty() {
                (section("Runs", html! {
                    ul class="space-y-2" {
                        @for run in runs {
                            li class="rounded-lg border border-line bg-sunk px-3 py-2 text-sm" {
                                div class="flex flex-wrap items-center gap-x-3 gap-y-1" {
                                    span class="font-medium" { "Run " (run.number) }
                                    (tone_badge(Tone::of_run(run.state), run.state.as_str()))
                                    @if let Some(outcome) = &run.outcome { span { (outcome_text(outcome)) } }
                                    @if let Some(retry) = run.retry_after { span class="text-status-limited-fg" { "retry at " (format_time(retry)) } }
                                }
                                @if !run.detail.is_empty() { div class="break-words text-muted" { (run.detail) } }
                                div class="font-mono text-xs text-faint" {
                                    (format_time(run.started_at))
                                    @if let Some(end) = run.ended_at { " - " (format_time(end)) }
                                }
                                @for u in &run.usage {
                                    div class="font-mono text-xs text-faint" {
                                        (u.model_id) ": " (u.input_tokens) " in, " (u.output_tokens) " out, "
                                        (u.cache_read_tokens) " cache read, " (u.cache_write_tokens) " cache write"
                                    }
                                }
                            }
                        }
                    }
                }))
            }
        }
    }
}

/// The details panel: bottom sheet, slide-over or column depending on the width.
pub fn details_column(task: &TaskDetail, runs: &[RunView]) -> Markup {
    details_panel(
        "Details",
        html! { (details_region(task)) (runs_region(runs)) },
    )
}

/// Values of the composer form.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub struct TaskForm {
    pub description: String,
    pub project: String,
    pub persona: String,
    pub issue_number: String,
}

/// Projects and personas to choose from, as `(id, label)`.
pub struct Choices {
    pub projects: Vec<(String, String)>,
    pub personas: Vec<(String, String)>,
}

/// The composer form. The first project/persona of the lists apply unless the values or the
/// browser's last choice say otherwise (`x-init`).
pub fn composer_form(values: &TaskForm, errors: &FormErrors, choices: &Choices) -> Markup {
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
        form id="composer-form" class="space-y-3 pt-3" hx-post="/tasks" hx-target="this" hx-swap="outerHTML"
            x-data={"{ issue: '" (values.issue_number.trim().chars().filter(char::is_ascii_digit).collect::<String>()) "', options: false }"}
            x-init="try { const d = JSON.parse(localStorage.getItem('df.composer') ?? '{}'); const pick = (el, v) => { if (!el.dataset.explicit && v && [...el.options].some(o => o.value === v)) el.value = v }; pick($refs.project, d.project); pick($refs.persona, d.persona) } catch (e) {}"
            "x-on:submit"="try { localStorage.setItem('df.composer', JSON.stringify({ project: $refs.project.value, persona: $refs.persona.value })) } catch (e) {}"
            "x-on:keydown.ctrl.enter.prevent"="$el.requestSubmit()" "x-on:keydown.meta.enter.prevent"="$el.requestSubmit()" {
            (form_error(errors))
            div class="flex flex-wrap items-center gap-2" {
                (choice_select("Project", "project", &choices.projects, project, !values.project.is_empty(), errors))
                span x-show="issue" x-cloak class="inline-flex items-center gap-1 rounded-full bg-accent-soft px-2.5 py-1 text-xs text-accent-text" {
                    "Issue #" span x-text="issue" {}
                    button type="button" aria-label="Remove linked issue" "x-on:click"="issue = ''" class="inline-flex size-11 items-center justify-center lg:size-6" { (icon(Icon::Close)) }
                }
            }
            div {
                label for="description" class="sr-only" { "Description" }
                textarea id="description" name="description" rows="6" placeholder="What should the agent do?" class=(INPUT_CLASS) autofocus { (values.description) }
                (field_error(errors.get("description")))
            }
            div x-show="options" x-cloak class="rounded-lg border border-line p-3" {
                label for="issue_number" class="block text-sm font-medium text-muted" { "Linked issue number" }
                input id="issue_number" name="issue_number" type="text" inputmode="numeric" x-model="issue" class=(INPUT_CLASS);
                (field_error(errors.get("issue_number")))
            }
            div class="flex items-center justify-between gap-2" {
                div class="flex items-center gap-2" {
                    button type="button" "x-on:click"="options = !options" ":aria-expanded"="options" class={(action_class(ButtonKind::Ghost)) " px-2"} { "Options" }
                    (choice_select("Assignee", "persona", &choices.personas, persona, !values.persona.is_empty(), errors))
                }
                button type="submit" class=(action_class(ButtonKind::Primary)) { "Create task" (icon(Icon::Send)) }
            }
        }
    }
}

/// The composer as a dialog (bottom sheet on phones), or a hint when nothing can be chosen yet.
pub fn composer_dialog(values: &TaskForm, errors: &FormErrors, choices: &Choices) -> Markup {
    let body = if choices.projects.is_empty() || choices.personas.is_empty() {
        html! { p class="py-4 text-sm text-muted" { "Create a project and a persona first." } }
    } else {
        composer_form(values, errors, choices)
    };
    dialog("composer", "New task", body)
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn short_time(time: DateTime<Utc>) -> String {
    time.format("%b %d %H:%M").to_string()
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

fn external_link(url: &str, label: &str, glyph: Icon) -> Markup {
    // Forge URLs are rendered as links only for http(s); anything else stays inert text.
    if url.starts_with("https://") || url.starts_with("http://") {
        html! {
            a href=(url) class="inline-flex items-center gap-1.5 text-accent-text underline" rel="noopener noreferrer" target="_blank" {
                (icon(glyph)) (label)
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

fn choice_select(
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
            select id=(name) name=(name) x-ref=(name) data-explicit=[explicit.then_some("true")] class=(CHIP_SELECT) {
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
        assert_eq!(f.status.group(), Some(StatusGroup::Done));
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
            "Linked issue number",
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
