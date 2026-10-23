//! Task pages: the three-column workspace (list, transcript, details), the composer, rename and
//! the message input (`specs/task-ui/spec.md`, design D4 to D8).

mod view;

use axum::extract::{Form, Path, Query, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use df_core::{
    PersonaId, ProjectId, TITLE_MAX_CHARS, TaskId, TaskKey, sanitize_title, title_too_long,
};
use df_db::NewTask;
use df_db::task_views::{RunView, TaskDetail};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::FormErrors;
use crate::layout::{View, workspace_layout};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};
use crate::sse;
use crate::transcript::{Folder, render_transcript};
pub use view::{
    Choices, Filters, TaskForm, details_region, head_region, list_column, pending_region,
    runs_region, task_rows,
};
use view::{composer_dialog, composer_form, details_column, message_form};

/// How many transcript rows the detail page renders at once.
const PAGE_BATCH: i64 = 1000;

pub fn routes() -> Router<WebState> {
    Router::new()
        .route("/", get(|| async { Redirect::to("/tasks") }))
        .route("/tasks", get(task_list).post(task_create))
        .route("/tasks/compose", get(composer))
        .route("/tasks/events", get(sse::task_list_events))
        .route("/tasks/{key}", get(task_detail))
        .route("/tasks/{key}/events", get(sse::task_events))
        .route("/tasks/{key}/messages", post(message_create))
        .route("/tasks/{key}/title", post(task_rename))
}

/// The query of the list pages.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    status: Option<String>,
    project: Option<String>,
}

impl ListQuery {
    pub fn filters(&self) -> Filters {
        Filters::new(self.status.as_deref(), self.project.as_deref())
    }
}

