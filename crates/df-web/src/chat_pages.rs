//! Chat pages: the project and chat list, the chat with its transcript and details, the new chat
//! dialog, rename, archive and unarchive (`specs/chat-ui/spec.md`, design D8). Messages, stop and
//! interrupt are the handlers of the task pages, which serve both sections.

mod archive;
mod view;

use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Extension, Router};
use df_core::{PersonaId, ProjectId, TITLE_MAX_CHARS, TaskId, sanitize_title, title_too_long};
use df_db::chat_views::ChatProject;
use df_db::task_views::TaskDetail;
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{EmptyPanel, EmptySize, FormErrors, Icon, icon};
use crate::layout::{Shell, View, chat_layout};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};
use crate::sse;
use crate::task_pages::{
    DetailData, Section, load_detail, load_folder, message_area, message_create, message_edit,
    message_remove, task_by_key, task_interrupt, task_stop, timeline_cards, transcript_scroll,
};
use crate::transcript::{Folder, render_transcript};
pub(crate) use view::{ChatForm, archived_notice, chat_rows, head_region, overview_region};

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/chats", get(chat_list).post(chat_create))
        .route("/chats/compose", get(compose))
        .route("/chats/archive", get(archive::archive_modal))
        .route("/chats/events", get(sse::chat_list_events))
        .route("/chats/{key}", get(chat_detail))
        .route("/chats/{key}/events", get(sse::task_events))
        .route("/chats/{key}/messages", post(message_create))
        .route(
            "/chats/{key}/messages/{id}",
            patch(message_edit).delete(message_remove),
        )
        .route("/chats/{key}/title", post(chat_rename))
        .route("/chats/{key}/stop", post(task_stop))
        .route("/chats/{key}/interrupt", post(task_interrupt))
        .route("/chats/{key}/archive", post(chat_archive))
        .route("/chats/{key}/unarchive", post(chat_unarchive))
}

