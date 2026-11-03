//! Task pages: the three-column workspace (list, transcript, details), the composer, rename and
//! the message input (`specs/task-ui/spec.md`, design D4 to D8).

mod resume;
mod view;

use std::time::Duration;

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
use df_db::volumes::VolumeError;
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{
    ButtonKind, EmptyPanel, EmptySize, FormErrors, Icon, action_class, icon, logo,
};
use crate::layout::{Shell, View, workspace_layout};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};
use crate::sse;
use crate::transcript::{Folder, render_transcript};
pub(crate) use resume::ResumeNote;
pub(crate) use view::{
    CancelNote, Choices, Filters, TaskForm, composer_action, details_region, head_region,
    list_column, pending_region, runs_region, stoppable, task_rows,
};
use view::{composer_dialog, composer_form, details_column, message_area};

/// How many transcript rows the detail page renders at once.
const PAGE_BATCH: i64 = 1000;
/// How long marking a task done waits for the forge to mark its PR ready. The status is already
/// committed; a forge that does not answer must not keep the page waiting through the plugin's
/// retries.
const MARK_READY_TIMEOUT: Duration = Duration::from_secs(10);
/// Shown in the details when the provider could not delete the volume.
const RESET_FAILED: &str = "The workspace could not be reset. Try again later.";

pub(crate) fn routes() -> Router<WebState> {
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
        .route("/tasks/{key}/done", post(task_done))
        .route("/tasks/{key}/stop", post(task_stop))
        .route("/tasks/{key}/cancel", post(task_cancel))
        .route("/tasks/{key}/reset-workspace", post(task_reset_workspace))
}

/// The query of the list pages.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListQuery {
    status: Option<String>,
    project: Option<String>,
}

impl ListQuery {
    pub(crate) fn filters(&self) -> Filters {
        Filters::new(self.status.as_deref(), self.project.as_deref())
    }
}

/// Loads everything the stream and the page render for one task.
pub(crate) struct DetailData {
    pub task: TaskDetail,
    pub runs: Vec<RunView>,
    pub pending: Vec<df_db::TaskMessageRow>,
}

