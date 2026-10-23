//! Task pages: the three-column workspace (list, transcript, details), the composer, rename and
//! the message input (`specs/task-ui/spec.md`, design D4 to D8).

mod view;

use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use axum::{Extension, Router};
use df_core::{
    PersonaId, ProjectId, PromptId, TITLE_MAX_CHARS, TaskId, TaskKey, sanitize_title,
    title_too_long,
};
use df_db::NewTask;
use df_db::task_views::{RunView, TaskDetail};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{
    ButtonKind, EmptyPanel, EmptySize, FormErrors, Icon, action_class, icon, logo,
};
use crate::layout::{View, workspace_layout};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};
use crate::sse;
use crate::transcript::{Folder, render_transcript};
pub use view::{
    Choices, Filters, TaskForm, details_region, head_region, list_column, pending_region,
    runs_region, task_rows,
};
use view::{composer_dialog, composer_form, details_column, message_area};

/// Alpine state of the task workspace (`x-data` of `.ws`), shared by the list, the transcript and
/// the details panel.
///
/// State:
/// - `details`: the details panel is open (below 1280 px).
/// - `collapsed`: the details column is collapsed to its strip (from 1280 px); remembered in
///   `localStorage['df.details.collapsed']`, read and written in `try/catch`.
/// - `sheet`: height of the phone details sheet, `'half'` or `'full'`.
/// - `dragging`: the sheet handle is being dragged (the sheet must not animate then).
/// - `drag`: `{ y, h, el, moved }` of the running drag; `dragged`: the last pointer sequence moved,
///   so the click that follows must not toggle the sheet.
///
/// Thresholds (named values): `closeBelow` is the share of the viewport height under which a
/// released sheet closes, `fullAbove` the share over which it becomes full height (otherwise half),
/// `dragSlop` the pixels a press has to move before it counts as a drag.
///
/// Methods: `setCollapsed(value)`, `sheetToggle()` (tap or keyboard on the handle), and the
/// pointer sequence `sheetDragStart(e)`, `sheetDragMove(e)`, `sheetDragEnd(e)` (pointerup) and
/// `sheetDragCancel(e)` (pointercancel: no click follows, so `dragged` is reset).
const WORKSPACE_STATE: &str = r#"{
  details: false,
  collapsed: false,
  sheet: 'half',
  dragging: false,
  drag: null,
  dragged: false,
  closeBelow: 0.25,
  fullAbove: 0.7,
  dragSlop: 4,
  init() {
    try { this.collapsed = localStorage.getItem('df.details.collapsed') === '1' } catch (e) {}
  },
  setCollapsed(value) {
    this.collapsed = value
    try { localStorage.setItem('df.details.collapsed', value ? '1' : '0') } catch (e) {}
  },
  sheetToggle() {
    if (this.dragged) { this.dragged = false; return }
    this.sheet = this.sheet === 'half' ? 'full' : 'half'
  },
  sheetDragStart(e) {
    const el = e.currentTarget.closest('aside')
    e.currentTarget.setPointerCapture(e.pointerId)
    this.drag = { y: e.clientY, h: el.offsetHeight, el, moved: false }
    this.dragging = true
  },
  sheetDragMove(e) {
    const d = this.drag
    if (!d) return
    const dy = e.clientY - d.y
    if (Math.abs(dy) > this.dragSlop) d.moved = true
    if (d.moved) d.el.style.height = Math.max(0, d.h - dy) + 'px'
  },
  sheetDragEnd(e) {
    const d = this.drag
    this.drag = null
    this.dragging = false
    if (!d) return
    d.el.style.height = ''
    this.dragged = d.moved
    if (!d.moved) return
    const share = (d.h - (e.clientY - d.y)) / window.innerHeight
    if (share < this.closeBelow) { this.details = false; this.sheet = 'half' }
    else this.sheet = share > this.fullAbove ? 'full' : 'half'
  },
  sheetDragCancel(e) {
    const d = this.drag
    this.drag = null
    this.dragging = false
    this.dragged = false
    if (d) d.el.style.height = ''
  },
}"#;

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
        .route(
            "/tasks/{key}/messages/{id}",
            patch(message_edit).delete(message_remove),
        )
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
async fn workspace(
    state: &WebState,
    user: &CurrentUser,
    filters: &Filters,
    selected: Option<Selected>,
) -> Response {
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
        // `collapsed` (the details column of 1280 px and more) is remembered in this browser.
        div class="ws" ":data-collapsed"="collapsed" "x-on:keydown.escape.window"="details = false"
            x-data=(WORKSPACE_STATE) {
            (list_column(&items, &projects, filters, current, &user.display_name))
            div id="ws-detail" {
                @match &selected {
                    Some(s) => (selected_columns(s)),
                    None => section class="ws-main items-center justify-center" aria-label="Task" {
                        (EmptyPanel {
                            size: EmptySize::Workspace,
                            icon: Some(logo("size-7")),
                            title: "No task selected",
                            text: "Pick a task from the list, or describe a new one and a persona starts on it.",
                            phone_hint: None,
                            action: html! {
                                button type="button" hx-get=(compose) hx-target="#composer-slot" hx-swap="innerHTML"
                                    class={(action_class(ButtonKind::Primary)) " mt-1.5"} { (icon(Icon::Plus)) "New task" }
                            },
                        }.render())
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
            div class="min-h-0 flex-1 overflow-y-auto p-4" {
                (render_transcript(s.folder.items()))
            }
            (message_area(key, &s.detail.status, &s.pending))
        }
        (details_column(&s.detail, &s.runs))
    }
}

async fn task_list(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ListQuery>,
) -> Response {
    workspace(&state, &user, &query.filters(), None).await
}

async fn task_detail(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    Query(query): Query<ListQuery>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(task.id);
    let mut folder = Folder::new(task.persona_name.clone());
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
    workspace(&state, &user, &query.filters(), Some(selected)).await
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
    let body = match stack(&state, &task, error).await {
        Ok(body) => body,
        Err(response) => return *response,
    };
    if error.is_some() {
        return invalid(body);
    }
    with_trigger(body, "message-queued")
}

/// `PATCH /tasks/{key}/messages/{id}`: replaces the text of a queued message that the agent has
/// not received yet.
async fn message_edit(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path((key, id)): Path<(String, String)>,
    Form(form): Form<MessageForm>,
) -> Response {
    let (task, prompt) = match queued_target(&state, &key, &id).await {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let result = df_db::edit_queued_message(
        &state.pool,
        TaskId::from_uuid(task.id),
        prompt,
        &form.message,
        user.principal(),
    )
    .await;
    queued_change(&state, &task, result, Some("message-saved")).await
}

/// `DELETE /tasks/{key}/messages/{id}`: removes a queued message that the agent has not received
/// yet.
async fn message_remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path((key, id)): Path<(String, String)>,
) -> Response {
    let (task, prompt) = match queued_target(&state, &key, &id).await {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let result = df_db::remove_queued_message(
        &state.pool,
        TaskId::from_uuid(task.id),
        prompt,
        user.principal(),
    )
    .await;
    queued_change(&state, &task, result, None).await
}

/// The task and prompt of a request; the error is the response to send.
async fn queued_target(
    state: &WebState,
    key: &str,
    id: &str,
) -> Result<(TaskDetail, PromptId), Box<Response>> {
    let task = task_by_key(state, key).await?;
    let prompt = id
        .parse::<PromptId>()
        .map_err(|_| Box::new(not_found("The message")))?;
    Ok((task, prompt))
}

/// Answers an edit or removal with the re-rendered stack: 409 when the agent has the message
/// already, 422 for an empty text.
async fn queued_change(
    state: &WebState,
    task: &TaskDetail,
    result: Result<(), df_db::DbError>,
    saved: Option<&'static str>,
) -> Response {
    let (status, error, trigger) = match result {
        Ok(()) => (StatusCode::OK, None, saved),
        Err(df_db::DbError::Conflict(_)) => (
            StatusCode::CONFLICT,
            Some("The agent has already received this message."),
            None,
        ),
        Err(df_db::DbError::InvalidData(_)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("Write a message first."),
            None,
        ),
        Err(e) => return internal_error(&e),
    };
    let body = match stack(state, task, error).await {
        Ok(body) => body,
        Err(response) => return *response,
    };
    let response = (status, body).into_response();
    match trigger {
        Some(name) => with_header(response, name),
        None => response,
    }
}

/// The queued stack of a task as it is now.
async fn stack(
    state: &WebState,
    task: &TaskDetail,
    error: Option<&str>,
) -> Result<Markup, Box<Response>> {
    let pending = df_db::list_undelivered_messages(&state.pool, TaskId::from_uuid(task.id))
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(pending_region(&task.key, &task.status, &pending, error))
}

fn with_trigger(body: Markup, name: &'static str) -> Response {
    with_header(body.into_response(), name)
}

fn with_header(mut response: Response, trigger: &'static str) -> Response {
    response
        .headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static(trigger));
    response
}

#[cfg(test)]
mod tests;
