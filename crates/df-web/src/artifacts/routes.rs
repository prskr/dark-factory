//! Routes of artifacts: the tab fragment, the artifact page and the image endpoint.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use df_core::TaskId;
use df_db::artifacts::{list_artifacts, read_artifact_content, text_previews};
use maud::html;

use super::view::{ArtifactCard, Place, artifact_view, cards, details_heading, type_label};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::layout::{Nav, Shell, layout};
use crate::responses::{internal_error, not_found};

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/tasks/{key}/artifacts", get(tab))
        .route("/tasks/{key}/artifacts/{name}", get(page))
        .route("/tasks/{key}/artifacts/{name}/raw", get(raw))
}

/// The cards of a task with the start of their text.
pub(crate) async fn load_cards(
    state: &WebState,
    task: TaskId,
) -> Result<Vec<ArtifactCard>, df_db::DbError> {
    let (metas, mut previews) = tokio::try_join!(
        list_artifacts(&state.pool, task),
        text_previews(&state.pool, task)
    )?;
    Ok(metas
        .into_iter()
        .map(|meta| {
            let preview = previews.remove(&meta.name).unwrap_or_default();
            ArtifactCard { meta, preview }
        })
        .collect())
}

/// `GET /tasks/{key}/artifacts`: the cards, and the details heading out of band.
async fn tab(State(state): State<WebState>, Path(key): Path<String>) -> Response {
    let task = match df_db::task_views::get_task_detail_by_key(&state.pool, &key).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let artifacts = match load_cards(&state, TaskId::from_uuid(task.id)).await {
        Ok(artifacts) => artifacts,
        Err(e) => return internal_error(&e),
    };
    html! {
        (cards(&task.key, &artifacts))
        (details_heading(artifacts.len(), true))
    }
    .into_response()
}

/// `GET /tasks/{key}/artifacts/{name}`: the page, or only the view for the split view.
async fn page(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    headers: HeaderMap,
    Path((key, name)): Path<(String, String)>,
) -> Response {
    let task = match df_db::task_views::get_task_detail_by_key(&state.pool, &key).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let id = TaskId::from_uuid(task.id);
    let (artifact, all) = match tokio::try_join!(
        read_artifact_content(&state.pool, id, &name),
        list_artifacts(&state.pool, id)
    ) {
        Ok(found) => found,
        Err(e) => return internal_error(&e),
    };
    let Some(artifact) = artifact else {
        return not_found("The artifact");
    };
    let images: Vec<String> = all
        .into_iter()
        .filter(|m| m.kind == "image")
        .map(|m| m.name)
        .collect();
    if crate::auth::is_htmx(&headers) {
        let view = artifact_view(
            &task.key,
            &artifact.meta,
            &artifact.content,
            &images,
            Place::Split,
        );
        return view.into_response();
    }
    let view = artifact_view(
        &task.key,
        &artifact.meta,
        &artifact.content,
        &images,
        Place::Page,
    );
    let body = html! {
        div class="mx-auto flex min-h-dvh max-w-3xl flex-col" {
            nav class="flex items-center gap-2 px-4 py-3 text-sm" {
                a href={"/tasks/" (task.key)} class="font-medium text-accent-text hover:underline" { "\u{2190} " (task.key) }
                span class="truncate text-muted" { (task.title) }
            }
            (view)
        }
    };
    let title = format!(
        "{} {} ({})",
        task.key,
        artifact.meta.name,
        type_label(&artifact.meta)
    );
    layout(&title, Nav::Tasks, &Shell::new(&user, &state), body).into_response()
}

/// `GET /tasks/{key}/artifacts/{name}/raw`: the bytes of an image artifact.
async fn raw(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path((key, name)): Path<(String, String)>,
) -> Response {
    let task = match df_db::task_views::get_task_detail_by_key(&state.pool, &key).await {
        Ok(Some(task)) => task,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return internal_error(&e),
    };
    let artifact = match read_artifact_content(&state.pool, TaskId::from_uuid(task.id), &name).await
    {
        Ok(Some(artifact)) if artifact.meta.kind == "image" => artifact,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return internal_error(&e),
    };
    let etag = format!("\"{}\"", artifact.meta.sha256);
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|t| t.trim().trim_start_matches("W/") == etag)
        });
    let mut response = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        artifact.content.into_response()
    };
    let out = response.headers_mut();
    if !fresh {
        if let Ok(mime) = HeaderValue::from_str(&artifact.meta.mime_type) {
            out.insert(header::CONTENT_TYPE, mime);
        }
        out.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
        out.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("inline"),
        );
    }
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        out.insert(header::ETAG, etag);
    }
    response
}