pub(crate) async fn load_detail(
    state: &WebState,
    task: TaskDetail,
) -> Result<DetailData, df_db::DbError> {
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
pub(crate) struct TitleForm {
    title: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct MessageForm {
    message: String,
}

/// Everything the transcript and details columns render for one task.
struct Selected {
    detail: TaskDetail,
    runs: Vec<RunView>,
    pending: Vec<df_db::TaskMessageRow>,
    folder: Folder,
    show_hidden: bool,
    max_auto_resumes: u32,
    requests: Vec<crate::requests::CardData>,
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
    let shell = Shell::new(user, state);
    let (title, view) = match &selected {
        Some(s) => (format!("{} {}", s.detail.key, s.detail.title), View::Task),
        None => ("Tasks".to_string(), View::List),
    };
    let body = html! {
        // `collapsed` (the details column of 1280 px and more) is remembered in this browser.
        div class="ws" ":data-collapsed"="collapsed" "x-on:keydown.escape.window"="details = false"
            x-data="workspace" {
            (list_column(&items, &projects, filters, current, &shell))
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
    workspace_layout(&title, view, &compose, &shell, body).into_response()
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
            // Follows new and growing items (DOM mutations) and a changed viewport or content height (queued
            // messages, loaded images) while the reader is within 48 px of the bottom; a swapped-in container
            // re-runs `x-init`, so opening another task lands at the bottom too.
            div id="transcript-scroll" class="min-h-0 flex-1 overflow-y-auto p-4"
                x-data="{ pinned: true }"
                x-init="const down = () => { if (pinned) $el.scrollTop = $el.scrollHeight }; down(); new MutationObserver(down).observe($el, { childList: true, subtree: true, characterData: true }); new ResizeObserver(down).observe($el); new ResizeObserver(down).observe($el.querySelector('#transcript'))"
                "x-on:scroll"="pinned = $el.scrollHeight - $el.scrollTop - $el.clientHeight < 48" {
                (render_transcript(s.folder.items(), &timeline_cards(&s.requests)))
            }
            (message_area(key, &s.detail.persona_name, s.detail.status, &s.runs, &s.pending, s.show_hidden))
        }
        (details_column(
            &s.detail,
            &s.runs,
            &CancelNote::new(&s.runs, &s.pending),
            &ResumeNote::new(&s.detail, &s.runs, s.max_auto_resumes),
        ))
    }
}

/// The request cards of a task with the time they were raised, for the timeline.
fn timeline_cards(
    requests: &[crate::requests::CardData],
) -> Vec<(chrono::DateTime<chrono::Utc>, Markup)> {
    requests
        .iter()
        .map(|card| {
            (
                card.row.created_at,
                crate::requests::request_card(card, None),
            )
        })
        .collect()
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
    let mut folder =
        Folder::new(task.persona_name.clone()).with_hidden_blocks(state.show_hidden_blocks);
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
    let requests =
        match crate::requests::task_cards(&state, id, &data.task.key, &data.task.title).await {
            Ok(requests) => requests,
            Err(e) => return internal_error(&e),
        };
    let selected = Selected {
        detail: data.task,
        runs: data.runs,
        pending: data.pending,
        folder,
        show_hidden: state.show_hidden_blocks,
        max_auto_resumes: state.max_auto_resumes,
        requests,
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

/// `POST /tasks/{key}/done`: a board member marks the task done and its PR, if any, becomes
/// ready for review. The answer is the details region as the task is now, also when the task
/// could not be marked done (409).
async fn task_done(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(task.id);
    let status = match df_db::mark_task_done(&state.pool, id, user.principal()).await {
        Ok(()) => {
            mark_pr_ready(&state, id).await;
            StatusCode::OK
        }
        Err(df_db::DbError::Conflict(_)) => StatusCode::CONFLICT,
        Err(e) => return internal_error(&e),
    };
    details_answer(&state, id, status, None).await
}

/// Marks the PR of a task ready for review, without a summary. Best effort: a forge failure is
/// logged and does not undo the status (design D5).
async fn mark_pr_ready(state: &WebState, id: TaskId) {
    let task = match df_db::get_task(&state.pool, id).await {
        Ok(Some(task)) => task,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, task_id = %id, "failed to load the task to mark its pull request ready");
            return;
        }
    };
    let Some(number) = task.pr_number.and_then(|n| u64::try_from(n).ok()) else {
        return;
    };
    mark_ready_bounded(
        state.forge.as_ref(),
        &task.project_id,
        number,
        MARK_READY_TIMEOUT,
    )
    .await;
}

/// Marks PR `number` ready for review, waiting at most `deadline`. Failures and an expired
/// deadline are logged; the forge may still apply the change later.
pub(crate) async fn mark_ready_bounded(
    forge: &dyn df_core::ForgeClient,
    project_id: &ProjectId,
    number: u64,
    deadline: Duration,
) {
    match tokio::time::timeout(deadline, forge.mark_pull_request_ready(project_id, number)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, %project_id, number, "failed to mark the pull request ready");
        }
        Err(_elapsed) => {
            tracing::warn!(%project_id, number, "marking the pull request ready timed out");
        }
    }
}

/// `POST /tasks/{key}/stop`: a board member stops the active run. The page follows through the
/// live events, so the answer has no content (409 if there is no run to stop).
async fn task_stop(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    match df_db::stop_run(&state.pool, TaskId::from_uuid(task.id), user.principal()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(df_db::DbError::Conflict(_)) => StatusCode::CONFLICT.into_response(),
        Err(e) => internal_error(&e),
    }
}

/// `POST /tasks/{key}/cancel`: a board member cancels the task (the dialog asked before). The
/// answer is the details region as the task is now, also when it could not be cancelled (409).
async fn task_cancel(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(task.id);
    let status = match df_db::cancel_task(&state.pool, id, user.principal()).await {
        Ok(()) => StatusCode::OK,
        Err(df_db::DbError::Conflict(_)) => StatusCode::CONFLICT,
        Err(e) => return internal_error(&e),
    };
    details_answer(&state, id, status, None).await
}

