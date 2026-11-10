//! Configuration pages: credential references, projects and personas
//! (`specs/configuration-management/spec.md`).

use axum::extract::{Form, Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Extension, Router};
use df_core::routing::{ForgeRoutes, ForgeServing};
use df_core::webhooks::WebhookWake;
use df_core::{CredentialRef, StatusSet};
use df_db::DbError;
use df_db::catalog::{Actor, CredentialRefInput, ProjectInput, ProjectRecord};
use df_db::push_checks::PushPolicy;
use df_db::task_views::ProjectStatusCount;
use maud::{Markup, html};
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::{INPUT_CLASS, field, form_group, select_field_with_hint};
use crate::components::{
    ButtonKind, Cell, DialogStyle, FormErrors, HostRole, ICON_BUTTON_CLASS, Icon, Tone,
    action_class, data_table, dialog_with, empty_panel, forge_icon, form_error, icon, issue_role,
    page_header, phone_profile_menu, project_forge_kind, readonly_field, role_icon, select_field,
    submit_button, td, text_field, th, tr,
};
use crate::layout::{Nav, Shell, layout};
use crate::responses::{htmx_location, internal_error, invalid, not_found};
use crate::{project_subscriptions, project_webhook};

mod personas;
mod project_page;
mod providers;
mod push_policy;

use push_policy::PushPolicyForm;

/// The forms are fragments that open as a modal over their list; a plain browser request for one
/// (no `HX-Request` header) goes to the list instead.
pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .merge(providers::routes())
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
        .route(
            "/projects/{key}",
            get(project_page::page).post(project_update),
        )
        .merge(personas::routes())
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
    webhook_wake: String,
    /// The tab the settings page showed when it was saved; it stays open afterwards.
    tab: String,
    /// The "Push checks" tab of the edit form.
    #[serde(flatten)]
    push: PushPolicyForm,
}

fn required(errors: &mut FormErrors, field: &'static str, value: &str, label: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        errors.add(field, format!("{label} is required."));
    }
    value.to_string()
}

/// Checks that `value` is a URL with a host and the scheme `https`, or `http` when
/// `network.allowPlainHttp` is on.
fn forge_url(errors: &mut FormErrors, field: &'static str, value: &str, allow_plain_http: bool) {
    match Url::parse(value) {
        Ok(url)
            if (url.scheme() == "https" || (allow_plain_http && url.scheme() == "http"))
                && url.host_str().is_some() => {}
        _ if allow_plain_http => errors.add(
            field,
            "Must be an HTTP or HTTPS URL, for example https://git.example.org/o/r.git.",
        ),
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

fn validate_project(
    form: &ProjectForm,
    credentials: &[Uuid],
    allow_plain_http: bool,
) -> Result<ProjectInput, FormErrors> {
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
    forge_url(&mut errors, "repo_url", &repo_url, allow_plain_http);
    let issue_tracker_url =
        Some(form.issue_tracker_url.trim().to_string()).filter(|u| !u.is_empty());
    if let Some(url) = &issue_tracker_url {
        forge_url(&mut errors, "issue_tracker_url", url, allow_plain_http);
    }
    let image = required(&mut errors, "image", &form.image, "Sandbox image");
    let base_branch = required(&mut errors, "base_branch", &form.base_branch, "Base branch");
    let forge_credential_id = optional_uuid(
        &mut errors,
        "forge_credential_id",
        &form.forge_credential_id,
        credentials,
    );
    let webhook_wake = form.webhook_wake.parse().unwrap_or_else(|_unknown| {
        errors.add("webhook_wake", "Choose who wakes tasks.");
        WebhookWake::default()
    });
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
        webhook_wake,
    })
}

/// Where a create or edit form posts: the collection and, when editing, the entity. The form
/// carries the id in a hidden field so that a refresh can render it again in the same mode.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FormTarget {
    collection: &'static str,
    id: Option<Uuid>,
}

