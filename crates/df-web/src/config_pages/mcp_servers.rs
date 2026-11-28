//! MCP server pages (`specs/mcp-servers/spec.md`): the upstream HTTP MCP servers the gateway
//! forwards tool calls to, with the credential Dark Factory uses for each. Admins only.
//!
//! `project_tab` holds the tab "MCP servers" of the project settings page, which enables servers
//! per project.

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get};
use axum::{Extension, Router, middleware};
use df_core::CredentialRef;
use df_core::mcp::{self, McpCredentialScheme, McpError, url_without_query};
use df_db::DbError;
use df_db::catalog::Actor;
use df_db::mcp_servers::{McpServerConflict, McpServerInput, McpServerRecord};
use maud::{Markup, html};
use serde::Deserialize;
use std::fmt;
use uuid::Uuid;

use super::{
    create_button, form_footer, form_grid, full_row, list_header_with_subtitle, list_page, modal,
    saved,
};
use crate::WebState;
use crate::auth::{CurrentUser, require_admin};
use crate::components::forms::{INPUT_CLASS, described_by, field, invalid as invalid_attr};
use crate::components::{
    Cell, FormErrors, ICON_BUTTON_CLASS, Icon, Tone, actions_cell, data_table, empty_panel,
    entry_button, entry_tr, form_error, icon, name_cell, readonly_field, select_field, separator,
    td, text_field, th, tone_badge,
};
use crate::layout::{Nav, Shell};
use crate::responses::{internal_error, invalid, not_found};

pub(super) mod project_tab;

/// The subtitle of the list.
const SUBTITLE: &str = "Third-party MCP servers that agents reach through the gateway. The gateway adds the credential; it never enters the sandbox.";
const NAME_HINT: &str = "Lowercase letters, digits and single hyphens, starting with a letter, at most 20 characters. Agents see the tools as <name>__<tool>. The name can't be changed later.";
const URL_HINT: &str = "MCP Streamable HTTP endpoint; path and query parameters are kept. The URL is not secret: it is stored and shown here. A token belongs into the credential reference, not into the URL.";
const PRIVATE_NETWORK_HINT: &str = "Skips the check that refuses private, link-local and loopback addresses. Only for servers on internal names in DNS you control: it also allows localhost and the gateway's own ports.";
const WARNING_TITLE: &str = "Agents get every tool of this server, including tools that write";
const WARNING_BODY: &str = "In tasks and chats, agents see every tool the server offers, with its unreviewed description. Writes bypass the Git proxy's push checks and the factory's approvals. The credential's scope is the effective permission: use a read-only token where possible, and never a forge token with write access.";
const HEADER_NAME_HINT: &str = "Any header except hop-by-hop headers, Host, Content-Length, Content-Type, Accept, Mcp-Session-Id and Mcp-Protocol-Version.";
const QUERY_NAME_HINT: &str = "Added after the URL's own parameters, URL-encoded. Must not already be in the URL (compared after percent-decoding).";
const QUERY_PREVIEW_HINT: &str = "The upstream and proxies in front of it may log full URLs, query included. Prefer Bearer or Header when the server supports them.";
const EXAMPLE_TOKEN: &str = "<value>";

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/mcp-servers", get(list).post(create))
        .route("/mcp-servers/new", get(new))
        .route(
            "/mcp-servers/{id}",
            get(edit).post(update).merge(delete(remove)),
        )
        .merge(project_tab::routes())
        .route_layer(middleware::from_fn(require_admin))
}

#[derive(Default, Clone, Deserialize)]
#[serde(default)]
struct ServerForm {
    name: String,
    url: String,
    credential_ref_id: String,
    /// `bearer`, `header` or `query`.
    scheme: String,
    credential_header: String,
    credential_prefix: String,
    credential_query_param: String,
    /// A checked box posts `on`.
    private_network: String,
}

/// Without the URL's query, where a credential may sit.
impl fmt::Debug for ServerForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerForm")
            .field("name", &self.name)
            .field("url", &url_without_query(&self.url))
            .field("credential_ref_id", &self.credential_ref_id)
            .field("scheme", &self.scheme)
            .field("credential_header", &self.credential_header)
            .field("credential_prefix", &self.credential_prefix)
            .field("credential_query_param", &self.credential_query_param)
            .field("private_network", &self.private_network)
            .finish()
    }
}

