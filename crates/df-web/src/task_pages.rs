//! Task pages: list, creation, detail with runs, PR/issue links and the message input
//! (`specs/task-ui/spec.md`).

use axum::extract::{Form, Path, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use chrono::{DateTime, Utc};
use df_core::{PersonaId, ProjectId, TaskId, TaskKey};
use df_db::task_views::{RunView, TaskDetail, TaskListItem};
use df_db::{NewTask, TaskMessageRow};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::layout::{Nav, layout};
use crate::sse;
use crate::transcript::{Folder, blocks_text, render_transcript};
use crate::ui::{
    FormErrors, form_error, htmx_redirect, internal_error, invalid, not_found, select_field,
    status_badge, submit_button, text_field, textarea_field,
};

/// How many transcript rows the detail page renders at once.
const PAGE_BATCH: i64 = 1000;

pub fn routes() -> Router<WebState> {
    Router::new()
        .route("/", get(|| async { Redirect::to("/tasks") }))
        .route("/tasks", get(task_list).post(task_create))
        .route("/tasks/new", get(task_new))
        .route("/tasks/events", get(sse::task_list_events))
        .route("/tasks/{key}", get(task_detail))
        .route("/tasks/{key}/events", get(sse::task_events))
        .route("/tasks/{key}/messages", post(message_create))
}

// ---------------------------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------------------------

/// The rows of the task list; replaced as a whole when a status changes.
pub fn task_rows(items: &[TaskListItem]) -> Markup {
    html! {
        ul id="task-rows" class="divide-y divide-slate-800" {
            @for t in items {
                li {
                    a href={"/tasks/" (t.key)} class="block min-h-11 py-3" {
                        div class="flex items-start justify-between gap-3" {
                            div class="min-w-0" {
                                span class="mr-2 font-mono text-sm text-slate-400" { (t.key) }
                                span class="break-words font-medium" { (t.title) }
                            }
                            (status_badge(&t.status))
                        }
                        @if !t.status_line.is_empty() { div class="mt-1 break-words text-sm text-slate-300" { (t.status_line) } }
                        div class="mt-1 text-xs text-slate-500" { (t.project_name) " - " (t.persona_name) }
                    }
                }
            }
            @if items.is_empty() { li class="py-6 text-center text-slate-400" { "No tasks yet." } }
        }
    }
}

async fn task_list(State(state): State<WebState>) -> Response {
    let items = match df_db::task_views::list_task_items(&state.pool).await {
        Ok(items) => items,
        Err(e) => return internal_error(&e),
    };
    layout("Tasks", Nav::Tasks, html! {
        div class="mb-4 flex items-center justify-between gap-3" {
            h1 class="text-xl font-semibold" { "Tasks" }
            a href="/tasks/new" class="min-h-11 rounded bg-sky-600 px-4 py-2 text-sm font-medium text-white" { "New task" }
        }
        div hx-sse:connect="/tasks/events" hx-swap="none" {}
        (task_rows(&items))
    })
    .into_response()
}

// ---------------------------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct TaskForm {
    title: String,
    description: String,
    project: String,
    persona: String,
    issue_number: String,
}

struct Choices {
    projects: Vec<(String, String)>,
    personas: Vec<(String, String)>,
}

async fn choices(state: &WebState) -> Result<Choices, df_db::DbError> {
    let (projects, personas) = tokio::try_join!(
        df_db::catalog::list_projects(&state.pool),
        df_db::catalog::list_personas(&state.pool)
    )?;
    Ok(Choices {
        projects: projects
            .into_iter()
            .map(|p| (p.id.to_string(), format!("{} - {}", p.key, p.name)))
            .collect(),
        personas: personas
            .into_iter()
            .map(|p| (p.id.to_string(), p.name))
            .collect(),
    })
}

fn task_form(values: &TaskForm, errors: &FormErrors, choices: &Choices) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post="/tasks" hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (text_field("Title", "title", &values.title, errors, None))
            (textarea_field("Description", "description", &values.description, 6, errors))
            (select_field("Project", "project", &choices.projects, &values.project, None, errors))
            (select_field("Persona", "persona", &choices.personas, &values.persona, None, errors))
            (text_field("Issue number", "issue_number", &values.issue_number, errors, Some("Optional.")))
            (submit_button("Create task"))
        }
    }
}

async fn task_new(State(state): State<WebState>) -> Response {
    let choices = match choices(&state).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    layout(
        "New task",
        Nav::Tasks,
        html! {
            h1 class="mb-4 text-xl font-semibold" { "New task" }
            div class="max-w-xl" {
                @if choices.projects.is_empty() || choices.personas.is_empty() {
                    p class="text-slate-300" { "Create a project and a persona first." }
                } @else {
                    (task_form(&TaskForm::default(), &FormErrors::default(), &choices))
                }
            }
        },
    )
    .into_response()
}