impl FormTarget {
    fn new(collection: &'static str, id: Option<Uuid>) -> Self {
        Self { collection, id }
    }

    /// From the hidden `entity_id` of a posted form: empty (or not an id) means create.
    fn posted(collection: &'static str, entity_id: &str) -> Self {
        Self::new(collection, entity_id.parse().ok())
    }

    fn is_create(self) -> bool {
        self.id.is_none()
    }

    fn action(self) -> String {
        self.id.map_or_else(
            || self.collection.to_string(),
            |id| format!("{}/{id}", self.collection),
        )
    }

    fn id_field(self) -> Markup {
        html! { input type="hidden" name="entity_id" value=(self.id.map(|id| id.to_string()).unwrap_or_default()); }
    }
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

/// Like [`modal`], 896 px wide, for the model provider form (a form with tabs, [`tabbed_form`]).
pub(crate) fn wider_modal(title: &str, form: Markup) -> Response {
    dialog_with("modal", title, DialogStyle::Wider, form).into_response()
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
pub(crate) fn list_page(title: &str, nav: Nav, shell: &Shell, region: Markup) -> Response {
    layout(title, nav, shell, region).into_response()
}

/// Header of a list: title with the number of entries, the create button and, on phones, the
/// profile menu of `shell`. Part of the live region, so a save updates the count.
pub(crate) fn list_header(
    title: &str,
    count: usize,
    create: Markup,
    shell: Option<&Shell>,
) -> Markup {
    page_header(
        title,
        Some(count),
        html! {
            div class="flex items-center gap-2" {
                (create)
                @if let Some(shell) = shell { (phone_profile_menu(shell)) }
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
    footer("mt-5 border-t border-line pt-3", submit_label)
}

fn footer(class: &str, submit_label: &str) -> Markup {
    html! {
        div class={"flex justify-end gap-2 " (class)} {
            button type="button" "x-on:click"="$el.closest('dialog').close()" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
            (submit_button(ButtonKind::Primary, submit_label))
        }
    }
}

/// A tab of a [`tabbed_form`].
pub(crate) struct Tab {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) panel: Markup,
}

/// Where a [`tabbed_form`] sits and how its tabs are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabsStyle {
    /// In a wide modal: the tabs on the left (on top below 768 px) and the footer with Cancel
    /// across the full width.
    Dialog,
    /// On a page: the tabs as a row on top with an underline on the open one, the open tab in the
    /// URL (`?tab=`, see `scripts/tabs.ts`) and posted as `tab`. The submit button of the page's
    /// header (`form="form"`) saves; below 768 px a full-width one ends the form instead.
    Page,
}

/// A form with tabs, one panel per tab; `open` is the id of the tab shown first. The Alpine
/// component `tabs` (`scripts/tabs.ts`) switches them. `style` is explained on [`TabsStyle`].
pub(crate) fn tabbed_form(
    action: &str,
    errors: &FormErrors,
    tabs: &[Tab],
    open: &str,
    submit: &str,
    style: TabsStyle,
) -> Markup {
    let page = style == TabsStyle::Page;
    let (form_class, list_class, tab_class, content_class) = if page {
        (
            "md:rounded-xl md:border md:border-line md:bg-panel",
            "flex shrink-0 gap-1 overflow-x-auto scroll-px-3 border-b border-line md:px-4",
            "group relative flex min-h-11 shrink-0 items-center px-3 text-sm text-muted hover:text-ink aria-selected:font-medium aria-selected:text-ink md:px-4",
            "min-w-0 flex-1 space-y-4 py-5 md:px-6",
        )
    } else {
        (
            "",
            "flex shrink-0 gap-1 overflow-x-auto scroll-px-3 border-b border-line px-3 py-2 md:w-42 md:flex-col md:overflow-visible md:border-r md:border-b-0 md:py-4",
            "group relative flex min-h-11 shrink-0 items-center rounded-lg px-3 text-sm text-muted hover:bg-sunk aria-selected:bg-sunk aria-selected:font-medium aria-selected:text-ink md:min-h-9",
            "min-w-0 flex-1 space-y-4 px-4 py-5 md:px-6",
        )
    };
    let marker_class = if page {
        "absolute inset-x-2 bottom-0 hidden h-0.5 rounded-full bg-accent-text group-aria-selected:block"
    } else {
        "absolute inset-y-2 left-0 hidden w-[3px] rounded-sm bg-accent-text group-aria-selected:block"
    };
    html! {
        form id="form" class=(form_class) hx-post=(action) hx-target="#form" hx-swap="outerHTML"
            x-data={"tabs(" (serde_json::Value::from(open).to_string()) ", " (page) ")"} {
            @if page { input type="hidden" name="tab" value=(open) ":value"="tab"; }
            div class=(if page { "flex flex-col" } else { "flex flex-col md:flex-row" }) {
                div role="tablist" aria-label="Sections" "x-on:keydown"="move($event)" class=(list_class) {
                    @for tab in tabs {
                        @let selected = tab.id == open;
                        button type="button" role="tab" id={"tab-" (tab.id)} data-tab=(tab.id) aria-controls={"panel-" (tab.id)}
                            aria-selected=(selected) tabindex=(if selected { "0" } else { "-1" })
                            ":aria-selected"={"tab === '" (tab.id) "'"} ":tabindex"={"tab === '" (tab.id) "' ? 0 : -1"}
                            "x-on:click"={"tab = '" (tab.id) "'"}
                            class=(tab_class) {
                            span aria-hidden="true" class=(marker_class) {}
                            (tab.label)
                        }
                    }
                }
                div class=(content_class) {
                    (form_error(errors))
                    @for tab in tabs {
                        div role="tabpanel" id={"panel-" (tab.id)} aria-labelledby={"tab-" (tab.id)} class="space-y-6"
                            "x-show"={"tab === '" (tab.id) "'"} x-cloak[tab.id != open] { (tab.panel) }
                    }
                }
            }
            @if page {
                div class="pt-2 pb-5 md:hidden" {
                    button type="submit" class={(action_class(ButtonKind::Primary)) " w-full"} { (submit) }
                }
            } @else {
                (footer("border-t border-line px-4 pt-3 pb-5 md:px-6", submit))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Credential references
// ---------------------------------------------------------------------------------------------

fn credential_values(c: &CredentialRef) -> CredentialForm {
    CredentialForm {
        name: c.name.clone(),
        secret_name: c.secret.name.clone(),
        secret_key: c.secret.key.clone(),
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
                                (td(Cell::Mono, html! { (c.secret.name) }))
                                (td(Cell::Mono, html! { (c.secret.key) }))
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

async fn credentials_list(State(state): State<WebState>, shell: Shell) -> Response {
    match credentials_markup(&state).await {
        Ok(region) => list_page("Credentials", Nav::Credentials, &shell, region),
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

fn project_values(p: &ProjectRecord, push_policy: &PushPolicy) -> ProjectForm {
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
        webhook_wake: p.webhook_wake.as_str().to_string(),
        tab: String::new(),
        push: PushPolicyForm::from_policy(push_policy),
    }
}

fn credential_options(credentials: &[CredentialRef]) -> Vec<(String, String)> {
    credentials
        .iter()
        .map(|c| (c.id.to_string(), c.name.clone()))
        .collect()
}

fn wake_options() -> Vec<(String, String)> {
    WebhookWake::ALL
        .iter()
        .map(|wake| {
            let label = match wake {
                WebhookWake::None => "Nobody",
                WebhookWake::Members => "Members with write access",
                WebhookWake::Anyone => "Anyone",
            };
            (wake.as_str().to_string(), label.to_string())
        })
        .collect()
}

/// What the forge feedback of the project form shows: an icon per URL field and one hint.
struct ForgeFeedback {
    repo_icon: Markup,
    issue_icon: Markup,
    hint: String,
}

/// The integration the typed URLs map to through the routing table (design D7); an issue tracker
/// on the repository's host uses it too.
fn forge_feedback(forges: &ForgeRoutes, repo_url: &str, issue_url: &str) -> ForgeFeedback {
    let serving = forges.forge_for_repo_url(repo_url);
    let served = serving.kind();
    // A typed but unparsable tracker URL still shows the issue icon.
    let issue_icon = if issue_url.trim().is_empty() {
        html! {}
    } else {
        let role = issue_role(served.as_ref(), repo_url, issue_url)
            .map_or(HostRole::IssueTracker, |(_, r)| r);
        role_icon(&role)
    };
    let hint = match (&serving, serving.plugin_kind()) {
        (ForgeServing::Served { .. }, Some(kind)) => format!(
            "Served by the {} integration; pull requests and issues use it.",
            kind.label()
        ),
        (ForgeServing::NotLoaded { default: false, .. }, Some(kind)) => format!(
            "The {} integration claims this host but is not loaded: pull request and issue features are unavailable.",
            kind.label()
        ),
        (ForgeServing::NotLoaded { default: true, .. }, Some(kind)) => format!(
            "The default {} integration is not loaded: pull request and issue features are unavailable.",
            kind.label()
        ),
        (ForgeServing::Unrouted, _) | (_, None) => {
            "No forge integration applies: pull request and issue features are unavailable."
                .to_string()
        }
    };
    ForgeFeedback {
        repo_icon: forge_icon(served.as_ref()),
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

/// Who the project form is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Editing {
    /// A new project: the groups of "General" in a modal, without tabs.
    No,
    /// An existing project, on its settings page: the tabs "General" and "Push checks".
    AsMember,
    /// An existing project, edited by an admin: also the tab "Integrations".
    AsAdmin,
}

impl Editing {
    fn of(existing: bool, user: &CurrentUser) -> Self {
        match (existing, user.has_role(crate::auth::ADMIN_ROLE)) {
            (false, _) => Self::No,
            (true, false) => Self::AsMember,
            (true, true) => Self::AsAdmin,
        }
    }
}

/// The groups Project, Repository, Sandbox and Forge events: the "General" tab and the new-project
/// form.
fn general_groups(
    values: &ProjectForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    state: &WebState,
    editing: Editing,
) -> Markup {
    let feedback = forge_feedback(&state.forges, &values.repo_url, &values.issue_tracker_url);
    let repo_hint = (!state.allow_plain_http).then_some("HTTPS only.");
    html! {
        (form_group("Project", form_grid(html! {
            (key_field(&values.key, errors, editing != Editing::No))
            (text_field("Name", "name", &values.name, errors, None))
        })))
        (form_group("Repository", form_grid(html! {
            (full_row(html! {
                (url_field("Repository URL", "repo_url", "repo-forge", &values.repo_url, errors, &feedback.repo_icon, repo_hint))
                p id="forge-hint" aria-live="polite" class="mt-1 text-xs text-muted" { (feedback.hint) }
            }))
            (text_field("Base branch", "base_branch", &values.base_branch, errors, None))
            (select_field("Forge credential", "forge_credential_id", &credential_options(credentials),
                &values.forge_credential_id, Some("None"), errors))
            (full_row(url_field("Issue tracker URL", "issue_tracker_url", "issue-forge", &values.issue_tracker_url, errors, &feedback.issue_icon, Some("Optional."))))
        })))
        (form_group("Sandbox", form_grid(html! {
            (full_row(text_field("Sandbox image", "image", &values.image, errors, None)))
            (full_row(text_field("Adapter command (optional)", "adapter_command", &values.adapter_command, errors,
                Some("Leave empty to use the instance's harness image. Set it when the project image already contains the adapter."))))
        })))
        (form_group("Forge events", form_grid(html! {
            (full_row(select_field_with_hint("Wake tasks on reviews and mentions", "webhook_wake", &wake_options(),
                &values.webhook_wake, None,
                Some("Reviews and @mentions of the bot account wake the task. Merged or closed PRs and failed checks apply regardless."),
                errors)))
        })))
    }
}

/// What the project form is rendered for.
#[derive(Clone, Copy)]
struct FormContext<'a> {
    editing: Editing,
    /// The stored project; the Integrations tab describes it, not the values typed into the form.
    project: Option<&'a ProjectRecord>,
    /// The tab to open (the URL's or the posted one); a re-rendered form with errors opens the tab
    /// of its first error instead.
    tab: &'a str,
}

/// The project form: untabbed for a new project, the tabs of the settings page otherwise.
fn project_form(
    action: &str,
    values: &ProjectForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    state: &WebState,
    context: FormContext<'_>,
) -> Markup {
    let FormContext {
        editing,
        project,
        tab: requested_tab,
    } = context;
    let general = general_groups(values, errors, credentials, state, editing);
    if editing == Editing::No {
        return html! {
            form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
                (form_error(errors))
                div class="space-y-6" { (general) }
                (form_footer("Create project"))
            }
        };
    }
    let mut tabs = vec![
        Tab {
            id: "general",
            label: "General",
            panel: general,
        },
        Tab {
            id: "push-checks",
            label: "Push checks",
            panel: push_policy::panel(&values.push, errors),
        },
    ];
    if let (Editing::AsAdmin, Some(project)) = (editing, project) {
        tabs.push(Tab {
            id: "integrations",
            label: "Integrations",
            panel: html! {
                (project_subscriptions::lifecycle_group(&values.key))
                (project_webhook::group(state, project))
            },
        });
    }
    let open = if errors.is_empty() {
        tabs.iter()
            .find(|tab| tab.id == requested_tab)
            .map_or("general", |tab| tab.id)
    } else if errors.fields().all(|f| push_policy::FIELDS.contains(&f)) {
        "push-checks"
    } else {
        "general"
    };
    tabbed_form(action, errors, &tabs, open, "Save project", TabsStyle::Page)
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
        webhook_wake: WebhookWake::default().as_str().to_string(),
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
        &state.forges,
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

/// The edit action of a project in the list: a link to its settings page.
fn project_link(key: &str, class: &str) -> Markup {
    html! {
        a href={"/projects/" (key)} aria-label={"Edit project " (key)} title={"Edit project " (key)} class=(class) {
            (icon(Icon::Pencil))
        }
    }
}

fn projects_region(
    projects: &[ProjectRecord],
    counts: &[ProjectStatusCount],
    forges: &ForgeRoutes,
    shell: &Shell,
) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Projects", projects.len(), create_button("/projects/new", "New project"), Some(shell)))
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
                                            a href={"/projects/" (p.key)} class="font-medium hover:underline" { (p.name) }
                                        }
                                    }))
                                    (td(Cell::Mono, html! {
                                        span class="inline-flex min-w-0 items-center gap-2" {
                                            (forge_icon(project_forge_kind(forges, p).as_ref()))
                                            span class="truncate" { (p.repo_url) }
                                        }
                                    }))
                                    (td(Cell::Mono, html! { (p.base_branch) }))
                                    @for (label, status, n) in c.columns() { (td(Cell::Number, count_link(&p.key, status, label, n, None))) }
                                    (td(Cell::Actions, html! {
                                        span class="flex justify-end" {
                                            (project_link(&p.key, ICON_BUTTON_CLASS))
                                        }
                                    }))
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
                                    a href={"/projects/" (p.key)} class="min-w-0 flex-1 truncate text-base font-medium" { (p.name) }
                                    a href={"/tasks?status=all&project=" (p.key)} aria-label={"Tasks of " (p.name)}
                                        class="inline-flex min-h-11 items-center gap-0.5 text-[13px] font-medium text-accent-text" { "Tasks" (icon(Icon::ChevronRight)) }
                                }
                                div class="flex items-center gap-2 font-mono text-xs text-muted" {
                                    (forge_icon(project_forge_kind(forges, p).as_ref()))
                                    span class="truncate" { (p.repo_url) }
                                }
                                div class="flex flex-wrap gap-x-3.5" {
                                    @for ((label, status, n), (_, tone)) in c.columns().into_iter().zip(COLUMN_TONES) { (count_link(&p.key, status, label, n, Some(tone))) }
                                }
                            }
                            (project_link(&p.key, "flex w-12 shrink-0 items-center justify-center self-stretch text-muted"))
                        }
                    }
                }
            }
        }
    }
}