#[derive(Debug, Default, Deserialize)]
struct ProjectQuery {
    project: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TitleForm {
    title: String,
}

/// Everything the center and the details render for one chat.
pub(crate) struct Selected {
    pub data: DetailData,
    pub base_branch: String,
    pub folder: Folder,
    pub requests: Vec<crate::requests::CardData>,
    pub artifacts: Vec<crate::artifacts::ArtifactCard>,
}

/// Loads the chat: its rows, transcript, requests, artifacts and checkout.
pub(crate) async fn load_selected(
    state: &WebState,
    detail: TaskDetail,
) -> Result<Selected, df_db::DbError> {
    let id = TaskId::from_uuid(detail.id);
    let folder = load_folder(state, id, crate::task_pages::persona_label(&detail)).await?;
    let base_branch = df_db::base_branch(&state.pool, id).await?;
    let requests = crate::requests::task_cards(state, id, &detail.key, &detail.title).await?;
    let artifacts = crate::artifacts::load_cards(state, id).await?;
    let data = load_detail(state, detail).await?;
    Ok(Selected {
        data,
        base_branch,
        folder,
        requests,
        artifacts,
    })
}

/// `GET /chats[?project=KEY]`: the project asked for, else the one of the chat that was active
/// last, else the first.
async fn chat_list(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ProjectQuery>,
) -> Response {
    let projects = match df_db::chat_views::list_chat_projects(&state.pool).await {
        Ok(projects) => projects,
        Err(e) => return internal_error(&e),
    };
    let latest = if query.project.is_some() {
        None
    } else {
        match df_db::chat_views::latest_chat_project(&state.pool).await {
            Ok(latest) => latest,
            Err(e) => return internal_error(&e),
        }
    };
    let wanted = query.project.or(latest);
    let picked = projects
        .iter()
        .position(|p| Some(&p.key) == wanted.as_ref())
        .unwrap_or_default();
    workspace(&state, &user, projects, picked, None).await
}

/// `GET /chats/{key}`.
async fn chat_detail(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    section: Section,
    Path(key): Path<String>,
) -> Response {
    let detail = match task_by_key(&state, section, &key).await {
        Ok(detail) => detail,
        Err(response) => return *response,
    };
    let projects = match df_db::chat_views::list_chat_projects(&state.pool).await {
        Ok(projects) => projects,
        Err(e) => return internal_error(&e),
    };
    let picked = projects
        .iter()
        .position(|p| p.key == detail.project_key)
        .unwrap_or_default();
    let selected = match load_selected(&state, detail).await {
        Ok(selected) => selected,
        Err(e) => return internal_error(&e),
    };
    workspace(&state, &user, projects, picked, Some(selected)).await
}

/// Renders the page: the list column of `projects[picked]` and the selected chat, or the empty
/// state.
async fn workspace(
    state: &WebState,
    user: &CurrentUser,
    projects: Vec<ChatProject>,
    picked: usize,
    selected: Option<Selected>,
) -> Response {
    let shell = Shell::new(user, state);
    let Some(project) = projects.get(picked) else {
        let body = html! {
            div class="flex min-h-dvh items-center justify-center p-6" {
                (EmptyPanel {
                    size: EmptySize::Workspace,
                    icon: Some(icon(Icon::Chat)),
                    title: "No projects yet",
                    text: "A chat belongs to a project. Create a project first.",
                    phone_hint: None,
                    action: html! { a href="/projects" class="text-sm font-medium text-accent-text underline" { "Go to projects" } },
                }.render())
            }
        };
        return chat_layout("Chats", View::List, "/chats", &shell, body).into_response();
    };
    let chats = match df_db::chat_views::list_chats(&state.pool, &project.key, false, None).await {
        Ok(chats) => chats,
        Err(e) => return internal_error(&e),
    };
    let current = selected.as_ref().map(|s| s.data.task.key.as_str());
    let compose = format!(
        "/chats/compose?project={}",
        url::form_urlencoded::byte_serialize(project.key.as_bytes()).collect::<String>()
    );
    let (title, screen) = match &selected {
        Some(s) => (
            format!("{} {}", s.data.task.key, s.data.task.title),
            View::Task,
        ),
        None => ("Chats".to_owned(), View::List),
    };
    let body = html! {
        // The collapse state of both columns is remembered in this browser (`chatLayout`).
        div class="ws" ":data-collapsed"="collapsed" ":data-list-collapsed"="listCollapsed" "x-on:keydown.escape.window"="details = false"
            x-data="chatLayout" {
            (view::list_column(&projects, project, &chats, current, &shell))
            div id="ws-detail" x-data=[selected.as_ref().map(|s| crate::artifacts::split_args(&s.data.task.key))] {
                @match &selected {
                    Some(s) => (chat_columns(s, state.show_hidden_blocks)),
                    None => (view::center_empty(project)),
                }
            }
        }
    };
    chat_layout(&title, screen, &compose, &shell, body).into_response()
}

/// The center and the details of a chat.
fn chat_columns(s: &Selected, show_hidden: bool) -> Markup {
    let task = &s.data.task;
    let key = task.key.as_str();
    let events_url = format!("/chats/{key}/events?after={}", s.folder.last_id());
    html! {
        div hx-sse:connect=(events_url) hx-swap="none" {}
        // The scrim closes the details panel where it floats over the transcript.
        div x-show="details" x-cloak "x-on:click"="details = false" class="fixed inset-0 z-30 bg-black/40 xl:hidden" {}
        section class="ws-main" aria-label="Chat" {
            (head_region(task))
            div class="flex min-h-0 flex-1" {
                div class="flex min-w-0 flex-1 flex-col" {
                    (transcript_scroll(render_transcript(s.folder.items(), &timeline_cards(&s.requests))))
                    @if task.archived_at.is_some() {
                        (archived_notice(key))
                    } @else {
                        (message_area(key, &task.persona_display_name, task.status, &s.data.runs, &s.data.pending, show_hidden))
                    }
                }
                (crate::artifacts::split_pane())
            }
        }
        (view::details_column(task, &s.base_branch, &s.data.runs, &s.data.subtasks, &s.artifacts))
    }
}

#[derive(Debug, Default, Deserialize)]
struct ComposeQuery {
    project: Option<String>,
}

/// `GET /chats/compose?project=KEY`: the new chat dialog of the project.
async fn compose(State(state): State<WebState>, Query(query): Query<ComposeQuery>) -> Response {
    let (projects, personas) = match tokio::try_join!(
        df_db::chat_views::list_chat_projects(&state.pool),
        df_db::catalog::list_personas(&state.pool)
    ) {
        Ok(loaded) => loaded,
        Err(e) => return internal_error(&e),
    };
    let Some(project) = projects
        .iter()
        .find(|p| Some(&p.key) == query.project.as_ref())
    else {
        return not_found("The project");
    };
    if personas.is_empty() {
        return view::nothing_to_choose().into_response();
    }
    view::compose_dialog(
        project,
        &personas,
        &ChatForm::default(),
        &FormErrors::default(),
    )
    .into_response()
}

/// `POST /chats`: starts a chat in the project with the persona and the first message, and opens
/// it.
async fn chat_create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ChatForm>,
) -> Response {
    let (projects, personas) = match tokio::try_join!(
        df_db::chat_views::list_chat_projects(&state.pool),
        df_db::catalog::list_personas(&state.pool)
    ) {
        Ok(loaded) => loaded,
        Err(e) => return internal_error(&e),
    };
    let Some(project) = projects.iter().find(|p| p.key == form.project) else {
        return not_found("The project");
    };
    let mut errors = FormErrors::default();
    let persona = personas.iter().find(|p| p.id.to_string() == form.persona);
    if persona.is_none() {
        errors.add("persona", "Choose a persona.");
    }
    if form.message.trim().is_empty() {
        errors.add("message", "Write a first message.");
    }
    let (Some(persona), true) = (persona, errors.is_empty()) else {
        return invalid(view::compose_form(project, &personas, &form, &errors));
    };
    let project_id = match project_id(&state, &project.key).await {
        Ok(Some(id)) => id,
        Ok(None) => return not_found("The project"),
        Err(e) => return internal_error(&e),
    };
    let created = df_db::chats::create_chat(
        &state.pool,
        df_db::chats::NewChat {
            project_id,
            persona_id: PersonaId::from_uuid(persona.id),
            author: user.principal(),
            text: form.message.clone(),
        },
    )
    .await;
    match created {
        Ok((chat, _)) => htmx_redirect(&format!("/chats/{}", chat.key)),
        Err(e) => internal_error(&e),
    }
}

