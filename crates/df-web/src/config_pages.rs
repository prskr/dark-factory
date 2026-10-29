//! Configuration pages: credential references, projects and personas
//! (`specs/configuration-management/spec.md`).

use axum::extract::{Form, Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Extension, Router};
use df_core::{CredentialRef, ForgeKind, StatusSet};
use df_db::DbError;
use df_db::catalog::{
    Actor, CredentialRefInput, PersonaInput, PersonaRecord, ProjectInput, ProjectRecord,
};
use df_db::task_views::ProjectStatusCount;
use maud::{Markup, html};
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::{INPUT_CLASS, field};
use crate::components::{
    ButtonKind, Cell, DialogStyle, FormErrors, HostRole, ICON_BUTTON_CLASS, Icon, Tone,
    action_class, avatar, data_table, dialog_with, empty_panel, forge_icon, form_error,
    harness_label, icon, initials, issue_role, page_header, profile_menu, project_forge_kind,
    readonly_field, role_icon, select_field, submit_button, td, text_field, textarea_field, th, tr,
};
use crate::layout::{Nav, layout};
use crate::responses::{internal_error, invalid, not_found};

/// Harness every persona uses in iteration 1.
const HARNESS: &str = df_core::CLAUDE_CODE_HARNESS;

/// The forms are fragments that open as a modal over their list; a plain browser request for one
/// (no `HX-Request` header) goes to the list instead.
pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route(
            "/credentials",
            get(credentials_list).post(credential_create),
        )
        .route("/credentials/new", get(credential_new))
        .route("/credentials/{id}", axum::routing::post(credential_update))
        .route("/credentials/{id}/edit", get(credential_edit))
        .route("/projects", get(projects_list).post(project_create))
        .route("/projects/new", get(project_new))
        .route("/projects/forge", get(project_forge))
        .route("/projects/{key}", axum::routing::post(project_update))
        .route("/projects/{key}/edit", get(project_edit))
        .route("/personas", get(personas_list).post(persona_create))
        .route("/personas/new", get(persona_new))
        .route("/personas/{id}", axum::routing::post(persona_update))
        .route("/personas/{id}/edit", get(persona_edit))
}

