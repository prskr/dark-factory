//! The dialogs that add and edit rules and the handlers behind them. One dialog serves both: a new
//! rule posts to `/egress/allowlist/host` or `/service`, a stored one to `/egress/allowlist/{id}`
//! (egress-rule-editing design D3). The answer renders the region the dialog was opened from
//! ([`Origin`], design D4).

use axum::Extension;
use axum::extract::{Form, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use df_core::host::validate_allowlist_host;
use df_db::catalog::Actor;
use df_db::{DbError, EntryKind, EntryRecord, HostScope};
use maud::{Markup, html};
use uuid::Uuid;

use super::origin::{Origin, ReturnTo};
use super::{
    ALLOWLIST, ANY_PORT, Effect, RuleForm, region_markup, validate_k8s_name, validate_port,
    validate_ports,
};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::INPUT_CLASS;
use crate::components::{
    ButtonKind, FormErrors, action_class, field, form_error, select_field, text_field,
};
use crate::config_pages::{form_footer, form_grid, full_row, modal, saved_into};
use crate::egress::project_tab;
use crate::responses::{internal_error, invalid, not_found};

/// The id of the form of a dialog and its selector. It is not `form`: on the project settings page
/// the dialog mounts after the project form, which has that id, and a rejected save must swap the
/// dialog's form.
macro_rules! dialog_form_id {
    () => {
        "rule-form"
    };
}
const DIALOG_FORM: &str = dialog_form_id!();
const DIALOG_FORM_TARGET: &str = concat!("#", dialog_form_id!());

// ---------------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------------

/// `GET /egress/allowlist/host/new`: the dialog of a new `http-host` rule or deny rule.
pub(in crate::egress) async fn new_host(
    state: State<WebState>,
    headers: HeaderMap,
    origin: Query<Origin>,
) -> Response {
    new_dialog(&state, &headers, origin.0, NewKind::Host).await
}

/// `GET /egress/allowlist/service/new`: the dialog of a new cluster service.
pub(in crate::egress) async fn new_service(
    state: State<WebState>,
    headers: HeaderMap,
    origin: Query<Origin>,
) -> Response {
    new_dialog(&state, &headers, origin.0, NewKind::ClusterService).await
}

/// `GET /egress/allowlist/{id}/edit`: the dialog of the stored rule.
pub(in crate::egress) async fn edit(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(origin): Query<Origin>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return plain_redirect(&origin);
    }
    let entry = match df_db::get_entry(&state.pool, id).await {
        Ok(Some(entry)) => entry,
        Ok(None) => return not_found("The rule"),
        Err(e) => return internal_error(&e),
    };
    match project_options(&state).await {
        Ok(options) => {
            let (title, form) = dialog_of(
                &entry,
                &options,
                &values_of(&entry, origin),
                &FormErrors::default(),
            );
            modal(title, form)
        }
        Err(response) => *response,
    }
}

/// `POST /egress/allowlist/host`: adds an `http-host` rule or a deny rule.
pub(in crate::egress) async fn add_host(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RuleForm>,
) -> Response {
    let mut errors = FormErrors::default();
    let kind = if form.is_deny() {
        deny_kind(&form, &mut errors)
    } else {
        allow_kind(&form, &mut errors)
    };
    if let Some(kind) = kind
        && let Some(response) = create(&state, &user, &form, &kind, &mut errors).await
    {
        return response;
    }
    show_again(&state, errors, |options, errors| {
        host_form(options, &form, errors, &Mode::New)
    })
    .await
}

/// `POST /egress/allowlist/service`: adds a cluster service.
pub(in crate::egress) async fn add_service(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RuleForm>,
) -> Response {
    let mut errors = FormErrors::default();
    if let Some(kind) = service_kind(&state, &form, &mut errors).await
        && let Some(response) = create(&state, &user, &form, &kind, &mut errors).await
    {
        return response;
    }
    show_again(&state, errors, |options, errors| {
        service_form(options, &form, errors, &Mode::New)
    })
    .await
}