impl ServerForm {
    fn from_record(record: &McpServerRecord) -> Self {
        let input = &record.input;
        let mut form = Self {
            name: input.name.clone(),
            url: input.url.clone(),
            credential_ref_id: input.credential_ref_id.to_string(),
            scheme: input.scheme.as_str().to_owned(),
            private_network: if input.private_network { "on" } else { "" }.to_owned(),
            ..Self::default()
        };
        match &input.scheme {
            McpCredentialScheme::Bearer => {}
            McpCredentialScheme::Header { name, prefix } => {
                name.clone_into(&mut form.credential_header);
                prefix.clone_into(&mut form.credential_prefix);
            }
            McpCredentialScheme::Query { parameter } => {
                parameter.clone_into(&mut form.credential_query_param);
            }
        }
        form
    }

    /// The scheme shown while none is chosen: Bearer, the first of the select.
    fn effective_scheme(&self) -> &str {
        match self.scheme.as_str() {
            "header" | "query" => &self.scheme,
            _ => "bearer",
        }
    }
}

/// The form field an error of the configuration belongs to.
fn error_field(error: &McpError) -> &'static str {
    match error {
        McpError::InvalidName => "name",
        McpError::InvalidUrl | McpError::PlainHttpNotAllowed => "url",
        McpError::HeaderNameMissing | McpError::HeaderName(_) | McpError::ReservedHeader(_) => {
            "credential_header"
        }
        McpError::InvalidPrefix => "credential_prefix",
        McpError::ParameterMissing | McpError::ParameterInUrl(_) => "credential_query_param",
    }
}

/// Checks the form. `known` are the ids of the credential references; with `editing` the name is
/// not checked, the stored one stays.
fn validate(
    form: &ServerForm,
    known: &[Uuid],
    allow_plain_http: bool,
    editing: bool,
) -> Result<McpServerInput, FormErrors> {
    let mut errors = FormErrors::default();
    let name = form.name.trim().to_owned();
    if !editing && let Err(error) = mcp::validate_name(&name) {
        errors.add(error_field(&error), error.to_string());
    }
    let credential_ref_id = match Uuid::parse_str(&form.credential_ref_id) {
        Ok(id) if known.contains(&id) => id,
        _ => {
            errors.add("credential_ref_id", "Choose a credential reference.");
            Uuid::nil()
        }
    };
    let scheme = match form.effective_scheme() {
        "header" => McpCredentialScheme::Header {
            name: form.credential_header.trim().to_owned(),
            prefix: form.credential_prefix.clone(),
        },
        "query" => McpCredentialScheme::Query {
            parameter: form.credential_query_param.trim().to_owned(),
        },
        _ => McpCredentialScheme::Bearer,
    };
    let url = form.url.trim().to_owned();
    match mcp::validate_url(&url, allow_plain_http) {
        Ok(parsed) => {
            if let Err(error) = mcp::validate_scheme(&scheme, &parsed) {
                errors.add(error_field(&error), error.to_string());
            }
        }
        Err(error) => errors.add(error_field(&error), error.to_string()),
    }
    errors.into_result(McpServerInput {
        name,
        url,
        credential_ref_id,
        scheme,
        private_network: !form.private_network.is_empty(),
    })
}

// ---------------------------------------------------------------------------------------------
// Form
// ---------------------------------------------------------------------------------------------

fn credential_options(credentials: &[CredentialRef]) -> Vec<(String, String)> {
    credentials
        .iter()
        .map(|c| {
            (
                c.id.to_string(),
                format!("{} ({}/{})", c.name, c.secret.name, c.secret.key),
            )
        })
        .collect()
}

/// The Alpine state of the form (plain data): the chosen scheme and what the preview needs.
fn form_state(values: &ServerForm) -> String {
    serde_json::json!({
        "scheme": values.effective_scheme(),
        "url": values.url,
        "param": values.credential_query_param,
    })
    .to_string()
}