// ---------------------------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct CredentialForm {
    name: String,
    secret_name: String,
    secret_key: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct ProjectForm {
    key: String,
    name: String,
    repo_url: String,
    issue_tracker_url: String,
    image: String,
    base_branch: String,
    adapter_command: String,
    forge_credential_id: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct PersonaForm {
    name: String,
    system_prompt: String,
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
        // Empty means the instance's harness image and adapter.
        adapter_command: form
            .adapter_command
            .split_whitespace()
            .map(str::to_string)
            .collect(),
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
// Shared: modals, saved lists, plain-browser redirects
// ---------------------------------------------------------------------------------------------

/// A form as modal over its list. `form` carries the `#form` the submit swaps.
pub(crate) fn modal(title: &str, form: Markup) -> Response {
    dialog_with("modal", title, DialogStyle::Form, form).into_response()
}

/// Answer of a successful save: the updated list replaces the old one and the modal closes.
pub(crate) fn saved(list: Markup) -> Response {
    let mut response = list.into_response();
    let headers = response.headers_mut();
    headers.insert("HX-Retarget", HeaderValue::from_static("#config-list"));
    headers.insert("HX-Reswap", HeaderValue::from_static("outerHTML"));
    headers.insert("HX-Trigger", HeaderValue::from_static("modal-close"));
    response
}

/// A button that opens a form fragment as modal.
pub(crate) fn open_modal(href: &str, label: &str, class: &str, content: Markup) -> Markup {
    html! {
        button type="button" hx-get=(href) hx-target="#composer-slot" hx-swap="innerHTML" aria-label=(label) title=[(!label.is_empty()).then_some(label)] class=(class) { (content) }
    }
}

/// An edit button of a list entry.
pub(crate) fn edit_button(href: &str, label: &str) -> Markup {
    open_modal(href, label, ICON_BUTTON_CLASS, icon(Icon::Pencil))
}

/// The primary create button of a list page.
pub(crate) fn create_button(href: &str, label: &str) -> Markup {
    open_modal(
        href,
        "",
        action_class(ButtonKind::Primary),
        html! { (icon(Icon::Plus)) (label) },
    )
}

/// A list page around its live region (`#config-list`), at the full width of the content area.
pub(crate) fn list_page(title: &str, nav: Nav, region: Markup) -> Response {
    layout(title, nav, region).into_response()
}

/// Header of a list: title with the number of entries, the create button and, on phones, the
/// profile menu of `user`. Part of the live region, so a save updates the count.
pub(crate) fn list_header(title: &str, count: usize, create: Markup, user: Option<&str>) -> Markup {
    page_header(
        title,
        Some(count),
        html! {
            div class="flex items-center gap-2" {
                (create)
                @if let Some(user) = user { (profile_menu(user)) }
            }
        },
    )
}

/// The rows of a modal form: one column on phones, two from 768 px.
pub(crate) fn form_grid(fields: Markup) -> Markup {
    html! { div class="grid grid-cols-1 gap-4 md:grid-cols-2" { (fields) } }
}

/// A field that takes the whole row of a [`form_grid`].
pub(crate) fn full_row(field: Markup) -> Markup {
    html! { div class="md:col-span-2" { (field) } }
}

/// Bottom right of a modal form: Cancel (closes the dialog) and the primary action.
pub(crate) fn form_footer(submit_label: &str) -> Markup {
    html! {
        div class="mt-5 flex justify-end gap-2 border-t border-line pt-3" {
            button type="button" "x-on:click"="$el.closest('dialog').close()" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
            (submit_button(ButtonKind::Primary, submit_label))
        }
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
            (form_footer(submit))
        }
    }
}

fn credentials_region(refs: &[CredentialRef]) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Credentials", refs.len(), create_button("/credentials/new", "New credential reference"), None))
            @if refs.is_empty() {
                (empty_panel(
                    "No credential references yet",
                    "A credential reference names a Kubernetes Secret key that holds a token, for private repositories or a harness login. The value itself is never entered here.",
                    create_button("/credentials/new", "New credential reference"),
                ))
            } @else {
                (data_table(
                    html! {
                        (th(Cell::First, html! { "Name" })) (th(Cell::Mono, html! { "Secret" })) (th(Cell::Mono, html! { "Key" }))
                        (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                    },
                    html! {
                        @for c in refs {
                            (tr(html! {
                                (td(Cell::First, html! { (c.name) }))
                                (td(Cell::Mono, html! { (c.secret_name) }))
                                (td(Cell::Mono, html! { (c.secret_key) }))
                                (td(Cell::Actions, edit_button(&format!("/credentials/{}/edit", c.id), &format!("Edit credential reference {}", c.name))))
                            }))
                        }
                    },
                ))
            }
        }
    }
}

async fn credentials_markup(state: &WebState) -> Result<Markup, Box<Response>> {
    let refs = df_db::catalog::list_credential_refs(&state.pool)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(credentials_region(&refs))
}

async fn credentials_list(State(state): State<WebState>) -> Response {
    match credentials_markup(&state).await {
        Ok(region) => list_page("Credentials", Nav::Credentials, region),
        Err(response) => *response,
    }
}

async fn credential_new(headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/credentials").into_response();
    }
    modal(
        "New credential reference",
        credential_form(
            "/credentials",
            &CredentialForm::default(),
            &FormErrors::default(),
            "Create credential reference",
        ),
    )
}