/// Loads everything the stream and the page render for one task.
pub struct DetailData {
    pub task: TaskDetail,
    pub runs: Vec<RunView>,
    pub pending: Vec<df_db::TaskMessageRow>,
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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TitleForm {
    title: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MessageForm {
    message: String,
}

/// Everything the transcript and details columns render for one task.
struct Selected {
    detail: TaskDetail,
    runs: Vec<RunView>,
    pending: Vec<df_db::TaskMessageRow>,
    folder: Folder,
}

/// Renders the workspace page; the transcript and details columns exist only for a selected task.
async fn workspace(state: &WebState, filters: &Filters, selected: Option<Selected>) -> Response {
    let filter = filters.to_db();
    let list = df_db::task_views::list_task_items(&state.pool, &filter);
    let projects =
        df_db::task_views::list_project_filter_items(&state.pool, filters.status.group());
    let (items, projects) = match tokio::try_join!(list, projects) {
        Ok(loaded) => loaded,
        Err(e) => return internal_error(&e),
    };
    let current = selected.as_ref().map(|s| s.detail.key.as_str());
    let compose = filters.compose_href();
    let (title, view) = match &selected {
        Some(s) => (format!("{} {}", s.detail.key, s.detail.title), View::Task),
        None => ("Tasks".to_string(), View::List),
    };
    let body = html! {
        div class="ws" x-data="{ details: false }" {
            (list_column(&items, &projects, filters, current))
            div id="ws-detail" {
                @match &selected {
                    Some(s) => (selected_columns(s)),
                    None => section class="ws-main items-center justify-center" aria-label="Task" {
                        p class="text-sm text-faint" { "Select a task." }
                    },
                }
            }
        }
    };
    workspace_layout(&title, view, &compose, body).into_response()
}

fn selected_columns(s: &Selected) -> Markup {
    let key = s.detail.key.as_str();
    let events_url = format!("/tasks/{key}/events?after={}", s.folder.last_id());
    html! {
        div hx-sse:connect=(events_url) hx-swap="none" {}
        // The scrim closes the details panel where it floats over the transcript.
        div x-show="details" x-cloak "x-on:click"="details = false" class="fixed inset-0 z-30 bg-black/40 xl:hidden" {}
        section class="ws-main" aria-label="Task" {
            (head_region(&s.detail, None))
            div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4" {
                (render_transcript(s.folder.items()))
                (pending_region(&s.pending, None))
            }
            (message_form(key))
        }
        (details_column(&s.detail, &s.runs))
    }
}

async fn task_list(State(state): State<WebState>, Query(query): Query<ListQuery>) -> Response {
    workspace(&state, &query.filters(), None).await
}

async fn task_detail(
    State(state): State<WebState>,
    Path(key): Path<String>,
    Query(query): Query<ListQuery>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
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
    let selected = Selected {
        detail: data.task,
        runs: data.runs,
        pending: data.pending,
        folder,
    };
    workspace(&state, &query.filters(), Some(selected)).await
}

async fn choices(state: &WebState) -> Result<(Choices, Vec<(String, String)>), df_db::DbError> {
    let (projects, personas) = tokio::try_join!(
        df_db::catalog::list_projects(&state.pool),
        df_db::catalog::list_personas(&state.pool)
    )?;
    let keys = projects
        .iter()
        .map(|p| (p.key.clone(), p.id.to_string()))
        .collect();
    Ok((
        Choices {
            projects: projects
                .into_iter()
                .map(|p| (p.id.to_string(), format!("{} - {}", p.key, p.name)))
                .collect(),
            personas: personas
                .into_iter()
                .map(|p| (p.id.to_string(), p.name))
                .collect(),
        },
        keys,
    ))
}

#[derive(Debug, Default, Deserialize)]
struct ComposeQuery {
    project: Option<String>,
}

/// `GET /tasks/compose?project=KEY`: the composer, with the project of the current filter as
/// default.
async fn composer(State(state): State<WebState>, Query(query): Query<ComposeQuery>) -> Response {
    let (choices, keys) = match choices(&state).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    let project = query
        .project
        .and_then(|key| keys.into_iter().find(|(k, _)| *k == key))
        .map(|(_, id)| id)
        .unwrap_or_default();
    let values = TaskForm {
        project,
        ..TaskForm::default()
    };
    composer_dialog(&values, &FormErrors::default(), &choices).into_response()
}

async fn task_create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<TaskForm>,
) -> Response {
    let (choices, _) = match choices(&state).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    let mut errors = FormErrors::default();
    let description = form.description.trim().to_string();
    if description.is_empty() {
        errors.add("description", "Describe what the agent should do.");
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
        errors.add("persona", "Choose an assignee.");
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
    let (Some(project), Some(persona), true) = (project, persona, errors.is_empty()) else {
        return invalid(composer_form(&form, &errors, &choices));
    };
    let created = df_db::create_task(
        &state.pool,
        NewTask {
            project_id: ProjectId::from_uuid(project),
            persona_id: PersonaId::from_uuid(persona),
            title: None,
            description,
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
    .with_details(&serde_json::json!({ "title": task.title }));
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

/// Loads a task by key; the error is the response to send.
async fn task_by_key(state: &WebState, key: &str) -> Result<TaskDetail, Box<Response>> {
    match df_db::task_views::get_task_detail_by_key(&state.pool, key).await {
        Ok(Some(task)) => Ok(task),
        Ok(None) => Err(Box::new(not_found("The task"))),
        Err(e) => Err(Box::new(internal_error(&e))),
    }
}

fn has_choice(options: &[(String, String)], id: &uuid::Uuid) -> bool {
    options.iter().any(|(value, _)| *value == id.to_string())
}

/// `POST /tasks/{key}/title`: a board member renames the task; the title is locked afterwards.
async fn task_rename(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    Form(form): Form<TitleForm>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    if sanitize_title(&form.title).is_none() {
        return invalid(head_region(&task, Some("Enter a title.")));
    }
    if title_too_long(&form.title) {
        return invalid(head_region(
            &task,
            Some(&format!("At most {TITLE_MAX_CHARS} characters.")),
        ));
    }
    let id = TaskId::from_uuid(task.id);
    if let Err(e) = df_db::rename_task(&state.pool, id, user.principal(), &form.title).await {
        return internal_error(&e);
    }
    let task = match df_db::task_views::get_task_detail(&state.pool, id).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let mut response = head_region(&task, None).into_response();
    response
        .headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static("title-saved"));
    response
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
    let task = match task_by_key(&state, &task_key.to_string()).await {
        Ok(task) => task,
        Err(response) => return *response,
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
        return invalid(body);
    }
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static("message-queued"));
    response
}

#[cfg(test)]
mod tests;