/// `POST /egress/allowlist/{id}`: saves the edited rule. The stored rule decides the kind; the
/// posted effect is ignored.
pub(in crate::egress) async fn update(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    Form(form): Form<RuleForm>,
) -> Response {
    let stored = match df_db::get_entry(&state.pool, id).await {
        Ok(stored) => stored,
        Err(e) => return internal_error(&e),
    };
    let mut errors = FormErrors::default();
    let Some(stored) = stored else {
        return gone(&state, id, form, errors).await;
    };
    let effect = match stored.kind {
        EntryKind::HttpDeny { .. } => Effect::Deny,
        EntryKind::HttpHost { .. } | EntryKind::ClusterService { .. } => Effect::Allow,
    };
    let form = RuleForm { effect, ..form };
    let scope = match resolve_scope(&state, &form.project, &stored, &mut errors).await {
        Ok(scope) => scope,
        Err(response) => return *response,
    };
    let kind = match stored.kind {
        EntryKind::HttpHost { .. } => allow_kind(&form, &mut errors),
        EntryKind::HttpDeny { .. } => deny_kind(&form, &mut errors),
        EntryKind::ClusterService { .. } => service_kind(&state, &form, &mut errors).await,
    };
    if let (Some(scope), Some(kind)) = (scope, kind) {
        let principal = user.principal();
        let actor = Actor {
            principal: &principal,
            roles: &user.roles,
        };
        match df_db::update_entry(&state.pool, actor, id, scope, &kind).await {
            Ok(_) => return respond(&state, form.origin.resolve(), Answer::Saved).await,
            Err(DbError::Conflict(message)) => errors.add("", message),
            Err(DbError::NotFound(_)) => errors.add("", "This rule no longer exists."),
            // The rule left its task scope between the read above and the update.
            Err(DbError::InvalidData(_)) => errors.add(
                "",
                "This rule was changed meanwhile. Close the dialog and edit it again.",
            ),
            Err(e) => return internal_error(&e),
        }
    }
    show_again(&state, errors, |options, errors| {
        dialog_of(&stored, options, &form, errors).1
    })
    .await
}

/// `DELETE /egress/allowlist/{id}`: deletes an entry and renders the region it was deleted from
/// again (a page that no longer exists shows the last one).
pub(in crate::egress) async fn remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    Query(origin): Query<Origin>,
) -> Response {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::delete_entry(&state.pool, actor, id).await {
        Ok(_) => respond(&state, origin.resolve(), Answer::Refreshed).await,
        Err(DbError::NotFound(_)) => not_found("The entry"),
        Err(e) => internal_error(&e),
    }
}

/// What a request that changed rules answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::egress) enum Answer {
    /// A dialog saved: the region is retargeted and the dialog closes.
    Saved,
    /// An action in the region (delete, "Make global"): the region replaces itself.
    Refreshed,
}

/// The answer of a dialog or an action: the region `back` names, replacing its old state.
pub(in crate::egress) async fn respond(
    state: &WebState,
    back: ReturnTo,
    answer: Answer,
) -> Response {
    let (region, target) = match back {
        ReturnTo::Allowlist(view) => (region_markup(state, &view).await, "#config-list"),
        ReturnTo::Project(key) => (
            project_tab::region_markup(state, &key).await,
            "#project-egress",
        ),
    };
    match region {
        Ok(region) if answer == Answer::Saved => saved_into(target, region),
        Ok(region) => region.into_response(),
        Err(response) => *response,
    }
}

// ---------------------------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------------------------

/// Whether a dialog adds a rule or edits a stored one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    New,
    /// The rule `id`; `task` is the key of its task for a task rule, whose "Applies to" then offers
    /// that task first.
    Edit {
        id: Uuid,
        task: Option<String>,
    },
}

