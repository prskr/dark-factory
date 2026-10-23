//! Configuration pages: credential references, projects and personas
//! (`specs/configuration-management/spec.md`).

use axum::extract::{Form, Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::{Extension, Router};
use df_core::CredentialRef;
use df_db::DbError;
use df_db::catalog::{
    Actor, CredentialRefInput, PersonaInput, PersonaRecord, ProjectInput, ProjectRecord,
};
use maud::{Markup, html};
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::INPUT_CLASS;
use crate::components::{
    ButtonKind, FormErrors, form_error, link_button, page_header, scroll_box, select_field,
    submit_button, text_field, textarea_field,
};
use crate::layout::{Nav, layout};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};

/// Harness every persona uses in iteration 1.
const HARNESS: &str = "claude-code";

pub fn routes() -> Router<WebState> {
    Router::new()
        .route(
            "/credentials",
            get(credentials_list).post(credential_create),
        )
        .route("/credentials/new", get(credential_new))
        .route(
            "/credentials/{id}",
            get(credential_edit).post(credential_update),
        )
        .route("/projects", get(projects_list).post(project_create))
        .route("/projects/new", get(project_new))
        .route("/projects/{key}", get(project_edit).post(project_update))
        .route("/personas", get(personas_list).post(persona_create))
        .route("/personas/new", get(persona_new))
        .route("/personas/{id}", get(persona_edit).post(persona_update))
}

// ---------------------------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct CredentialForm {
    name: String,
    secret_name: String,
    secret_key: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct ProjectForm {
    key: String,
    name: String,
    repo_url: String,
    issue_tracker_url: String,
    image: String,
    base_branch: String,
    forge_credential_id: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PersonaForm {
    name: String,
    system_prompt: String,
    adapter_command: String,
    model: String,
    harness_credential_id: String,
}

fn required(errors: &mut FormErrors, field: &'static str, value: &str, label: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        errors.add(field, format!("{label} is required."));
    }
    value.to_string()
}

fn https_url(errors: &mut FormErrors, field: &'static str, value: &str) {
    match Url::parse(value) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some() => {}
        _ => errors.add(
            field,
            "Must be an HTTPS URL, for example https://git.example.org/o/r.git.",
        ),
    }
}

/// A Kubernetes object name (DNS-1123 subdomain).
fn is_secret_name(value: &str) -> bool {
    let edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    value.len() <= 253
        && value.starts_with(edge)
        && value.ends_with(edge)
        && value.chars().all(|c| edge(c) || c == '-' || c == '.')
}