async fn task_create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<TaskForm>,
) -> Response {
    let choices = match choices(&state).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    let mut errors = FormErrors::default();
    let title = form.title.trim().to_string();
    if title.is_empty() {
        errors.add("title", "Title is required.");
    }
    let project = form
        .project
        .parse::<uuid::Uuid>()
        .ok()
        .filter(|id| has_choice(&choices.projects, id));
    if project.is_none() {
        errors.add("project", "Choose a project.");
    }
    let persona = form
        .persona
        .parse::<uuid::Uuid>()
        .ok()
        .filter(|id| has_choice(&choices.personas, id));
    if persona.is_none() {
        errors.add("persona", "Choose a persona.");
    }
    let issue_number = match form.issue_number.trim() {
        "" => None,
        n => match n.parse::<i64>() {
            Ok(n) if n >= 1 => Some(n),
            _ => {
                errors.add("issue_number", "Must be a positive number.");
                None
            }
        },
    };
    let (Some(project), Some(persona)) = (project, persona) else {
        return invalid(task_form(&form, &errors, &choices));
    };
    if !errors.is_empty() {
        return invalid(task_form(&form, &errors, &choices));
    }
    let created = df_db::create_task(
        &state.pool,
        NewTask {
            project_id: ProjectId::from_uuid(project),
            persona_id: PersonaId::from_uuid(persona),
            title: title.clone(),
            description: form.description.trim().to_string(),
            issue_number,
            created_by: Some(user.id),
        },
    )
    .await;
    let (task, _) = match created {
        Ok(created) => created,
        Err(e) => return internal_error(&e),
    };
    let entry = df_db::NewAuditEntry::new(
        user.principal(),
        user.roles.clone(),
        "task.create",
        "task",
        task.key.to_string(),
    )
    .with_details(&serde_json::json!({ "title": title }));
    // Best effort: the task exists already, so a failed audit entry is logged, not reported.
    let audited = match entry {
        Ok(entry) => df_db::write_audit_entry(&state.pool, &entry).await,
        Err(e) => Err(e),
    };
    if let Err(error) = audited {
        tracing::warn!(%error, task = %task.key, "audit entry for task creation failed");
    }
    htmx_redirect(&format!("/tasks/{}", task.key))
}

fn has_choice(options: &[(String, String)], id: &uuid::Uuid) -> bool {
    options.iter().any(|(value, _)| *value == id.to_string())
}

// ---------------------------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------------------------