fn warning() -> Markup {
    html! {
        div class="rounded-lg bg-status-limited-bg px-4 py-3 text-sm text-status-limited-fg" {
            p class="font-semibold" { (WARNING_TITLE) }
            p class="mt-1" { (WARNING_BODY) }
        }
    }
}

/// The controls of the chosen scheme. All three blocks are rendered and Alpine shows the chosen
/// one, so typed values survive a change of the scheme.
fn scheme_fields(values: &ServerForm, errors: &FormErrors) -> Markup {
    let chosen = values.effective_scheme();
    let hidden = |scheme: &str| (chosen != scheme).then_some("display: none");
    html! {
        div x-show="scheme === 'bearer'" style=[hidden("bearer")] class="md:col-span-2" {
            p class="text-xs text-faint" { "Sent as Authorization: Bearer " (EXAMPLE_TOKEN) "." }
        }
        div x-show="scheme === 'header'" style=[hidden("header")] class="contents" {
            (field("Header name", "credential_header", errors, Some(HEADER_NAME_HINT), html! {
                input id="credential_header" name="credential_header" type="text" value=(values.credential_header)
                    aria-invalid=[invalid_attr("credential_header", errors)]
                    aria-describedby=[described_by("credential_header", true, errors)]
                    class={(INPUT_CLASS) " font-mono"};
            }))
            (field("Value", "credential_prefix", errors,
                Some("Optional prefix, then the secret. Sent as <name>: <prefix><value>. The token is an example."),
                html! {
                    div class="flex min-h-11 items-stretch overflow-hidden rounded-lg border border-line-strong bg-panel lg:min-h-10" {
                        input id="credential_prefix" name="credential_prefix" type="text" value=(values.credential_prefix) placeholder="Prefix"
                            aria-invalid=[invalid_attr("credential_prefix", errors)]
                            aria-describedby=[described_by("credential_prefix", true, errors)]
                            class="w-24 shrink-0 border-0 border-r border-line bg-transparent px-3 font-mono text-base text-ink";
                        span class="min-w-0 flex-1 self-center truncate px-3 font-mono text-base text-faint" { (EXAMPLE_TOKEN) }
                    }
                }))
        }
        div x-show="scheme === 'query'" style=[hidden("query")] class="contents" {
            (field("Parameter name", "credential_query_param", errors, Some(QUERY_NAME_HINT), html! {
                input id="credential_query_param" name="credential_query_param" type="text" value=(values.credential_query_param)
                    x-model="param"
                    aria-invalid=[invalid_attr("credential_query_param", errors)]
                    aria-describedby=[described_by("credential_query_param", true, errors)]
                    class={(INPUT_CLASS) " font-mono"};
            }))
            (field("Sent to", "credential_preview", errors, Some(QUERY_PREVIEW_HINT), html! {
                div id="credential_preview" class="flex min-h-11 items-center break-all rounded-lg bg-sunk px-3 py-2 font-mono text-[13px] text-ink lg:min-h-10"
                    x-text={"url + (url.includes('?') ? '&' : '?') + (param || 'name') + '=" (EXAMPLE_TOKEN) "'"} {
                    (preview(&values.url, &values.credential_query_param))
                }
            }))
        }
    }
}

/// What the preview shows before Alpine runs: the URL ending in the credential parameter.
fn preview(url: &str, parameter: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    let name = if parameter.is_empty() {
        "name"
    } else {
        parameter
    };
    format!("{url}{separator}{name}={EXAMPLE_TOKEN}")
}