/// A key of a Kubernetes Secret.
fn is_secret_key(value: &str) -> bool {
    value.len() <= 253
        && !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn optional_uuid(
    errors: &mut FormErrors,
    field: &'static str,
    value: &str,
    known: &[Uuid],
) -> Option<Uuid> {
    if value.is_empty() {
        return None;
    }
    match Uuid::parse_str(value) {
        Ok(id) if known.contains(&id) => Some(id),
        _ => {
            errors.add(field, "Unknown credential reference.");
            None
        }
    }
}

fn validate_credential(form: &CredentialForm) -> Result<CredentialRefInput, FormErrors> {
    let mut errors = FormErrors::default();
    let name = required(&mut errors, "name", &form.name, "Name");
    let secret_name = required(&mut errors, "secret_name", &form.secret_name, "Secret name");
    let secret_key = required(&mut errors, "secret_key", &form.secret_key, "Key");
    if !secret_name.is_empty() && !is_secret_name(&secret_name) {
        errors.add(
            "secret_name",
            "Must be a Kubernetes name: lowercase letters, digits, '-' and '.'.",
        );
    }
    if !secret_key.is_empty() && !is_secret_key(&secret_key) {
        errors.add(
            "secret_key",
            "May contain letters, digits, '-', '_' and '.'.",
        );
    }
    errors.into_result(CredentialRefInput {
        name,
        secret_name,
        secret_key,
    })
}

fn validate_project(form: &ProjectForm, credentials: &[Uuid]) -> Result<ProjectInput, FormErrors> {
    let mut errors = FormErrors::default();
    let key = form.key.trim().to_string();
    if df_core::ProjectKey::new(key.clone()).is_err() {
        errors.add(
            "key",
            "Use 2 to 10 uppercase letters or digits, starting with a letter.",
        );
    }
    let name = required(&mut errors, "name", &form.name, "Name");
    let repo_url = form.repo_url.trim().to_string();
    https_url(&mut errors, "repo_url", &repo_url);
    let issue_tracker_url =
        Some(form.issue_tracker_url.trim().to_string()).filter(|u| !u.is_empty());
    if let Some(url) = &issue_tracker_url {
        https_url(&mut errors, "issue_tracker_url", url);
    }
    let image = required(&mut errors, "image", &form.image, "Sandbox image");
    let base_branch = required(&mut errors, "base_branch", &form.base_branch, "Base branch");
    let forge_credential_id = optional_uuid(
        &mut errors,
        "forge_credential_id",
        &form.forge_credential_id,
        credentials,
    );
    errors.into_result(ProjectInput {
        key,
        name,
        repo_url,
        issue_tracker_url,
        image,
        base_branch,
        forge_credential_id,
    })
}

fn validate_persona(form: &PersonaForm, credentials: &[Uuid]) -> Result<PersonaInput, FormErrors> {
    let mut errors = FormErrors::default();
    let name = required(&mut errors, "name", &form.name, "Name");
    let system_prompt = required(
        &mut errors,
        "system_prompt",
        &form.system_prompt,
        "System prompt",
    );
    let harness_credential_id = optional_uuid(
        &mut errors,
        "harness_credential_id",
        &form.harness_credential_id,
        credentials,
    );
    errors.into_result(PersonaInput {
        name,
        system_prompt,
        harness: HARNESS.to_string(),
        // Empty means the instance default (`harness.claudeCode.adapterCommand`).
        adapter_command: form
            .adapter_command
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        model: Some(form.model.trim().to_string()).filter(|m| !m.is_empty()),
        harness_credential_id,
    })
}

/// Puts a database conflict on the field it belongs to; any other error is an error page.
fn conflict(errors: &mut FormErrors, field: &'static str, error: DbError) -> Option<Response> {
    match error {
        DbError::Conflict(message) => {
            errors.add(field, message);
            None
        }
        other => Some(internal_error(&other)),
    }
}

// ---------------------------------------------------------------------------------------------
// Credential references
// ---------------------------------------------------------------------------------------------

fn credential_values(c: &CredentialRef) -> CredentialForm {
    CredentialForm {
        name: c.name.clone(),
        secret_name: c.secret_name.clone(),
        secret_key: c.secret_key.clone(),
    }
}

fn credential_form(
    action: &str,
    values: &CredentialForm,
    errors: &FormErrors,
    submit: &str,
) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (text_field("Name", "name", &values.name, errors, None))
            (text_field("Secret name", "secret_name", &values.secret_name, errors,
                Some("Name of the Kubernetes Secret. The value itself is never entered here.")))
            (text_field("Key", "secret_key", &values.secret_key, errors, Some("Key within the Secret.")))
            (submit_button(ButtonKind::Primary, submit))
        }
    }
}

async fn credentials_list(State(state): State<WebState>) -> Response {
    let refs = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(refs) => refs,
        Err(e) => return internal_error(&e),
    };
    page(
        "Credentials",
        Nav::Credentials,
        "/credentials/new",
        "New credential reference",
        html! {
            table class="w-full min-w-max text-left text-sm" {
                thead { tr class="text-faint" { th class="py-2" { "Name" } th { "Secret" } th { "Key" } } }
                tbody {
                    @for c in &refs {
                        tr class="border-t border-line" {
                            td class="py-2" { a class="text-accent-text underline" href={"/credentials/" (c.id)} { (c.name) } }
                            td { (c.secret_name) } td { (c.secret_key) }
                        }
                    }
                }
            }
        },
    )
}