async fn projects_markup(state: &WebState, user: &CurrentUser) -> Result<Markup, Box<Response>> {
    let (projects, counts) = tokio::try_join!(
        df_db::catalog::list_projects(&state.pool),
        df_db::task_views::project_status_counts(&state.pool)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(projects_region(
        &projects,
        &counts,
        &state.forges,
        &Shell::new(user, state),
    ))
}

async fn projects_list(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match projects_markup(&state, &user).await {
        Ok(region) => list_page(
            "Projects",
            Nav::Projects,
            &Shell::new(&user, &state),
            region,
        ),
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
            &state,
            FormContext {
                editing: Editing::No,
                project: None,
                tab: "",
            },
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
    let mut form = match &existing {
        Some(e) if key_changed => ProjectForm {
            key: e.key.clone(),
            ..form
        },
        _ => form,
    };
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let mut errors = FormErrors::default();
    let input = validate_project(&form, &known, state.allow_plain_http)
        .map_err(|e| errors.extend(e))
        .ok();
    let push_policy = match &existing {
        Some(project) => match push_policy::validate(&form.push) {
            Ok(Some(policy)) => Some(policy),
            // Without the tab's fields the stored policy stays; a re-rendered form shows it.
            Ok(None) => match df_db::push_checks::push_policy(&state.pool, project.id).await {
                Ok(stored) => {
                    form.push = PushPolicyForm::from_policy(&stored);
                    None
                }
                Err(e) => return internal_error(&e),
            },
            Err(e) => {
                errors.extend(e);
                None
            }
        },
        None => None,
    };
    if key_changed {
        errors.add("key", "The key cannot change after creation.");
    }
    if let Some(input) = input.filter(|_| errors.is_empty()) {
        let principal = user.principal();
        let actor = Actor {
            principal: &principal,
            roles: &user.roles,
        };
        let stored = if let (Some(project), Some(policy)) = (&existing, &push_policy) {
            df_db::catalog::update_project_with_push_policy(
                &state.pool,
                actor,
                project.id,
                &input,
                policy,
            )
            .await
        } else {
            let id = existing.as_ref().map(|p| p.id);
            df_db::catalog::save_project(&state.pool, actor, id, &input).await
        };
        match stored {
            Ok(record) if existing.is_some() => {
                return project_page::saved(&state, &user, &record, &credentials, &form.tab).await;
            }
            Ok(record) => return htmx_location(&format!("/projects/{}", record.key)),
            Err(e) => {
                if let Some(response) = conflict(&mut errors, "key", e) {
                    return response;
                }
            }
        }
    }
    let action = existing_key
        .as_ref()
        .map_or_else(|| "/projects".to_string(), |k| format!("/projects/{k}"));
    invalid(project_form(
        &action,
        &form,
        &errors,
        &credentials,
        &state,
        FormContext {
            editing: Editing::of(existing.is_some(), &user),
            project: existing.as_ref(),
            tab: &form.tab,
        },
    ))
}

#[cfg(test)]
mod tests;