async fn credential_edit(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/credentials").into_response();
    }
    match df_db::catalog::get_credential_ref(&state.pool, id).await {
        Ok(Some(c)) => modal(
            "Edit credential reference",
            credential_form(
                &format!("/credentials/{id}"),
                &credential_values(&c),
                &FormErrors::default(),
                "Save credential reference",
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
    let submit = if id.is_some() {
        "Save credential reference"
    } else {
        "Create credential reference"
    };
    let errors = match validate_credential(&form) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::catalog::save_credential_ref(&state.pool, actor, id, &input).await {
                Ok(_) => {
                    return match credentials_markup(&state).await {
                        Ok(region) => saved(region),
                        Err(response) => *response,
                    };
                }
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
        adapter_command: p.adapter_command.join(" "),
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

/// A valid HTTPS repository URL (stricter than the lenient `url_host` of the host icons, which only
/// needs the host): scheme, host and at least an owner and a repository segment.
fn repository_host(url: &str) -> Option<String> {
    let url = Url::parse(url.trim()).ok()?;
    let segments = url.path_segments()?.filter(|s| !s.is_empty()).count();
    (url.scheme() == "https" && segments >= 2)
        .then(|| url.host_str().map(str::to_string))
        .flatten()
}

/// What the forge feedback of the project form shows: an icon per URL field and one hint.
struct ForgeFeedback {
    repo_icon: Markup,
    issue_icon: Markup,
    hint: String,
}

/// The integration the typed URLs map to (design D5): the instance's one integration serves every
/// valid HTTPS repository; an issue tracker on the repository's host uses it too.
fn forge_feedback(forge: Option<&ForgeKind>, repo_url: &str, issue_url: &str) -> ForgeFeedback {
    let host = repository_host(repo_url);
    let served = forge.filter(|_| host.is_some());
    // A typed but unparsable tracker URL still shows the issue icon.
    let issue_icon = if issue_url.trim().is_empty() {
        html! {}
    } else {
        let role =
            issue_role(served, repo_url, issue_url).map_or(HostRole::IssueTracker, |(_, r)| r);
        role_icon(&role)
    };
    let hint = match served {
        Some(kind) => format!(
            "Served by the {} integration; pull requests and issues use it.",
            kind.label()
        ),
        None => "No forge integration applies: pull request and issue features are unavailable."
            .to_string(),
    };
    ForgeFeedback {
        repo_icon: forge_icon(served),
        issue_icon,
        hint,
    }
}

/// A URL field with the forge icon inside it; typing refreshes the icons and the hint.
fn url_field(
    label: &str,
    name: &str,
    icon_id: &str,
    value: &str,
    errors: &FormErrors,
    icon: &Markup,
    hint: Option<&str>,
) -> Markup {
    let mut describedby = vec![];
    if hint.is_some() {
        describedby.push(format!("{name}-hint"));
    }
    if errors.get(name).is_some() {
        describedby.push(format!("{name}-error"));
    }
    field(
        label,
        name,
        errors,
        hint,
        html! {
            div class="relative" {
                span id=(icon_id) class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-muted" { (icon) }
                input id=(name) name=(name) type="text" value=(value) class={(INPUT_CLASS) " pl-10"}
                    hx-get="/projects/forge" hx-trigger="keyup changed delay:300ms"
                    hx-include="[name=repo_url],[name=issue_tracker_url]" hx-swap="none"
                    aria-invalid=[errors.get(name).map(|_| "true")]
                    aria-describedby=[(!describedby.is_empty()).then(|| describedby.join(" "))];
            }
        },
    )
}

fn project_form(
    action: &str,
    values: &ProjectForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    forge: Option<&ForgeKind>,
    submit: &str,
    editing: bool,
) -> Markup {
    let feedback = forge_feedback(forge, &values.repo_url, &values.issue_tracker_url);
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (form_grid(html! {
                (key_field(&values.key, errors, editing))
                (text_field("Name", "name", &values.name, errors, None))
                (full_row(html! {
                    (url_field("Repository URL", "repo_url", "repo-forge", &values.repo_url, errors, &feedback.repo_icon, Some("HTTPS only.")))
                    p id="forge-hint" aria-live="polite" class="mt-1 text-xs text-muted" { (feedback.hint) }
                }))
                (full_row(url_field("Issue tracker URL", "issue_tracker_url", "issue-forge", &values.issue_tracker_url, errors, &feedback.issue_icon, Some("Optional."))))
                (full_row(text_field("Sandbox image", "image", &values.image, errors, None)))
                (text_field("Base branch", "base_branch", &values.base_branch, errors, None))
                (select_field("Forge credential", "forge_credential_id", &credential_options(credentials),
                    &values.forge_credential_id, Some("None"), errors))
                (full_row(text_field("Adapter command (optional)", "adapter_command", &values.adapter_command, errors,
                    Some("Leave empty to use the instance's harness image. Set it when the project image already contains the adapter."))))
            }))
            (form_footer(submit))
        }
    }
}

/// The key: typed when a project is created, shown read-only afterwards (it never changes).
fn key_field(value: &str, errors: &FormErrors, locked: bool) -> Markup {
    if locked {
        readonly_field(
            "Key",
            "key",
            value,
            errors,
            Some("The key cannot change after creation."),
        )
    } else {
        text_field(
            "Key",
            "key",
            value,
            errors,
            Some("For example DF; tasks become DF-1, DF-2."),
        )
    }
}

fn new_project_values() -> ProjectForm {
    ProjectForm {
        base_branch: "main".into(),
        ..ProjectForm::default()
    }
}

/// `GET /projects/forge?repo_url=..&issue_tracker_url=..`: the icons and the hint of the project
/// form for the typed URLs, as htmx partials. The query is read leniently because htmx sends the
/// field it fires from twice.
async fn project_forge(State(state): State<WebState>, RawQuery(query): RawQuery) -> Response {
    let value = |name: &str| {
        url::form_urlencoded::parse(query.as_deref().unwrap_or_default().as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    };
    let feedback = forge_feedback(
        state.forge.as_ref(),
        &value("repo_url"),
        &value("issue_tracker_url"),
    );
    html! {
        template hx type="partial" hx-target="#repo-forge" hx-swap="innerHTML" { (feedback.repo_icon) }
        template hx type="partial" hx-target="#issue-forge" hx-swap="innerHTML" { (feedback.issue_icon) }
        template hx type="partial" hx-target="#forge-hint" hx-swap="innerHTML" { (feedback.hint) }
    }
    .into_response()
}

/// Number of tasks of a project per column of the project list.
struct Counts {
    queued: i64,
    running: i64,
    waiting: i64,
    done: i64,
}

impl Counts {
    /// Folds the grouped counts of one project into the four columns (design D6).
    fn of(project: &ProjectRecord, counts: &[ProjectStatusCount]) -> Self {
        let sum = |set: StatusSet| {
            counts
                .iter()
                .filter(|c| c.project_id == project.id)
                .filter(|c| set.statuses().iter().any(|s| s.as_str() == c.status))
                .map(|c| c.count)
                .sum()
        };
        Self {
            queued: sum(StatusSet::Queued),
            running: sum(StatusSet::Running),
            waiting: sum(StatusSet::Waiting),
            done: sum(StatusSet::Done),
        }
    }

    /// `(label, status filter of the task list, count)` per column.
    fn columns(&self) -> [(&'static str, &'static str, i64); 4] {
        [
            ("Queued", "queued", self.queued),
            ("Running", "running", self.running),
            ("Waiting", "waiting", self.waiting),
            ("Done", "done", self.done),
        ]
    }
}

/// Header and dot color of the four counters.
const COLUMN_TONES: [(&str, Tone); 4] = [
    ("Queued", Tone::Queued),
    ("Running", Tone::Running),
    ("Waiting", Tone::Waiting),
    ("Done", Tone::Completed),
];

/// A count that links to the task list of the project with exactly these statuses; zeros are
/// muted. With a `dot` tone (phone list) it reads "<dot> <number> <state>".
fn count_link(project: &str, status: &str, label: &str, count: i64, dot: Option<Tone>) -> Markup {
    let tone = if count == 0 {
        "text-faint"
    } else {
        "text-accent-text underline"
    };
    html! {
        a href={"/tasks?project=" (project) "&status=" (status)} aria-label={(label) ": " (count) (if count == 1 { " task" } else { " tasks" }) " of " (project)}
            class={"inline-flex min-h-11 min-w-7 items-center justify-end gap-1.5 rounded-md px-2 font-mono text-sm tabular-nums lg:min-h-9 " (tone)} {
            @if let Some(dot) = dot { span aria-hidden="true" class={"size-2 rounded-full " (dot.dot_class())} {} }
            @if dot.is_some() {
                span { (count) }
                span class="font-sans text-[13px]" { (label.to_lowercase()) }
            } @else { (count) }
        }
    }
}

fn projects_region(
    projects: &[ProjectRecord],
    counts: &[ProjectStatusCount],
    forge: Option<&ForgeKind>,
    user: &str,
) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Projects", projects.len(), create_button("/projects/new", "New project"), Some(user)))
            @if projects.is_empty() {
                (empty_panel(
                    "No projects yet",
                    "A project ties a repository and a sandbox image to the tasks that run on it.",
                    html! {
                        (create_button("/projects/new", "New project"))
                        p class="text-sm text-muted" {
                            "Private repositories need a forge token: add it as a "
                            a href="/credentials" class="text-accent-text underline" { "credential reference" } " first."
                        }
                    },
                ))
            } @else {
                div class="hidden min-w-0 md:block" {
                    (data_table(
                        html! {
                            (th(Cell::First, html! { "Project" })) (th(Cell::Mono, html! { "Repository" })) (th(Cell::Mono, html! { "Branch" }))
                            @for (label, tone) in COLUMN_TONES {
                                (th(Cell::Number, html! {
                                    span class="inline-flex items-center gap-1.5" {
                                        span aria-hidden="true" class={"size-2 rounded-full " (tone.dot_class())} {}
                                        (label)
                                    }
                                }))
                            }
                            (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                        },
                        html! {
                            @for p in projects {
                                @let c = Counts::of(p, counts);
                                (tr(html! {
                                    (td(Cell::First, html! {
                                        span class="flex items-center gap-2.5" {
                                            span class="rounded-md bg-sunk px-2 py-0.5 font-mono text-xs font-medium text-muted" { (p.key) }
                                            span class="font-medium" { (p.name) }
                                        }
                                    }))
                                    (td(Cell::Mono, html! {
                                        span class="inline-flex min-w-0 items-center gap-2" {
                                            (forge_icon(project_forge_kind(forge, p)))
                                            span class="truncate" { (p.repo_url) }
                                        }
                                    }))
                                    (td(Cell::Mono, html! { (p.base_branch) }))
                                    @for (label, status, n) in c.columns() { (td(Cell::Number, count_link(&p.key, status, label, n, None))) }
                                    (td(Cell::Actions, edit_button(&format!("/projects/{}/edit", p.key), &format!("Edit project {}", p.key))))
                                }))
                            }
                        },
                    ))
                }
                ul class="divide-y divide-line rounded-xl border border-line bg-panel md:hidden" {
                    @for p in projects {
                        @let c = Counts::of(p, counts);
                        li class="flex items-stretch" {
                            div class="min-w-0 flex-1 space-y-2 py-3.5 pr-1 pl-4" {
                                div class="flex items-center gap-2.5" {
                                    span class="rounded-md bg-sunk px-2 py-0.5 font-mono text-xs font-medium text-muted" { (p.key) }
                                    span class="min-w-0 flex-1 truncate text-base font-medium" { (p.name) }
                                    a href={"/tasks?status=all&project=" (p.key)} aria-label={"Tasks of " (p.name)}
                                        class="inline-flex min-h-11 items-center gap-0.5 text-[13px] font-medium text-accent-text" { "Tasks" (icon(Icon::ChevronRight)) }
                                }
                                div class="flex items-center gap-2 font-mono text-xs text-muted" {
                                    (forge_icon(project_forge_kind(forge, p)))
                                    span class="truncate" { (p.repo_url) }
                                }
                                div class="flex flex-wrap gap-x-3.5" {
                                    @for ((label, status, n), (_, tone)) in c.columns().into_iter().zip(COLUMN_TONES) { (count_link(&p.key, status, label, n, Some(tone))) }
                                }
                            }
                            (open_modal(&format!("/projects/{}/edit", p.key), &format!("Edit project {}", p.key),
                                "flex w-12 shrink-0 items-center justify-center self-stretch text-muted", icon(Icon::Pencil)))
                        }
                    }
                }
            }
        }
    }
}

async fn projects_markup(state: &WebState, user: &str) -> Result<Markup, Box<Response>> {
    let (projects, counts) = tokio::try_join!(
        df_db::catalog::list_projects(&state.pool),
        df_db::task_views::project_status_counts(&state.pool)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(projects_region(
        &projects,
        &counts,
        state.forge.as_ref(),
        user,
    ))
}

async fn projects_list(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match projects_markup(&state, &user.display_name).await {
        Ok(region) => list_page("Projects", Nav::Projects, region),
        Err(response) => *response,
    }
}

async fn project_new(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/projects").into_response();
    }
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    modal(
        "New project",
        project_form(
            "/projects",
            &new_project_values(),
            &FormErrors::default(),
            &credentials,
            state.forge.as_ref(),
            "Create project",
            false,
        ),
    )
}

async fn project_edit(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/projects").into_response();
    }
    let (project, credentials) = match tokio::try_join!(
        df_db::catalog::get_project_by_key(&state.pool, &key),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(project), credentials)) => (project, credentials),
        Ok((None, _)) => return not_found("The project"),
        Err(e) => return internal_error(&e),
    };
    modal(
        "Edit project",
        project_form(
            &format!("/projects/{key}"),
            &project_values(&project),
            &FormErrors::default(),
            &credentials,
            state.forge.as_ref(),
            "Save project",
            true,
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
    // The key is set at creation and never changes (df-db enforces it as well): an edit that
    // carries another key is refused, next to the other errors, and the form shows the stored key.
    let key_changed = existing.as_ref().is_some_and(|e| form.key.trim() != e.key);
    let form = match &existing {
        Some(e) if key_changed => ProjectForm {
            key: e.key.clone(),
            ..form
        },
        _ => form,
    };
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let mut errors = match validate_project(&form, &known) {
        Ok(input) if !key_changed => {
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
                Ok(_) => {
                    return match projects_markup(&state, &user.display_name).await {
                        Ok(region) => saved(region),
                        Err(response) => *response,
                    };
                }
                Err(e) => {
                    let mut errors = FormErrors::default();
                    if let Some(response) = conflict(&mut errors, "key", e) {
                        return response;
                    }
                    errors
                }
            }
        }
        Ok(_) => FormErrors::default(),
        Err(errors) => errors,
    };
    if key_changed {
        errors.add("key", "The key cannot change after creation.");
    }
    let action = existing_key
        .as_ref()
        .map_or_else(|| "/projects".to_string(), |k| format!("/projects/{k}"));
    let submit = if existing.is_some() {
        "Save project"
    } else {
        "Create project"
    };
    invalid(project_form(
        &action,
        &form,
        &errors,
        &credentials,
        state.forge.as_ref(),
        submit,
        existing.is_some(),
    ))
}