async fn credential_new() -> Response {
    form_page(
        "New credential reference",
        Nav::Credentials,
        credential_form(
            "/credentials",
            &CredentialForm::default(),
            &FormErrors::default(),
            "Create",
        ),
    )
}

async fn credential_edit(State(state): State<WebState>, Path(id): Path<Uuid>) -> Response {
    match df_db::catalog::get_credential_ref(&state.pool, id).await {
        Ok(Some(c)) => form_page(
            "Edit credential reference",
            Nav::Credentials,
            credential_form(
                &format!("/credentials/{id}"),
                &credential_values(&c),
                &FormErrors::default(),
                "Save",
            ),
        ),
        Ok(None) => not_found("The credential reference"),
        Err(e) => internal_error(&e),
    }
}

async fn credential_create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<CredentialForm>,
) -> Response {
    save_credential(state, user, None, form.0).await
}

async fn credential_update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    form: Form<CredentialForm>,
) -> Response {
    save_credential(state, user, Some(id), form.0).await
}

async fn save_credential(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    id: Option<Uuid>,
    form: CredentialForm,
) -> Response {
    let action = id.map_or_else(
        || "/credentials".to_string(),
        |id| format!("/credentials/{id}"),
    );
    let submit = if id.is_some() { "Save" } else { "Create" };
    let errors = match validate_credential(&form) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::catalog::save_credential_ref(&state.pool, actor, id, &input).await {
                Ok(_) => return htmx_redirect("/credentials"),
                Err(DbError::NotFound(_)) => return not_found("The credential reference"),
                Err(e) => {
                    let mut errors = FormErrors::default();
                    if let Some(response) = conflict(&mut errors, "name", e) {
                        return response;
                    }
                    errors
                }
            }
        }
        Err(errors) => errors,
    };
    invalid(credential_form(&action, &form, &errors, submit))
}

// ---------------------------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------------------------

fn project_values(p: &ProjectRecord) -> ProjectForm {
    ProjectForm {
        key: p.key.clone(),
        name: p.name.clone(),
        repo_url: p.repo_url.clone(),
        issue_tracker_url: p.issue_tracker_url.clone().unwrap_or_default(),
        image: p.image.clone(),
        base_branch: p.base_branch.clone(),
        forge_credential_id: p
            .forge_credential_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
    }
}

fn credential_options(credentials: &[CredentialRef]) -> Vec<(String, String)> {
    credentials
        .iter()
        .map(|c| (c.id.to_string(), c.name.clone()))
        .collect()
}

fn project_form(
    action: &str,
    values: &ProjectForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    submit: &str,
    key_locked: bool,
) -> Markup {
    let key_hint = key_locked.then_some("The key cannot change once the project has tasks.");
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (text_field("Key", "key", &values.key, errors, key_hint.or(Some("For example DF; tasks become DF-1, DF-2."))))
            (text_field("Name", "name", &values.name, errors, None))
            (text_field("Repository URL", "repo_url", &values.repo_url, errors, Some("HTTPS only.")))
            (text_field("Issue tracker URL", "issue_tracker_url", &values.issue_tracker_url, errors, Some("Optional.")))
            (text_field("Sandbox image", "image", &values.image, errors, None))
            (text_field("Base branch", "base_branch", &values.base_branch, errors, None))
            (select_field("Forge credential", "forge_credential_id", &credential_options(credentials),
                &values.forge_credential_id, Some("None"), errors))
            (submit_button(ButtonKind::Primary, submit))
        }
    }
}

fn new_project_values() -> ProjectForm {
    ProjectForm {
        base_branch: "main".into(),
        ..ProjectForm::default()
    }
}