fn server_form(
    action: &str,
    values: &ServerForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    editing: bool,
) -> Markup {
    let submit = if editing { "Save" } else { "Create MCP server" };
    let chosen = values.effective_scheme();
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" x-data=(form_state(values)) {
            (form_error(errors))
            (form_grid(html! {
                @if editing {
                    (readonly_field("Name", "name", &values.name, errors,
                        Some(&format!("The name can't be changed: it is part of every tool name agents see ({}__<tool>).", values.name))))
                } @else {
                    (text_field("Name", "name", &values.name, errors, Some(NAME_HINT)))
                }
                div class="hidden md:block" {}
                (full_row(field("URL", "url", errors, Some(URL_HINT), html! {
                    input id="url" name="url" type="text" value=(values.url) x-model="url" placeholder="https://mcp.example.com/mcp"
                        aria-invalid=[invalid_attr("url", errors)] aria-describedby=[described_by("url", true, errors)]
                        class={(INPUT_CLASS) " font-mono"};
                })))
                (select_field("Credential reference", "credential_ref_id", &credential_options(credentials),
                    &values.credential_ref_id, Some("Choose…"), errors))
                (field("Credential scheme", "scheme", errors, None, html! {
                    select id="scheme" name="scheme" class=(INPUT_CLASS) x-model="scheme"
                        aria-invalid=[invalid_attr("scheme", errors)] {
                        option value="bearer" selected[chosen == "bearer"] { "Bearer" }
                        option value="header" selected[chosen == "header"] { "Header" }
                        option value="query" selected[chosen == "query"] { "Query parameter" }
                    }
                }))
                (full_row(warning()))
                (scheme_fields(values, errors))
                (full_row(html! {
                    label class="inline-flex min-h-11 cursor-pointer items-center gap-2 text-sm font-medium" for="private_network" {
                        input id="private_network" name="private_network" type="checkbox" value="on" checked[!values.private_network.is_empty()]
                            class="size-4 accent-accent-text";
                        "Private network"
                    }
                    p class="text-xs text-faint" { (PRIVATE_NETWORK_HINT) }
                }))
            }))
            (form_footer(submit))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------------------------

fn scheme_text(scheme: &McpCredentialScheme) -> String {
    match scheme {
        McpCredentialScheme::Bearer => "Authorization: Bearer".to_owned(),
        McpCredentialScheme::Header { name, .. } => format!("{name} header"),
        McpCredentialScheme::Query { parameter } => format!("{parameter} query parameter"),
    }
}

fn region(
    servers: &[McpServerRecord],
    credentials: &[CredentialRef],
    notice: Option<&str>,
) -> Markup {
    html! {
        div id="config-list" {
            (list_header_with_subtitle("MCP servers", servers.len(), Some(SUBTITLE),
                create_button("/mcp-servers/new", "New MCP server"), None))
            @if let Some(notice) = notice {
                p class="mb-4 rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger" role="alert" { (notice) }
            }
            @if servers.is_empty() {
                (empty_panel(
                    "No MCP servers yet",
                    "Register an HTTP MCP server with a credential reference, then enable it for a project. Its tools appear in the project's next run.",
                    create_button("/mcp-servers/new", "New MCP server"),
                ))
            } @else {
                (data_table(
                    html! {
                        (th(Cell::First, html! { "Name" }))
                        (th(Cell::NoWrap, html! { "Projects" }))
                        (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                    },
                    html! { @for server in servers { (row(server, credentials)) } },
                ))
            }
        }
    }
}

fn row(server: &McpServerRecord, credentials: &[CredentialRef]) -> Markup {
    let input = &server.input;
    let credential = credentials
        .iter()
        .find(|c| c.id.as_uuid() == input.credential_ref_id);
    html! {
        (entry_tr(html! {
            (name_cell(
                html! {
                    (entry_button(&format!("/mcp-servers/{}", server.id), &format!("Edit MCP server {}", input.name), &input.name))
                    @if input.private_network { " " (tone_badge(Tone::Queued, "Private network")) }
                },
                html! {
                    (input.url)
                    (separator("·"))
                    (scheme_text(&input.scheme))
                    (separator("·"))
                    span class="font-mono" {
                        @if let Some(c) = credential { (c.name) " (" (c.secret.name) "/" (c.secret.key) ")" } @else { "missing" }
                    }
                },
            ))
            (td(Cell::Text, html! {
                span class="block w-max max-w-60 wrap-anywhere" {
                    @if server.projects.is_empty() { span class="text-faint" { "no project" } }
                    @else { (server.projects.join(", ")) }
                }
            }))
            (actions_cell(html! {
                button type="button" hx-delete={"/mcp-servers/" (server.id)} hx-target="#config-list" hx-swap="outerHTML"
                    hx-confirm={"Delete MCP server " (input.name) "?"}
                    aria-label={"Delete MCP server " (input.name)} title="Delete" class=(ICON_BUTTON_CLASS) {
                    (icon(Icon::Trash))
                }
            }))
        }))
    }
}

async fn list_markup(state: &WebState, notice: Option<&str>) -> Result<Markup, Box<Response>> {
    let (servers, credentials) = tokio::try_join!(
        df_db::mcp_servers::list_mcp_servers(&state.pool),
        df_db::catalog::list_credential_refs(&state.pool)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(region(&servers, &credentials, notice))
}

async fn list(State(state): State<WebState>, shell: Shell) -> Response {
    match list_markup(&state, None).await {
        Ok(markup) => list_page("MCP servers", Nav::McpServers, &shell, markup),
        Err(response) => *response,
    }
}

// ---------------------------------------------------------------------------------------------
// Create, edit, delete
// ---------------------------------------------------------------------------------------------

async fn new(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::htmx::is_htmx(&headers) {
        return Redirect::to("/mcp-servers").into_response();
    }
    match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(credentials) => modal(
            "New MCP server",
            server_form(
                "/mcp-servers",
                &ServerForm::default(),
                &FormErrors::default(),
                &credentials,
                false,
            ),
        ),
        Err(e) => internal_error(&e),
    }
}

async fn edit(State(state): State<WebState>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    if !crate::htmx::is_htmx(&headers) {
        return Redirect::to("/mcp-servers").into_response();
    }
    let (record, credentials) = match tokio::try_join!(
        df_db::mcp_servers::get_mcp_server(&state.pool, id),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(record), credentials)) => (record, credentials),
        Ok((None, _)) => return not_found("The MCP server"),
        Err(e) => return internal_error(&e),
    };
    modal(
        "Edit MCP server",
        server_form(
            &format!("/mcp-servers/{id}"),
            &ServerForm::from_record(&record),
            &FormErrors::default(),
            &credentials,
            true,
        ),
    )
}