fn format_time(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn external_link(url: &str, label: &str) -> Markup {
    // Forge URLs are rendered as links only for http(s); anything else stays inert text.
    if url.starts_with("https://") || url.starts_with("http://") {
        html! { a href=(url) class="text-sky-400 underline" rel="noopener noreferrer" target="_blank" { (label) } }
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

/// Title, status and links; updated live.
pub fn head_region(task: &TaskDetail) -> Markup {
    html! {
        section id="task-head" class="mb-4 space-y-2" {
            div class="flex items-start justify-between gap-3" {
                h1 class="min-w-0 break-words text-xl font-semibold" {
                    span class="mr-2 font-mono text-slate-400" { (task.key) } (task.title)
                }
                (status_badge(&task.status))
            }
            @if !task.status_line.is_empty() { p class="break-words text-slate-300" { (task.status_line) } }
            div class="flex flex-wrap gap-x-4 gap-y-1 text-sm text-slate-400" {
                span { (task.project_name) } span { (task.persona_name) }
                @if let Some(url) = &task.pr_url {
                    (external_link(url, &format!("PR{}", task.pr_number.map(|n| format!(" #{n}")).unwrap_or_default())))
                }
                @if let (Some(number), Some(url)) = (task.issue_number, issue_url(task)) {
                    (external_link(&url, &format!("Issue #{number}")))
                }
            }
            @if !task.description.is_empty() {
                details class="text-sm text-slate-300" {
                    summary class="min-h-11 cursor-pointer py-2" { "Description" }
                    div class="whitespace-pre-wrap break-words" { (task.description) }
                }
            }
        }
    }
}

fn outcome_text(outcome: &str) -> String {
    outcome.replace('_', " ")
}

/// The runs of the task with token counts; updated live.
pub fn runs_region(runs: &[RunView]) -> Markup {
    html! {
        section id="runs" class="mb-4" {
            @if !runs.is_empty() {
                h2 class="mb-2 text-sm font-semibold uppercase text-slate-400" { "Runs" }
                ul class="space-y-2" {
                    @for run in runs {
                        li class="rounded border border-slate-800 bg-slate-900 px-3 py-2 text-sm" {
                            div class="flex flex-wrap items-center gap-x-3 gap-y-1" {
                                span class="font-medium" { "Run " (run.number) }
                                (status_badge(&run.state))
                                @if let Some(outcome) = &run.outcome { span { (outcome_text(outcome)) } }
                                @if let Some(retry) = run.retry_after { span class="text-amber-300" { "retry at " (format_time(retry)) } }
                            }
                            @if !run.detail.is_empty() { div class="break-words text-slate-300" { (run.detail) } }
                            div class="text-xs text-slate-500" {
                                (format_time(run.started_at))
                                @if let Some(end) = run.ended_at { " - " (format_time(end)) }
                            }
                            @for u in &run.usage {
                                div class="text-xs text-slate-400" {
                                    (u.model_id) ": " (u.input_tokens) " in, " (u.output_tokens) " out, "
                                    (u.cache_read_tokens) " cache read, " (u.cache_write_tokens) " cache write"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Queued messages the agent has not received yet.
pub fn pending_region(messages: &[TaskMessageRow], error: Option<&str>) -> Markup {
    html! {
        div id="pending" class="flex flex-col gap-2" {
            @if let Some(error) = error { p class="text-sm text-red-400" role="alert" { (error) } }
            @for m in messages {
                div id={"pending-" (m.id)} class="min-w-0 self-end rounded-lg border border-dashed border-sky-700 px-3 py-2 opacity-70 md:max-w-[80%]" {
                    div class="text-xs text-sky-300" { "Pending" }
                    div class="whitespace-pre-wrap break-words" { (blocks_text(&m.content)) }
                }
            }
        }
    }
}

fn message_form(key: &str) -> Markup {
    html! {
        // The draft lives in Alpine state outside every region the stream swaps, so it survives
        // reconnects. The server clears it with the `message-queued` trigger.
        form class="sticky bottom-0 mt-4 space-y-2 border-t border-slate-800 bg-slate-950 py-3"
            x-data="{ draft: '' }" "x-on:message-queued.window"="draft = ''"
            hx-post={"/tasks/" (key) "/messages"} hx-target="#pending" hx-swap="outerMorph" {
            label for="message" class="sr-only" { "Message" }
            textarea id="message" name="message" rows="3" required placeholder="Message the agent" x-model="draft"
                class="min-h-11 w-full rounded border border-slate-700 bg-slate-900 px-3 py-2 text-base" {}
            (submit_button("Send"))
        }
    }
}

/// Loads everything the stream and the page render for one task.
pub struct DetailData {
    pub task: TaskDetail,
    pub runs: Vec<RunView>,
    pub pending: Vec<TaskMessageRow>,
}

pub async fn load_detail(state: &WebState, task: TaskDetail) -> Result<DetailData, df_db::DbError> {
    let id = TaskId::from_uuid(task.id);
    let (runs, pending) = tokio::try_join!(
        df_db::task_views::list_runs(&state.pool, id),
        df_db::list_undelivered_messages(&state.pool, id)
    )?;
    Ok(DetailData {
        task,
        runs,
        pending,
    })
}

async fn task_detail(State(state): State<WebState>, Path(key): Path<String>) -> Response {
    let task = match df_db::task_views::get_task_detail_by_key(&state.pool, &key).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let id = TaskId::from_uuid(task.id);
    let mut folder = Folder::default();
    loop {
        let rows = match df_db::task_views::transcript_after(
            &state.pool,
            id,
            folder.last_id(),
            PAGE_BATCH,
        )
        .await
        {
            Ok(rows) => rows,
            Err(e) => return internal_error(&e),
        };
        let done = i64::try_from(rows.len()).is_ok_and(|n| n < PAGE_BATCH);
        for row in &rows {
            folder.apply(row);
        }
        if done {
            break;
        }
    }
    let data = match load_detail(&state, task).await {
        Ok(data) => data,
        Err(e) => return internal_error(&e),
    };
    let events_url = format!("/tasks/{key}/events?after={}", folder.last_id());
    layout(
        &format!("{key} {}", data.task.title),
        Nav::Tasks,
        html! {
            div hx-sse:connect=(events_url) hx-swap="none" {}
            (head_region(&data.task))
            (runs_region(&data.runs))
            (render_transcript(folder.items()))
            (pending_region(&data.pending, None))
            (message_form(&key))
        },
    )
    .into_response()
}

// ---------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MessageForm {
    message: String,
}

async fn message_create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    Form(form): Form<MessageForm>,
) -> Response {
    let Ok(task_key) = key.parse::<TaskKey>() else {
        return not_found("The task");
    };
    let task =
        match df_db::task_views::get_task_detail_by_key(&state.pool, &task_key.to_string()).await {
            Ok(Some(task)) => task,
            Ok(None) => return not_found("The task"),
            Err(e) => return internal_error(&e),
        };
    let id = TaskId::from_uuid(task.id);
    let text = form.message.trim();
    let error = if text.is_empty() {
        Some("Write a message first.")
    } else {
        let content = vec![ContentBlock {
            kind: Some(content_block::Kind::Text(TextContent {
                text: text.to_string(),
            })),
        }];
        match df_db::queue_message(&state.pool, id, user.principal(), content).await {
            Ok(_) => None,
            Err(e) => return internal_error(&e),
        }
    };
    let pending = match df_db::list_undelivered_messages(&state.pool, id).await {
        Ok(p) => p,
        Err(e) => return internal_error(&e),
    };
    let body = pending_region(&pending, error);
    if error.is_some() {
        return (StatusCode::UNPROCESSABLE_ENTITY, body).into_response();
    }
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static("message-queued"));
    response
}

#[cfg(test)]
mod tests;