impl Mode {
    fn action(&self, new: &str) -> String {
        match self {
            Self::New => new.to_string(),
            Self::Edit { id, .. } => format!("{ALLOWLIST}/{id}"),
        }
    }

    const fn is_edit(&self) -> bool {
        matches!(self, Self::Edit { .. })
    }
}

/// `(key, "Name (KEY)")` of every project, for the scope select.
async fn project_options(state: &WebState) -> Result<Vec<(String, String)>, Box<Response>> {
    let projects = df_db::catalog::list_projects(&state.pool)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(projects
        .into_iter()
        .map(|p| (p.key.clone(), format!("{} ({})", p.name, p.key)))
        .collect())
}

fn scope_field(options: &[(String, String)], selected: &str, errors: &FormErrors) -> Markup {
    select_field(
        "Applies to",
        "project",
        options,
        selected,
        Some("Every project (instance)"),
        errors,
    )
}

/// The dialog of an `http-host` rule or a deny rule. `effect` (Allow or Deny) is a radio group; the
/// Alpine component `egressRule` (`scripts/egress-rule.ts`) disables "Applies to" and Port for Deny
/// and shows `every project` and `any` in them. The server renders the same state, so a form that
/// is shown again after an error keeps it. Disabled fields are not submitted. When editing, the
/// effect is fixed: the radios are disabled and a hidden field carries the value.
fn host_form(
    options: &[(String, String)],
    values: &RuleForm,
    errors: &FormErrors,
    mode: &Mode,
) -> Markup {
    let deny = values.is_deny();
    let effect = values.effect;
    let edit = mode.is_edit();
    let radio = |value: Effect, label: &'static str, tone: &'static str| {
        html! {
            label class={"flex flex-1 items-center justify-center rounded-md px-3 text-sm font-medium text-muted has-focus-visible:outline-2 has-checked:border has-checked:" (tone) " " (if edit { "cursor-not-allowed" } else { "cursor-pointer" })} {
                input type="radio" name="effect" value=(value.as_str()) checked[effect == value] disabled[edit] x-model="effect" class="sr-only";
                (label)
            }
        }
    };
    let submit = match (edit, deny) {
        (true, _) => "Save rule",
        (false, true) => "Add deny rule",
        (false, false) => "Add rule",
    };
    let task_value = match mode {
        Mode::Edit {
            task: Some(task), ..
        } => Some(format!("task:{task}")),
        _ => None,
    };
    html! {
        form id=(DIALOG_FORM) class="space-y-4" hx-post=(mode.action("/egress/allowlist/host")) hx-target=(DIALOG_FORM_TARGET) hx-swap="outerHTML"
            x-data={"egressRule(" (serde_json::json!({ "effect": effect.as_str() })) ")"} {
            (form_error(errors))
            (values.origin.hidden_fields())
            div class="flex flex-col gap-4 md:flex-row" {
                div class="shrink-0" { (field("Effect", "effect", errors, None, html! {
                    div role="radiogroup" aria-label="Effect" class="flex min-h-11 items-stretch gap-1 rounded-lg border border-line bg-sunk p-1 lg:min-h-10" {
                        (radio(Effect::Allow, "Allow", "border-status-completed-fg/40 has-checked:bg-status-completed-bg has-checked:text-status-completed-fg"))
                        (radio(Effect::Deny, "Deny", "border-status-failed-fg/40 has-checked:bg-status-failed-bg has-checked:text-status-failed-fg"))
                    }
                    @if edit { input type="hidden" name="effect" value=(effect.as_str()); }
                })) }
                div class="min-w-0 flex-1" { (field("Applies to", "project", errors, None, html! {
                    select id="project" name="project" class=(INPUT_CLASS) x-ref="project" ":disabled"="deny" disabled[deny] {
                        @if let Some(task_value) = &task_value {
                            option value=(task_value) selected[!deny && *task_value == values.project] {
                                "Task " (task_value.trim_start_matches("task:"))
                            }
                        }
                        option value="" selected[values.project.is_empty() || deny] { "Every project (instance)" }
                        @for (value, text) in options {
                            option value=(value) selected[!deny && *value == values.project] { (text) }
                        }
                    }
                })) }
            }
            div class="flex flex-col gap-4 md:flex-row" {
                div class="min-w-0 flex-1" { (field("Host", "host", errors, None, html! {
                    input id="host" name="host" type="text" value=(values.host) class=(INPUT_CLASS)
                        aria-invalid=[errors.get("host").map(|_| "true")] aria-describedby="host-hint";
                    p id="host-hint" class="text-xs text-faint" {
                        span x-show="!deny" style=[deny.then_some("display: none")] { "A hostname such as api.anthropic.com, or *.example.org for all its subdomains." }
                        span x-show="deny" style=[(!deny).then_some("display: none")] { "Every port and project. No requests are raised for matching hosts." }
                    }
                })) }
                div class="md:w-32 md:shrink-0" { (field("Port", "port", errors, None, html! {
                    input id="port" name="port" type="text" x-ref="port" ":disabled"="deny" disabled[deny]
                        value=(if deny { ANY_PORT } else { values.port.as_str() }) class=(INPUT_CLASS)
                        aria-invalid=[errors.get("port").map(|_| "true")];
                    p class="text-xs text-faint" x-show="!deny" style=[deny.then_some("display: none")] { "Empty means 443. Plain HTTP needs an explicit 80." }
                })) }
            }
            div class="mt-5 flex justify-end gap-2 border-t border-line pt-3" {
                button type="button" "x-on:click"="$el.closest('dialog').close()" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                button type="submit" class=(action_class(ButtonKind::Primary))
                    "x-text"=[(!edit).then_some("deny ? 'Add deny rule' : 'Add rule'")] { (submit) }
            }
        }
    }
}