// ---------------------------------------------------------------------------------------------
// Personas
// ---------------------------------------------------------------------------------------------

fn persona_values(p: &PersonaRecord) -> PersonaForm {
    PersonaForm {
        name: p.name.clone(),
        system_prompt: p.system_prompt.clone(),
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
            (form_grid(html! {
                (full_row(text_field("Name", "name", &values.name, errors, None)))
                (full_row(textarea_field("System prompt", "system_prompt", &values.system_prompt, 8, errors)))
                div class="space-y-1" {
                    label class="block text-sm font-medium text-muted" for="harness" { "Harness" }
                    select id="harness" name="harness" class=(INPUT_CLASS) {
                        option value=(HARNESS) selected { (HARNESS) }
                    }
                }
                (text_field("Model", "model", &values.model, errors, Some("Optional.")))
                (full_row(select_field("Harness credential", "harness_credential_id", &credential_options(credentials),
                    &values.harness_credential_id, Some("None"), errors)))
            }))
            (form_footer(submit))
        }
    }
}

fn personas_region(personas: &[PersonaRecord], credentials: &[CredentialRef]) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Personas", personas.len(), create_button("/personas/new", "New persona"), None))
            @if personas.is_empty() {
                (empty_panel(
                    "No personas yet",
                    "A persona is the agent a task is assigned to: a system prompt, a harness and a model.",
                    create_button("/personas/new", "New persona"),
                ))
            } @else {
                ul class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3" {
                    @for p in personas {
                        @let credential = p.harness_credential_id
                            .and_then(|id| credentials.iter().find(|c| c.id.as_uuid() == id))
                            .map(|c| c.name.as_str());
                        li class="flex flex-col gap-3 rounded-xl border border-line bg-panel p-4" {
                            div class="flex items-start gap-3" {
                                (avatar(&initials(&p.name)))
                                div class="min-w-0 flex-1" {
                                    h2 class="truncate font-semibold" { (p.name) }
                                    p class="text-xs text-muted" { (harness_label(&p.harness)) " · " (p.model.as_deref().unwrap_or("default")) }
                                }
                                (edit_button(&format!("/personas/{}/edit", p.id), &format!("Edit persona {}", p.name)))
                            }
                            p class="line-clamp-3 whitespace-pre-wrap break-words text-sm text-muted" { (p.system_prompt) }
                            p class="mt-auto text-xs text-faint" {
                                "Credential: " span class="font-mono" { (credential.unwrap_or("none")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn personas_markup(state: &WebState) -> Result<Markup, Box<Response>> {
    let (personas, credentials) = tokio::try_join!(
        df_db::catalog::list_personas(&state.pool),
        df_db::catalog::list_credential_refs(&state.pool)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(personas_region(&personas, &credentials))
}

async fn personas_list(State(state): State<WebState>) -> Response {
    match personas_markup(&state).await {
        Ok(region) => list_page("Personas", Nav::Personas, region),
        Err(response) => *response,
    }
}

async fn persona_new(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/personas").into_response();
    }
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(c) => c,
        Err(e) => return internal_error(&e),
    };
    modal(
        "New persona",
        persona_form(
            "/personas",
            &PersonaForm::default(),
            &FormErrors::default(),
            &credentials,
            "Create persona",
        ),
    )
}

async fn persona_edit(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/personas").into_response();
    }
    let (persona, credentials) = match tokio::try_join!(
        df_db::catalog::get_persona(&state.pool, id),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(persona), credentials)) => (persona, credentials),
        Ok((None, _)) => return not_found("The persona"),
        Err(e) => return internal_error(&e),
    };
    modal(
        "Edit persona",
        persona_form(
            &format!("/personas/{id}"),
            &persona_values(&persona),
            &FormErrors::default(),
            &credentials,
            "Save persona",
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
                Ok(_) => {
                    return match personas_markup(&state).await {
                        Ok(region) => saved(region),
                        Err(response) => *response,
                    };
                }
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
        if id.is_some() {
            "Save persona"
        } else {
            "Create persona"
        },
    ))
}

#[cfg(test)]
mod tests;