async fn projects_list(State(state): State<WebState>) -> Response {
    let projects = match df_db::catalog::list_projects(&state.pool).await {
        Ok(p) => p,
        Err(e) => return internal_error(&e),
    };
    page(
        "Projects",
        Nav::Projects,
        "/projects/new",
        "New project",
        html! {
            table class="w-full min-w-max text-left text-sm" {
                thead { tr class="text-faint" { th class="py-2" { "Key" } th { "Name" } th { "Repository" } } }
                tbody {
                    @for p in &projects {
                        tr class="border-t border-line" {
                            td class="py-2" { a class="text-accent-text underline" href={"/projects/" (p.key)} { (p.key) } }
                            td { (p.name) } td class="break-all" { (p.repo_url) }
                        }
                    }
                }
            }
        },
    )
}

async fn project_new(State(state): State<WebState>) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    form_page(
        "New project",
        Nav::Projects,
        project_form(
            "/projects",
            &new_project_values(),
            &FormErrors::default(),
            &credentials,
            "Create",
            false,
        ),
    )
}

async fn project_edit(State(state): State<WebState>, Path(key): Path<String>) -> Response {
    let (project, credentials) = match tokio::try_join!(
        df_db::catalog::get_project_by_key(&state.pool, &key),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(project), credentials)) => (project, credentials),
        Ok((None, _)) => return not_found("The project"),
        Err(e) => return internal_error(&e),
    };
    form_page(
        "Edit project",
        Nav::Projects,
        project_form(
            &format!("/projects/{key}"),
            &project_values(&project),
            &FormErrors::default(),
            &credentials,
            "Save",
            project.has_tasks,
        ),
    )
}

async fn project_create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<ProjectForm>,
) -> Response {
    save_project(state, user, None, form.0).await
}

async fn project_update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(key): Path<String>,
    form: Form<ProjectForm>,
) -> Response {
    save_project(state, user, Some(key), form.0).await
}

async fn save_project(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    existing_key: Option<String>,
    form: ProjectForm,
) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    let existing = match &existing_key {
        Some(key) => match df_db::catalog::get_project_by_key(&state.pool, key).await {
            Ok(Some(project)) => Some(project),
            Ok(None) => return not_found("The project"),
            Err(e) => return internal_error(&e),
        },
        None => None,
    };
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let errors = match validate_project(&form, &known) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::catalog::save_project(
                &state.pool,
                actor,
                existing.as_ref().map(|p| p.id),
                &input,
            )
            .await
            {
                Ok(_) => return htmx_redirect("/projects"),
                Err(e) => {
                    let mut errors = FormErrors::default();
                    if let Some(response) = conflict(&mut errors, "key", e) {
                        return response;
                    }
                    errors
                }
            }
        }
        Err(errors) => errors,
    };
    let action = existing_key
        .as_ref()
        .map_or_else(|| "/projects".to_string(), |k| format!("/projects/{k}"));
    let submit = if existing.is_some() { "Save" } else { "Create" };
    invalid(project_form(
        &action,
        &form,
        &errors,
        &credentials,
        submit,
        existing.is_some_and(|p| p.has_tasks),
    ))
}

// ---------------------------------------------------------------------------------------------
// Personas
// ---------------------------------------------------------------------------------------------

fn persona_values(p: &PersonaRecord) -> PersonaForm {
    PersonaForm {
        name: p.name.clone(),
        system_prompt: p.system_prompt.clone(),
        adapter_command: p.adapter_command.join(" "),
        model: p.model.clone().unwrap_or_default(),
        harness_credential_id: p
            .harness_credential_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
    }
}

fn persona_form(
    action: &str,
    values: &PersonaForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    submit: &str,
) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (text_field("Name", "name", &values.name, errors, None))
            (textarea_field("System prompt", "system_prompt", &values.system_prompt, 8, errors))
            div class="space-y-1" {
                label class="block text-sm font-medium text-muted" for="harness" { "Harness" }
                select id="harness" name="harness" class=(INPUT_CLASS) {
                    option value=(HARNESS) selected { (HARNESS) }
                }
            }
            (text_field("Adapter command", "adapter_command", &values.adapter_command, errors,
                Some("Optional, space separated. Empty uses the instance default.")))
            (text_field("Model", "model", &values.model, errors, Some("Optional.")))
            (select_field("Harness credential", "harness_credential_id", &credential_options(credentials),
                &values.harness_credential_id, Some("None"), errors))
            (submit_button(ButtonKind::Primary, submit))
        }
    }
}