/// The dialog of a cluster service.
fn service_form(
    options: &[(String, String)],
    values: &RuleForm,
    errors: &FormErrors,
    mode: &Mode,
) -> Markup {
    let submit = if mode.is_edit() {
        "Save rule"
    } else {
        "Add cluster service"
    };
    html! {
        form id=(DIALOG_FORM) class="space-y-4" hx-post=(mode.action("/egress/allowlist/service")) hx-target=(DIALOG_FORM_TARGET) hx-swap="outerHTML" {
            (form_error(errors))
            (values.origin.hidden_fields())
            (form_grid(html! {
                (full_row(scope_field(options, &values.project, errors)))
                (text_field("Namespace", "namespace", &values.namespace, errors, None))
                (text_field("Service", "service", &values.service, errors, None))
                (full_row(text_field("Ports", "ports", &values.ports, errors,
                    Some("Comma separated target ports. Empty means every port of the Service."))))
                (full_row(html! {
                    label class="flex min-h-11 items-center gap-2 text-sm" {
                        input type="checkbox" name="allow_dns" checked[values.allow_dns.is_some()];
                        "Allow the cluster DNS (for headless Services)"
                    }
                }))
            }))
            (form_footer(submit))
        }
    }
}

/// The values of the stored rule `entry`, for the edit dialog.
fn values_of(entry: &EntryRecord, origin: Origin) -> RuleForm {
    let project = match (&entry.task_key, &entry.project_key) {
        (Some(task), _) => format!("task:{task}"),
        (None, Some(project)) => project.clone(),
        (None, None) => String::new(),
    };
    let form = RuleForm {
        project,
        origin,
        ..RuleForm::default()
    };
    match &entry.kind {
        EntryKind::HttpHost { host, port } => RuleForm {
            host: host.clone(),
            port: port.to_string(),
            ..form
        },
        EntryKind::HttpDeny { host } => RuleForm {
            effect: Effect::Deny,
            host: host.clone(),
            ..form
        },
        EntryKind::ClusterService {
            namespace,
            service,
            ports,
            allow_dns,
        } => RuleForm {
            namespace: namespace.clone(),
            service: service.clone(),
            ports: ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            allow_dns: allow_dns.then(|| "on".to_string()),
            ..form
        },
    }
}

