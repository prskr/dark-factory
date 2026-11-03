//! `/egress/allowlist`: instance, project and task entries of kind `http-host` and
//! `cluster-service` (task entries are only `http-host`) and deny rules (`http-deny`, instance
//! only), with search and pages.

use axum::Extension;
use axum::extract::{Form, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use df_core::host::{DEFAULT_HOST_PORT, is_dns_label, validate_allowlist_host};
use df_db::catalog::Actor;
use df_db::{DbError, EntryKind, EntryRecord};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::INPUT_CLASS;
use crate::components::{
    ButtonKind, Cell, FormErrors, HostIconSize, ICON_BUTTON_CLASS, Icon, ProjectHosts, Tone,
    action_class, data_table, empty_panel, field, form_error, host_icon, icon, link_button,
    select_field, td, text_field, th, tone_badge,
};
use crate::config_pages::{
    create_button, form_footer, form_grid, full_row, list_page, modal, open_modal, saved,
};
use crate::layout::Nav;
use crate::responses::{internal_error, invalid, not_found};

pub(super) mod paging;

use paging::View;

const ALLOWLIST: &str = "/egress/allowlist";

/// The shown port of a deny rule, which applies to every port.
const ANY_PORT: &str = "any";

/// What a rule does: allow the host, or deny it (a silencer for requests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Effect {
    #[default]
    Allow,
    Deny,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct HostForm {
    effect: Effect,
    project: String,
    host: String,
    port: String,
}

impl Effect {
    /// The wire name: the value of the radio button, as `serde` reads it back.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

impl HostForm {
    fn is_deny(&self) -> bool {
        self.effect == Effect::Deny
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct ServiceForm {
    project: String,
    namespace: String,
    service: String,
    ports: String,
    allow_dns: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------------------------

fn validate_port(raw: &str) -> Result<u16, &'static str> {
    if raw.trim().is_empty() {
        return Ok(DEFAULT_HOST_PORT);
    }
    validate_required_port(raw)
}

/// A port between 1 and 65535 that must be given.
pub(super) fn validate_required_port(raw: &str) -> Result<u16, &'static str> {
    raw.trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Must be a port between 1 and 65535.")
}

/// A Kubernetes namespace or Service name (DNS-1123 label).
fn validate_k8s_name(raw: &str, what: &'static str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(format!("{what} is required."));
    }
    if is_dns_label(name) {
        Ok(name.to_string())
    } else {
        Err(format!("{what} must be a lower case DNS label."))
    }
}

/// Comma or space separated ports; none means every port of the Service.
fn validate_ports(raw: &str) -> Result<Vec<u16>, &'static str> {
    raw.split([',', ' '])
        .filter(|port| !port.is_empty())
        .map(|port| port.parse::<u16>().ok().filter(|p| *p != 0))
        .collect::<Option<Vec<_>>>()
        .ok_or("Ports are numbers between 1 and 65535, separated by commas.")
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

/// The scope of an entry by its keys, as the allowlist and the allow dialog name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScopeName<'a> {
    Task(&'a str),
    Project(&'a str),
    Instance,
}

impl<'a> ScopeName<'a> {
    /// The scope of an entry with these keys; a task entry also names its project.
    pub(super) fn of(project: Option<&'a str>, task: Option<&'a str>) -> Self {
        match (project, task) {
            (_, Some(task)) => Self::Task(task),
            (Some(project), None) => Self::Project(project),
            (None, None) => Self::Instance,
        }
    }

    /// `Task DF-7`, `Project DF`, `Everywhere`.
    pub(super) fn label(self) -> String {
        match self {
            Self::Task(task) => format!("Task {task}"),
            Self::Project(project) => format!("Project {project}"),
            Self::Instance => "Everywhere".to_string(),
        }
    }

    /// `for task DF-7`, `for project DF`, `everywhere`: the end of "… is allowed …".
    pub(super) fn phrase(self) -> String {
        match self {
            Self::Task(task) => format!("for task {task}"),
            Self::Project(project) => format!("for project {project}"),
            Self::Instance => "everywhere".to_string(),
        }
    }
}

fn scope_label(record: &EntryRecord) -> String {
    ScopeName::of(record.project_key.as_deref(), record.task_key.as_deref()).label()
}

/// The ports of a cluster service entry: the list, or "all ports".
fn service_ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        "all ports".to_string()
    } else {
        ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `host:port` of an `http-host` entry, `namespace/service` of a cluster service: the target as
/// named in accessible labels and matched by `?highlight=`.
fn target(record: &EntryRecord) -> String {
    match &record.kind {
        EntryKind::HttpHost { host, port } => format!("{host}:{port}"),
        EntryKind::HttpDeny { host } => format!("{host}:{ANY_PORT}"),
        EntryKind::ClusterService {
            namespace, service, ..
        } => format!("{namespace}/{service}"),
    }
}

/// What the search looks at, lower case: the host (or `namespace/service`) and the scope label.
fn search_text(record: &EntryRecord) -> String {
    let name = match &record.kind {
        EntryKind::HttpHost { host, .. } | EntryKind::HttpDeny { host } => host.clone(),
        EntryKind::ClusterService {
            namespace, service, ..
        } => format!("{namespace}/{service}"),
    };
    format!("{name} {}", scope_label(record)).to_lowercase()
}

/// Whether the target of `record` (`host:port`, `host:any` for a deny rule) is the one
/// `?highlight=` names. Cluster services have no `host:port` to highlight.
fn is_highlighted(record: &EntryRecord, highlight: Option<&str>) -> bool {
    !matches!(record.kind, EntryKind::ClusterService { .. })
        && highlight == Some(target(record).as_str())
}

fn effect_badge(record: &EntryRecord) -> Markup {
    match record.kind {
        EntryKind::HttpDeny { .. } => tone_badge(Tone::Failed, "Deny"),
        EntryKind::HttpHost { .. } | EntryKind::ClusterService { .. } => {
            tone_badge(Tone::Completed, "Allow")
        }
    }
}

fn target_cell(record: &EntryRecord, projects: &ProjectHosts) -> Markup {
    match &record.kind {
        EntryKind::HttpHost { host, .. } | EntryKind::HttpDeny { host } => html! {
            span class="flex items-center gap-3" {
                (host_icon(host, HostIconSize::Table, projects))
                span class="min-w-0 truncate font-mono text-[13px]" { (host) }
            }
        },
        EntryKind::ClusterService {
            namespace, service, ..
        } => html! {
            span class="flex items-center gap-3" {
                span aria-hidden="true" class="inline-flex size-6 shrink-0 items-center justify-center rounded-md bg-sunk text-muted [&>svg]:size-4" { (icon(Icon::Server)) }
                span class="min-w-0 truncate font-mono text-[13px]" { (namespace) "/" (service) }
                (tone_badge(Tone::Waiting, "cluster service"))
            }
        },
    }
}

fn ports_cell(record: &EntryRecord) -> String {
    match &record.kind {
        EntryKind::HttpHost { port, .. } => port.to_string(),
        EntryKind::HttpDeny { .. } => ANY_PORT.to_string(),
        EntryKind::ClusterService {
            ports, allow_dns, ..
        } => {
            let dns = if *allow_dns { ", DNS allowed" } else { "" };
            format!("{}{dns}", service_ports(ports))
        }
    }
}

fn header(count: usize) -> Markup {
    html! {
        header class="mb-5 flex flex-wrap items-start justify-between gap-4" {
            div {
                h1 class="text-2xl font-semibold" {
                    "Egress rules " span class="font-normal text-faint" { (count) }
                }
                p class="mt-1 text-sm text-muted" {
                    "External hosts and cluster services that sandboxes may reach."
                }
            }
            div class="flex flex-wrap items-center gap-2" {
                (link_button(ButtonKind::Secondary, "/egress/log", "Live traffic"))
                (open_modal(
                    "/egress/allowlist/service/new",
                    "",
                    action_class(ButtonKind::Secondary),
                    html! { "Add cluster service" },
                ))
                (create_button("/egress/allowlist/host/new", "New egress rule"))
            }
        }
    }
}

/// The page's live region: the header, the search, the table of the page `view` selects and the
/// line below it. The highlighted rows are tinted and the first of them scrolled into view.
fn region(entries: &[EntryRecord], view: &View, projects: &ProjectHosts) -> Markup {
    let page = view.page(entries, |entry| is_highlighted(entry, view.highlight()));
    html! {
        div id="config-list" {
            (header(entries.len()))
            @if entries.is_empty() {
                (empty_panel(
                    "No egress rules yet",
                    "Sandboxes reach no external host until it is allowed. Blocked attempts appear in the live traffic view.",
                    html! {
                        div class="flex flex-wrap items-center justify-center gap-2" {
                            (link_button(ButtonKind::Secondary, "/egress/log", "Open live traffic"))
                            (create_button("/egress/allowlist/host/new", "New egress rule"))
                        }
                    },
                ))
            } @else {
                (search_bar(view))
                // Search and paging swap only this part: the search field keeps its text and focus.
                (results(view, &page, entries.len(), projects))
            }
        }
    }
}

/// The part of the region that search and paging replace: the table (or the empty search) and
/// the line below it.
fn results(view: &View, page: &paging::Page<'_>, total: usize, projects: &ProjectHosts) -> Markup {
    let mut scrolled = false;
    html! {
        div id="rules-results" {
            @if page.matched == 0 {
                (empty_panel(&format!("No rules match \"{}\"", view.q.trim()), "", html! {}))
            } @else {
                // Every column but Target has a fixed width so columns stay put across pages; the
                // `nth-child` indices follow the header order below.
                div class="[&_table]:table-fixed [&_th:nth-child(2)]:w-28 [&_th:nth-child(3)]:w-28 [&_th:nth-child(4)]:w-44 [&_th:nth-child(5)]:w-20" {
                    (data_table(
                        html! {
                            (th(Cell::First, html! { "Target" })) (th(Cell::Text, html! { "Effect" }))
                            (th(Cell::Mono, html! { "Port" })) (th(Cell::Text, html! { "Scope" }))
                            (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                        },
                        html! {
                            @for entry in &page.rows {
                                @let target = target(entry);
                                @let marked = is_highlighted(entry, view.highlight());
                                @let scroll = marked && !std::mem::replace(&mut scrolled, true);
                                tr class={"border-t border-line" @if marked { " bg-accent-soft" }}
                                    aria-current=[marked.then_some("true")]
                                    x-data=[scroll.then_some("")]
                                    x-init=[scroll.then_some("$el.scrollIntoView({ block: 'center' })")] {
                                    (td(Cell::First, target_cell(entry, projects)))
                                    (td(Cell::Text, effect_badge(entry)))
                                    (td(Cell::Mono, html! { (ports_cell(entry)) }))
                                    (td(Cell::Text, html! { (scope_label(entry)) }))
                                    (td(Cell::Actions, html! {
                                        button type="button" hx-delete={"/egress/allowlist/" (entry.id) (view.query(page.number))}
                                            hx-target="#config-list" hx-swap="outerHTML"
                                            aria-label={"Delete egress rule " (target)} title="Delete" class=(ICON_BUTTON_CLASS) {
                                            (icon(Icon::Trash))
                                        }
                                    }))
                                }
                            }
                        },
                    ))
                }
            }
            (footer(view, page, total))
        }
    }
}

/// The search field above the table. Typing reloads the region for page 1 of the new search.
fn search_bar(view: &View) -> Markup {
    html! {
        div class="mb-4 flex justify-end" {
            label class="relative block w-full sm:w-[300px]" {
                span class="sr-only" { "Search rules" }
                span aria-hidden="true" class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-faint" { (icon(Icon::Search)) }
                // The id keeps the focus while the region is replaced.
                input id="rules-search" type="search" name="q" value=(view.q) placeholder="Search host or scope"
                    hx-get=(ALLOWLIST) hx-trigger="input changed delay:300ms" hx-target="#rules-results"
                    hx-swap="outerHTML" hx-select="#rules-results" hx-push-url="true"
                    class={(INPUT_CLASS) " pl-10 font-mono text-sm"};
            }
        }
    }
}

/// The count line below the table and, with more than one page, the paging controls.
fn footer(view: &View, page: &paging::Page<'_>, total: usize) -> Markup {
    let count = if view.search().is_some() {
        format!("{} of {total} rules match", page.matched)
    } else {
        format!("{}\u{2013}{} of {total} rules", page.first, page.last)
    };
    let step = |label: &'static str, to: usize, disabled: bool| {
        html! {
            button type="button" disabled[disabled] class=(action_class(ButtonKind::Secondary))
                hx-get={(ALLOWLIST) (view.query(to))} hx-target="#rules-results" hx-swap="outerHTML"
                hx-select="#rules-results" hx-push-url="true" { (label) }
        }
    };
    html! {
        div class="mt-4 flex flex-wrap items-center justify-between gap-3 text-sm text-muted" {
            p { (count) }
            @if page.pages > 1 {
                nav aria-label="Pages" class="flex items-center gap-2" {
                    (step("Previous", page.number.saturating_sub(1), page.number == 1))
                    span { "Page " (page.number) " of " (page.pages) }
                    (step("Next", page.number + 1, page.number == page.pages))
                }
            }
        }
    }
}

async fn region_markup(state: &WebState, view: &View) -> Result<Markup, Box<Response>> {
    let entries = df_db::list_entries(&state.pool)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(region(&entries, view, &super::project_hosts(state).await))
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

/// The "New egress rule" dialog. `effect` (Allow or Deny) is a radio group; the Alpine component
/// `egressRule` (`assets/egress-rule.js`) disables "Applies to" and Port for Deny and shows
/// `every project` and `any` in them. The server renders the same state, so a form that is shown
/// again after an error keeps it. Disabled fields are not submitted.
fn host_form(options: &[(String, String)], values: &HostForm, errors: &FormErrors) -> Markup {
    let deny = values.is_deny();
    let effect = values.effect;
    let radio = |value: Effect, label: &'static str, tone: &'static str| {
        html! {
            label class={"flex flex-1 cursor-pointer items-center justify-center rounded-md px-3 text-sm font-medium text-muted has-focus-visible:outline-2 has-checked:border has-checked:" (tone)} {
                input type="radio" name="effect" value=(value.as_str()) checked[effect == value] x-model="effect" class="sr-only";
                (label)
            }
        }
    };
    let submit = if deny { "Add deny rule" } else { "Add rule" };
    html! {
        form id="form" class="space-y-4" hx-post="/egress/allowlist/host" hx-target="#form" hx-swap="outerHTML"
            x-data={"egressRule(" (serde_json::json!({ "effect": effect.as_str() })) ")"} {
            (form_error(errors))
            div class="flex flex-col gap-4 md:flex-row" {
                div class="shrink-0" { (field("Effect", "effect", errors, None, html! {
                    div role="radiogroup" aria-label="Effect" class="flex min-h-11 items-stretch gap-1 rounded-lg border border-line bg-sunk p-1 lg:min-h-10" {
                        (radio(Effect::Allow, "Allow", "border-status-completed-fg/40 has-checked:bg-status-completed-bg has-checked:text-status-completed-fg"))
                        (radio(Effect::Deny, "Deny", "border-status-failed-fg/40 has-checked:bg-status-failed-bg has-checked:text-status-failed-fg"))
                    }
                })) }
                div class="min-w-0 flex-1" { (field("Applies to", "project", errors, None, html! {
                    select id="project" name="project" class=(INPUT_CLASS) x-ref="project" ":disabled"="deny" disabled[deny] {
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
                    "x-text"="deny ? 'Add deny rule' : 'Add rule'" { (submit) }
            }
        }
    }
}

fn service_form(options: &[(String, String)], values: &ServiceForm, errors: &FormErrors) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post="/egress/allowlist/service" hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
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
            (form_footer("Add cluster service"))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------------

pub(super) async fn list(State(state): State<WebState>, Query(view): Query<View>) -> Response {
    let entries = match df_db::list_entries(&state.pool).await {
        Ok(entries) => entries,
        Err(e) => return internal_error(&e),
    };
    let projects = super::project_hosts(&state).await;
    list_page(
        "Egress rules",
        Nav::Allowlist,
        region(&entries, &view, &projects),
    )
}

pub(super) async fn new_host(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to(ALLOWLIST).into_response();
    }
    match project_options(&state).await {
        Ok(options) => modal(
            "New egress rule",
            host_form(&options, &HostForm::default(), &FormErrors::default()),
        ),
        Err(response) => *response,
    }
}

pub(super) async fn new_service(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to(ALLOWLIST).into_response();
    }
    match project_options(&state).await {
        Ok(options) => modal(
            "Add cluster service",
            service_form(&options, &ServiceForm::default(), &FormErrors::default()),
        ),
        Err(response) => *response,
    }
}

/// Stores `kind`. `Some` is the final response (the updated list, or an error page); `None`
/// means the database refused the entry and `errors` says why.
async fn save(
    state: &WebState,
    user: &CurrentUser,
    project: &str,
    kind: &EntryKind,
    errors: &mut FormErrors,
) -> Option<Response> {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let project_key = (!project.is_empty()).then_some(project);
    match df_db::add_entry(&state.pool, actor, project_key, kind).await {
        Ok(_) => Some(match region_markup(state, &View::default()).await {
            Ok(region) => saved(region),
            Err(response) => *response,
        }),
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

pub(super) async fn add_host(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<HostForm>,
) -> Response {
    let mut errors = FormErrors::default();
    let host = validate_allowlist_host(&form.host).map_err(|m| errors.add("host", m));
    if form.is_deny() {
        // A deny rule has no project and no port: both fields are ignored.
        if let Ok(host) = host
            && let Some(response) = save_deny(&state, &user, &host, &mut errors).await
        {
            return response;
        }
    } else {
        let port = validate_port(&form.port).map_err(|m| errors.add("port", m));
        if let (Ok(host), Ok(port)) = (host, port) {
            let kind = EntryKind::HttpHost { host, port };
            if let Some(response) = save(&state, &user, &form.project, &kind, &mut errors).await {
                return response;
            }
        }
    }
    show_again(&state, errors, |options, errors| {
        host_form(options, &form, errors)
    })
    .await
}

/// Stores the deny rule for `host`; like [`save`], `None` means `errors` says why not.
async fn save_deny(
    state: &WebState,
    user: &CurrentUser,
    host: &str,
    errors: &mut FormErrors,
) -> Option<Response> {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::egress::add_deny_rule(&state.pool, host, actor).await {
        Ok(_) => Some(match region_markup(state, &View::default()).await {
            Ok(region) => saved(region),
            Err(response) => *response,
        }),
        Err(DbError::Conflict(message)) => {
            errors.add("", message);
            None
        }
        Err(e) => Some(internal_error(&e)),
    }
}

pub(super) async fn add_service(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ServiceForm>,
) -> Response {
    let mut errors = FormErrors::default();
    let namespace =
        validate_k8s_name(&form.namespace, "Namespace").map_err(|m| errors.add("namespace", m));
    let service = validate_k8s_name(&form.service, "Service").map_err(|m| errors.add("service", m));
    let ports = validate_ports(&form.ports).map_err(|m| errors.add("ports", m));
    if let (Ok(namespace), Ok(service), Ok(ports)) = (namespace, service, ports)
        && service_exists(&state, &namespace, &service, &mut errors).await
    {
        let kind = EntryKind::ClusterService {
            namespace,
            service,
            ports,
            allow_dns: form.allow_dns.is_some(),
        };
        if let Some(response) = save(&state, &user, &form.project, &kind, &mut errors).await {
            return response;
        }
    }
    show_again(&state, errors, |options, errors| {
        service_form(options, &form, errors)
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

/// Deletes an entry and renders the same search and page again (a page that no longer exists
/// shows the last one).
pub(super) async fn remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    Query(view): Query<View>,
) -> Response {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::delete_entry(&state.pool, actor, id).await {
        Ok(_) => match region_markup(
            &state,
            &View {
                highlight: String::new(),
                ..view
            },
        )
        .await
        {
            Ok(region) => region.into_response(),
            Err(response) => *response,
        },
        Err(DbError::NotFound(_)) => not_found("The entry"),
        Err(e) => internal_error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_default_to_443_and_must_be_valid() {
        assert_eq!(validate_port(""), Ok(443));
        assert_eq!(validate_port(" 80 "), Ok(80));
        for bad in ["0", "65536", "http", "-1"] {
            assert!(validate_port(bad).is_err(), "{bad}");
        }
        assert_eq!(validate_required_port(" 80 "), Ok(80));
        assert_eq!(validate_required_port(""), validate_port("0"));
        assert_eq!(
            validate_ports("50051, 8080 9090"),
            Ok(vec![50051, 8080, 9090])
        );
        assert_eq!(validate_ports(""), Ok(vec![]));
        assert!(validate_ports("80,x").is_err());
        assert!(validate_ports("0").is_err());
    }

    #[test]
    fn a_scope_is_named_by_its_narrowest_key() {
        let names = |project, task| {
            let scope = ScopeName::of(project, task);
            (scope.label(), scope.phrase())
        };
        assert_eq!(
            names(Some("DF"), Some("DF-7")),
            ("Task DF-7".into(), "for task DF-7".into())
        );
        assert_eq!(
            names(Some("DF"), None),
            ("Project DF".into(), "for project DF".into())
        );
        assert_eq!(
            names(None, None),
            ("Everywhere".into(), "everywhere".into())
        );
    }

    #[test]
    fn kubernetes_names_are_dns_labels() {
        assert_eq!(
            validate_k8s_name("nativelink", "Namespace"),
            Ok("nativelink".into())
        );
        assert!(validate_k8s_name("Native", "Namespace").is_err());
        assert!(validate_k8s_name("a.b", "Service").is_err());
        assert_eq!(
            validate_k8s_name("", "Service"),
            Err("Service is required.".into())
        );
    }
}
