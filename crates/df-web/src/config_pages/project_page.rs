//! The settings page of a project, `/projects/{key}` (`configuration-management`, "Project
//! settings page"): a back link, the project's heading, the tabs of [`super::project_form`] and one
//! "Save project" for General and Push checks.
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

use super::{Editing, FormContext, project_form, project_values};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{
    ButtonKind, FormErrors, Icon, action_class, forge_icon, icon, project_forge_kind,
};
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
    let body = html! {
        div id="project-settings" class="mx-auto max-w-5xl" {
            a href="/projects" class="mb-3 inline-flex min-h-11 items-center gap-1 text-sm text-muted hover:text-ink lg:min-h-9" {
                (icon(Icon::ChevronLeft)) "Projects"
            }
            div class="mb-5 flex flex-wrap items-center justify-between gap-x-4 gap-y-2" {
                (heading(&project, &state))
                div class="flex items-center gap-3" {
                    div id="save-status" role="status" aria-live="polite" class="text-sm" {}
                    span class="hidden md:block" {
                        button type="submit" form="form" class=(action_class(ButtonKind::Primary)) { "Save project" }
                    }
                }
            }
            (form)
            // The subscription editor's form cannot sit inside the project form.
            @if editing == Editing::AsAdmin { (project_subscriptions::form_anchor()) }
        }
    };
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
    html! {
        (form)
        template hx type="partial" hx-target="#project-heading" hx-swap="outerHTML" { (heading(project, state)) }
        template hx type="partial" hx-target="#save-status" hx-swap="innerHTML" {
            span class="inline-flex items-center gap-1.5 text-accent-text" { (icon(Icon::Check)) "Project saved" }
        }
    }
    .into_response()
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