async fn create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<ServerForm>,
) -> Response {
    save(state, user, None, form.0).await
}

async fn update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    form: Form<ServerForm>,
) -> Response {
    save(state, user, Some(id), form.0).await
}

async fn save(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    id: Option<Uuid>,
    mut form: ServerForm,
) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(credentials) => credentials,
        Err(e) => return internal_error(&e),
    };
    let action = id.map_or_else(
        || "/mcp-servers".to_owned(),
        |id| format!("/mcp-servers/{id}"),
    );
    // The name of a stored server is not an input: the stored one stays.
    if let Some(id) = id {
        match df_db::mcp_servers::get_mcp_server(&state.pool, id).await {
            Ok(Some(record)) => form.name = record.input.name,
            Ok(None) => return not_found("The MCP server"),
            Err(e) => return internal_error(&e),
        }
    }
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let errors = match validate(&form, &known, state.allow_plain_http, id.is_some()) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::mcp_servers::save_mcp_server(&state.pool, actor, id, &input).await {
                Ok(_) => {
                    return match list_markup(&state, None).await {
                        Ok(region) => saved(region),
                        Err(response) => *response,
                    };
                }
                Err(DbError::NotFound(_)) => return not_found("The MCP server"),
                Err(DbError::McpServer(conflict)) => {
                    let field = match &conflict {
                        McpServerConflict::Name(_) => "name",
                        McpServerConflict::Credential(_) => "credential_ref_id",
                    };
                    let mut errors = FormErrors::default();
                    errors.add(field, conflict.message());
                    errors
                }
                Err(e) => return internal_error(&e),
            }
        }
        Err(errors) => errors,
    };
    invalid(server_form(
        &action,
        &form,
        &errors,
        &credentials,
        id.is_some(),
    ))
}

async fn remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
) -> Response {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let notice = match df_db::mcp_servers::delete_mcp_server(&state.pool, actor, id).await {
        Ok(()) => None,
        Err(DbError::Conflict(message)) => Some(message),
        Err(DbError::NotFound(_)) => return not_found("The MCP server"),
        Err(e) => return internal_error(&e),
    };
    match list_markup(&state, notice.as_deref()).await {
        Ok(markup) => markup.into_response(),
        Err(response) => *response,
    }
}

#[cfg(test)]
mod tests;