async fn personas_list(State(state): State<WebState>) -> Response {
    let personas = match df_db::catalog::list_personas(&state.pool).await {
        Ok(p) => p,
        Err(e) => return internal_error(&e),
    };
    page(
        "Personas",
        Nav::Personas,
        "/personas/new",
        "New persona",
        html! {
            table class="w-full min-w-max text-left text-sm" {
                thead { tr class="text-faint" { th class="py-2" { "Name" } th { "Harness" } th { "Model" } } }
                tbody {
                    @for p in &personas {
                        tr class="border-t border-line" {
                            td class="py-2" { a class="text-accent-text underline" href={"/personas/" (p.id)} { (p.name) } }
                            td { (p.harness) } td { (p.model.as_deref().unwrap_or("default")) }
                        }
                    }
                }
            }
        },
    )
}

async fn persona_new(State(state): State<WebState>) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    form_page(
        "New persona",
        Nav::Personas,
        persona_form(
            "/personas",
            &PersonaForm::default(),
            &FormErrors::default(),
            &credentials,
            "Create",
        ),
    )
}

async fn persona_edit(State(state): State<WebState>, Path(id): Path<Uuid>) -> Response {
    let (persona, credentials) = match tokio::try_join!(
        df_db::catalog::get_persona(&state.pool, id),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(persona), credentials)) => (persona, credentials),
        Ok((None, _)) => return not_found("The persona"),
        Err(e) => return internal_error(&e),
    };
    form_page(
        "Edit persona",
        Nav::Personas,
        persona_form(
            &format!("/personas/{id}"),
            &persona_values(&persona),
            &FormErrors::default(),
            &credentials,
            "Save",
        ),
    )
}

async fn persona_create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<PersonaForm>,
) -> Response {
    save_persona(state, user, None, form.0).await
}

async fn persona_update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    form: Form<PersonaForm>,
) -> Response {
    save_persona(state, user, Some(id), form.0).await
}

async fn save_persona(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    id: Option<Uuid>,
    form: PersonaForm,
) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let errors = match validate_persona(&form, &known) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::catalog::save_persona(&state.pool, actor, id, &input).await {
                Ok(_) => return htmx_redirect("/personas"),
                Err(DbError::NotFound(_)) => return not_found("The persona"),
                Err(e) => {
                    let mut errors = FormErrors::default();
                    if let Some(response) = conflict(&mut errors, "name", e) {
                        return response;
                    }
                    errors
                }
            }
        }
        Err(errors) => errors,
    };
    let action = id.map_or_else(|| "/personas".to_string(), |id| format!("/personas/{id}"));
    invalid(persona_form(
        &action,
        &form,
        &errors,
        &credentials,
        if id.is_some() { "Save" } else { "Create" },
    ))
}

// ---------------------------------------------------------------------------------------------
// Page chrome
// ---------------------------------------------------------------------------------------------

fn page(title: &str, nav: Nav, new_href: &str, new_label: &str, table: Markup) -> Response {
    axum::response::IntoResponse::into_response(layout(
        title,
        nav,
        html! {
            (page_header(title, link_button(ButtonKind::Primary, new_href, new_label)))
            (scroll_box(table))
        },
    ))
}

fn form_page(title: &str, nav: Nav, form: Markup) -> Response {
    axum::response::IntoResponse::into_response(layout(
        title,
        nav,
        html! {
            h1 class="mb-4 text-xl font-semibold" { (title) }
            div class="max-w-xl" { (form) }
        },
    ))
}

#[cfg(test)]
mod tests;