/// `POST /tasks/{key}/reset-workspace`: a board member deletes the task's volume (the dialog asked
/// before). The answer is the details region, also when a run is active (409) or the provider
/// failed (503, with an inline error).
async fn task_reset_workspace(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
) -> Response {
    let task = match task_by_key(&state, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(task.id);
    let reset = df_db::volumes::reset_workspace(
        &state.pool,
        state.provider.as_ref(),
        id,
        user.principal(),
        df_db::volumes::DELETE_TIMEOUT,
    )
    .await;
    let (status, error) = match reset {
        Ok(()) => (StatusCode::OK, None),
        Err(VolumeError::Db(df_db::DbError::Conflict(_))) => (StatusCode::CONFLICT, None),
        Err(VolumeError::Db(df_db::DbError::NotFound(_))) => return not_found("The task"),
        Err(VolumeError::Provider(error)) => {
            tracing::error!(%error, task = %task.key, "resetting the workspace failed");
            (StatusCode::SERVICE_UNAVAILABLE, Some(RESET_FAILED))
        }
        Err(e) => return internal_error(&e),
    };
    details_answer(&state, id, status, error).await
}

/// The details region of the task as it is now, with `status` and an optional inline `error`.
async fn details_answer(
    state: &WebState,
    id: TaskId,
    status: StatusCode,
    error: Option<&str>,
) -> Response {
    let task = match df_db::task_views::get_task_detail(&state.pool, id).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    match load_detail(state, task).await {
        Ok(data) => (
            status,
            details_region(
                &data.task,
                &CancelNote::new(&data.runs, &data.pending),
                &ResumeNote::new(&data.task, &data.runs, state.max_auto_resumes),
                error,
            ),
        )
            .into_response(),
        Err(e) => internal_error(&e),
    }
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
            hidden: false,
        }];
        match df_db::queue_message(&state.pool, id, user.principal(), content).await {
            Ok(_) => return stream_renders_stack(Some("message-queued")),
            Err(e) => return internal_error(&e),
        }
    };
    match stack(&state, &task, error).await {
        Ok(body) => invalid(body),
        Err(response) => *response,
    }
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
        .map_err(|_invalid_id| Box::new(not_found("The message")))?;
    Ok((task, prompt))
}

/// Answers a successful edit or removal without a body (the stream re-renders the stack), a
/// failed one with the stack and the error: 409 when the agent has the message already, 422 for
/// an empty text.
async fn queued_change(
    state: &WebState,
    task: &TaskDetail,
    result: Result<(), df_db::DbError>,
    saved: Option<&'static str>,
) -> Response {
    let (status, error) = match result {
        Ok(()) => return stream_renders_stack(saved),
        Err(df_db::DbError::Conflict(_)) => (
            StatusCode::CONFLICT,
            "The agent has already received this message.",
        ),
        Err(df_db::DbError::InvalidData(_)) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "Write a message first.")
        }
        Err(e) => return internal_error(&e),
    };
    match stack(state, task, Some(error)).await {
        Ok(body) => (status, body).into_response(),
        Err(response) => *response,
    }
}

/// The answer to a successful queue change. The event stream is the only writer of `#pending`
/// on success: it renders the stack after the change's notification, and a response that
/// arrived after a newer stream update would otherwise put back a stale stack that the stream,
/// which only sends changed regions, never corrects.
fn stream_renders_stack(trigger: Option<&'static str>) -> Response {
    let mut response = StatusCode::OK.into_response();
    response
        .headers_mut()
        .insert("HX-Reswap", HeaderValue::from_static("none"));
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
    Ok(pending_region(
        &task.key,
        &task.persona_name,
        task.status.as_str(),
        &pending,
        state.show_hidden_blocks,
        error,
    ))
}

fn with_header(mut response: Response, trigger: &'static str) -> Response {
    response
        .headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static(trigger));
    response
}

#[cfg(test)]
mod tests;
