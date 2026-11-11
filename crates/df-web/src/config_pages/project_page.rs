//! The settings page of a project, `/projects/{key}` (`configuration-management`, "Project
//! settings page"): a back link, the project's heading, the tabs of [`super::project_form`] and one
//! "Save project" for General and Security.
//!
//! A save swaps only the form and, out of band, the heading and the status line of the header, so
//! the live region exists before its text changes and screen readers announce "Project saved".

use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use df_core::CredentialRef;
use df_db::catalog::ProjectRecord;
use maud::{Markup, html};
use serde::Deserialize;

use super::{
    Editing, FormContext, SettingsBack, project_form, project_values, saved_response,
    settings_page_frame,
};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{FormErrors, forge_icon, project_forge_kind};
use crate::layout::{Nav, Shell, layout};
use crate::project_subscriptions;
use crate::responses::{internal_error, not_found};

#[derive(Debug, Default, Deserialize)]
pub(super) struct PageQuery {
    tab: Option<String>,
}

/// `GET /projects/{key}?tab=`: the page. An unknown or forbidden `tab` opens General.
pub(super) async fn page(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    Query(query): Query<PageQuery>,
) -> Response {
    let (project, credentials) = match tokio::try_join!(
        df_db::catalog::get_project_by_key(&state.pool, &key),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(project), credentials)) => (project, credentials),
        Ok((None, _)) => return not_found("The project"),
        Err(e) => return internal_error(&e),
    };
    let editing = Editing::of(true, &user);
    let form = match form_of(
        &state,
        &project,
        &credentials,
        editing,
        query.tab.as_deref(),
    )
    .await
    {
        Ok(form) => form,
        Err(response) => return *response,
    };
    let body = settings_page_frame(
        "project-settings",
        SettingsBack {
            href: "/projects",
            label: "Projects",
        },
        heading(&project, &state),
        "Save project",
        html! {
            (form)
            // The subscription editor's form cannot sit inside the project form.
            @if editing == Editing::AsAdmin { (project_subscriptions::form_anchor()) }
        },
    );
    layout(
        &project.name,
        Nav::ProjectSettings,
        &Shell::new(&user, &state),
        body,
    )
    .into_response()
}

/// The answer of a successful save: the form with the stored values and, out of band, the heading
/// (the name may have changed) and the "Project saved" status. The tab stays open.
pub(super) async fn saved(
    state: &WebState,
    user: &CurrentUser,
    project: &ProjectRecord,
    credentials: &[CredentialRef],
    tab: &str,
) -> Response {
    let form = match form_of(
        state,
        project,
        credentials,
        Editing::of(true, user),
        Some(tab),
    )
    .await
    {
        Ok(form) => form,
        Err(response) => return *response,
    };
    saved_response(
        form,
        "project-heading",
        heading(project, state),
        "Project saved",
    )
}

/// The project form with the stored values, opened on `tab`.
async fn form_of(
    state: &WebState,
    project: &ProjectRecord,
    credentials: &[CredentialRef],
    editing: Editing,
    tab: Option<&str>,
) -> Result<Markup, Box<Response>> {
    let policy = df_db::push_checks::push_policy(&state.pool, project.id)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(project_form(
        &format!("/projects/{}", project.key),
        &project_values(project, &policy),
        &FormErrors::default(),
        credentials,
        state,
        FormContext {
            editing,
            project: Some(project),
            tab: tab.unwrap_or_default(),
        },
    ))
}

/// Key, name and forge icon of the project.
fn heading(project: &ProjectRecord, state: &WebState) -> Markup {
    html! {
        div id="project-heading" class="flex min-w-0 items-center gap-3" {
            span class="rounded-md bg-sunk px-2 py-0.5 font-mono text-xs font-medium text-muted" { (project.key) }
            h1 class="min-w-0 truncate text-2xl font-semibold" { (project.name) }
            (forge_icon(project_forge_kind(&state.forges, project).as_ref()))
        }
    }
}