fn mode_of(entry: &EntryRecord) -> Mode {
    Mode::Edit {
        id: entry.id,
        task: entry.task_key.clone(),
    }
}

/// The dialog of `entry` (host and deny rules use one form, cluster services their own) with its
/// title.
fn dialog_of(
    entry: &EntryRecord,
    options: &[(String, String)],
    values: &RuleForm,
    errors: &FormErrors,
) -> (&'static str, Markup) {
    let mode = mode_of(entry);
    match entry.kind {
        EntryKind::ClusterService { .. } => (
            "Edit cluster service",
            service_form(options, values, errors, &mode),
        ),
        EntryKind::HttpHost { .. } | EntryKind::HttpDeny { .. } => (
            "Edit egress rule",
            host_form(options, values, errors, &mode),
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers of the handlers
// ---------------------------------------------------------------------------------------------

/// The redirect of a plain browser request for a dialog: the list it belongs to.
fn plain_redirect(origin: &Origin) -> Response {
    match origin.project_key() {
        Some(key) => Redirect::to(&format!("/projects/{key}?tab=egress")).into_response(),
        None => Redirect::to(ALLOWLIST).into_response(),
    }
}

/// The kind of rule a new-rule dialog adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewKind {
    /// A host or deny rule.
    Host,
    ClusterService,
}

/// The dialog of a new rule of `kind`; "Applies to" starts at the project whose tab opened it.
async fn new_dialog(
    state: &WebState,
    headers: &HeaderMap,
    origin: Origin,
    kind: NewKind,
) -> Response {
    if !crate::auth::is_htmx(headers) {
        return plain_redirect(&origin);
    }
    let values = RuleForm {
        project: origin.project_key().unwrap_or_default().to_string(),
        origin,
        ..RuleForm::default()
    };
    let errors = FormErrors::default();
    match project_options(state).await {
        Ok(options) if kind == NewKind::ClusterService => modal(
            "Add cluster service",
            service_form(&options, &values, &errors, &Mode::New),
        ),
        Ok(options) => modal(
            "New egress rule",
            host_form(&options, &values, &errors, &Mode::New),
        ),
        Err(response) => *response,
    }
}

fn allow_kind(form: &RuleForm, errors: &mut FormErrors) -> Option<EntryKind> {
    let host = validate_allowlist_host(&form.host).map_err(|m| errors.add("host", m));
    let port = validate_port(&form.port).map_err(|m| errors.add("port", m));
    match (host, port) {
        (Ok(host), Ok(port)) => Some(EntryKind::HttpHost { host, port }),
        _ => None,
    }
}

/// A deny rule has no project and no port: both fields are ignored.
fn deny_kind(form: &RuleForm, errors: &mut FormErrors) -> Option<EntryKind> {
    validate_allowlist_host(&form.host)
        .map_err(|m| errors.add("host", m))
        .ok()
        .map(|host| EntryKind::HttpDeny { host })
}

async fn service_kind(
    state: &WebState,
    form: &RuleForm,
    errors: &mut FormErrors,
) -> Option<EntryKind> {
    let namespace =
        validate_k8s_name(&form.namespace, "Namespace").map_err(|m| errors.add("namespace", m));
    let service = validate_k8s_name(&form.service, "Service").map_err(|m| errors.add("service", m));
    let ports = validate_ports(&form.ports).map_err(|m| errors.add("ports", m));
    if let (Ok(namespace), Ok(service), Ok(ports)) = (namespace, service, ports)
        && service_exists(state, &namespace, &service, errors).await
    {
        return Some(EntryKind::ClusterService {
            namespace,
            service,
            ports,
            allow_dns: form.allow_dns.is_some(),
        });
    }
    None
}

/// Stores the new rule `kind`. `Some` is the final response (the updated region, or an error
/// page); `None` means the database refused the rule and `errors` says why.
async fn create(
    state: &WebState,
    user: &CurrentUser,
    form: &RuleForm,
    kind: &EntryKind,
    errors: &mut FormErrors,
) -> Option<Response> {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let result = match kind {
        EntryKind::HttpDeny { host } => {
            df_db::egress::add_deny_rule(&state.pool, host, actor).await
        }
        EntryKind::HttpHost { .. } | EntryKind::ClusterService { .. } => {
            let project = (!form.project.is_empty()).then_some(form.project.as_str());
            df_db::add_entry(&state.pool, actor, project, kind).await
        }
    };
    match result {
        Ok(_) => Some(respond(state, form.origin.resolve(), Answer::Saved).await),
        Err(DbError::Conflict(message)) => {
            errors.add("", message);
            None
        }
        Err(DbError::NotFound(_)) => {
            errors.add("project", "Unknown project.");
            None
        }
        Err(e) => Some(internal_error(&e)),
    }
}

/// The scope that "Applies to" names for the stored rule `stored`: the instance, a project, or
/// the rule's own task (never another one). `None` means `errors` says why not.
async fn resolve_scope(
    state: &WebState,
    value: &str,
    stored: &EntryRecord,
    errors: &mut FormErrors,
) -> Result<Option<HostScope>, Box<Response>> {
    // A deny rule applies to the instance; its field is disabled.
    if value.is_empty() || matches!(stored.kind, EntryKind::HttpDeny { .. }) {
        return Ok(Some(HostScope::Instance));
    }
    if let Some(task) = value.strip_prefix("task:") {
        return Ok(match (stored.task_key.as_deref(), stored.task_id) {
            (Some(key), Some(id)) if key == task => Some(HostScope::Task(id)),
            _ => {
                errors.add("project", "Unknown scope.");
                None
            }
        });
    }
    match df_db::catalog::get_project_by_key(&state.pool, value).await {
        Ok(Some(project)) => Ok(Some(HostScope::Project(project.id))),
        Ok(None) => {
            errors.add("project", "Unknown project.");
            Ok(None)
        }
        Err(e) => Err(Box::new(internal_error(&e))),
    }
}

/// The dialog of a rule that was deleted while it was open: it says so and creates nothing.
async fn gone(state: &WebState, id: Uuid, form: RuleForm, mut errors: FormErrors) -> Response {
    errors.add("", "This rule no longer exists.");
    let mode = Mode::Edit { id, task: None };
    show_again(state, errors, |options, errors| {
        if form.namespace.is_empty() && form.service.is_empty() {
            host_form(options, &form, errors, &mode)
        } else {
            service_form(options, &form, errors, &mode)
        }
    })
    .await
}

/// Whether the Service exists; adds the reason to `errors` if not.
async fn service_exists(
    state: &WebState,
    namespace: &str,
    service: &str,
    errors: &mut FormErrors,
) -> bool {
    let Some(lookup) = &state.services else {
        errors.add(
            "",
            "Services cannot be checked: the Kubernetes API is not available.",
        );
        return false;
    };
    match lookup.exists(namespace, service).await {
        Ok(true) => true,
        Ok(false) => {
            errors.add(
                "service",
                format!("The Service {namespace}/{service} does not exist."),
            );
            false
        }
        Err(error) => {
            tracing::warn!(%error, namespace, service, "Service lookup failed");
            errors.add("", format!("The Service could not be checked: {error}"));
            false
        }
    }
}

async fn show_again(
    state: &WebState,
    errors: FormErrors,
    form: impl FnOnce(&[(String, String)], &FormErrors) -> Markup,
) -> Response {
    match project_options(state).await {
        Ok(options) => invalid(form(&options, &errors)),
        Err(response) => *response,
    }
}