async fn project_id(state: &WebState, key: &str) -> Result<Option<ProjectId>, df_db::DbError> {
    let projects = df_db::catalog::list_projects(&state.pool).await?;
    Ok(projects
        .into_iter()
        .find(|p| p.key == key)
        .map(|p| ProjectId::from_uuid(p.id)))
}

/// The overview of the chat as it is now, with `status` and an optional inline `error`.
async fn overview_answer(
    state: &WebState,
    id: TaskId,
    status: StatusCode,
    error: Option<&str>,
) -> Response {
    let detail = match df_db::task_views::get_task_detail(&state.pool, id).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return not_found("The chat"),
        Err(e) => return internal_error(&e),
    };
    match df_db::base_branch(&state.pool, id).await {
        Ok(base) => (status, overview_region(&detail, &base, error)).into_response(),
        Err(e) => internal_error(&e),
    }
}

/// `POST /chats/{key}/title`: a board member renames the chat; the topic is locked afterwards.
async fn chat_rename(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    section: Section,
    Path(key): Path<String>,
    Form(form): Form<TitleForm>,
) -> Response {
    let detail = match task_by_key(&state, section, &key).await {
        Ok(detail) => detail,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(detail.id);
    let problem = if sanitize_title(&form.title).is_none() {
        Some("Enter a topic.".to_owned())
    } else if title_too_long(&form.title) {
        Some(format!("At most {TITLE_MAX_CHARS} characters."))
    } else {
        None
    };
    if let Some(problem) = problem {
        return overview_answer(&state, id, StatusCode::UNPROCESSABLE_ENTITY, Some(&problem)).await;
    }
    if let Err(e) = df_db::rename_task(&state.pool, id, user.principal(), &form.title).await {
        return internal_error(&e);
    }
    overview_answer(&state, id, StatusCode::OK, None).await
}

/// `POST /chats/{key}/archive`: archives an idle chat and goes back to the list; 409 with the
/// reason in the overview when the chat can't be archived now.
async fn chat_archive(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    section: Section,
    Path(key): Path<String>,
) -> Response {
    let detail = match task_by_key(&state, section, &key).await {
        Ok(detail) => detail,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(detail.id);
    match df_db::chats::archive_chat(&state.pool, id, user.principal()).await {
        Ok(()) => htmx_redirect(&format!(
            "/chats?project={}",
            url::form_urlencoded::byte_serialize(detail.project_key.as_bytes()).collect::<String>()
        )),
        Err(df_db::DbError::Conflict(reason)) => {
            overview_answer(&state, id, StatusCode::CONFLICT, Some(&reason)).await
        }
        Err(e) => internal_error(&e),
    }
}

/// `POST /chats/{key}/unarchive`: opens the chat again; a message that waited starts a run.
async fn chat_unarchive(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    section: Section,
    Path(key): Path<String>,
) -> Response {
    let detail = match task_by_key(&state, section, &key).await {
        Ok(detail) => detail,
        Err(response) => return *response,
    };
    let id = TaskId::from_uuid(detail.id);
    match df_db::chats::unarchive_chat(&state.pool, id, user.principal()).await {
        Ok(()) => htmx_redirect(&format!("/chats/{}", detail.key)),
        Err(df_db::DbError::Conflict(_)) => StatusCode::CONFLICT.into_response(),
        Err(e) => internal_error(&e),
    }
}

#[cfg(test)]
mod tests;
