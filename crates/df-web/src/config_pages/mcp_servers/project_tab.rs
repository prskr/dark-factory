//! The tab "MCP servers" of the project settings page (`specs/mcp-servers/spec.md`, "Enablement
//! per project"): which registered servers the runs of the project may use. Admins only.
//!
//! The tab loads and saves through its own route, never through the project form: its checkboxes
//! and button belong to an empty form next to the project form (forms cannot be nested), and
//! `save_project` never sees them.

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Router, body::Bytes};
use df_db::DbError;
use df_db::catalog::{Actor, ProjectRecord};
use df_db::mcp_servers::McpServerRecord;
use maud::{Markup, html};
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::form_group;
use crate::components::{ButtonKind, Icon, Tone, icon, link_button, tone_badge};
use crate::responses::{internal_error, not_found};

const FORM_ID: &str = "mcp-servers-form";
const PANEL_ID: &str = "mcp-servers-panel";
const INTRO: &str = "Runs of this project, chats included, get every tool of the servers checked here, with each server's shared credential. Nothing is enabled by default. Changes apply to running runs at once.";

pub(super) fn routes() -> Router<WebState> {
    Router::new().route("/projects/{key}/mcp-servers", get(region).post(save))
}

fn path(key: &str) -> String {
    format!("/projects/{key}/mcp-servers")
}

/// The panel of the tab: it loads its content when it scrolls into view.
pub(crate) fn panel(key: &str) -> Markup {
    html! {
        div id=(PANEL_ID) hx-get=(path(key)) hx-trigger="intersect once" hx-swap="outerHTML" class="text-sm text-muted" {
            "Loading MCP servers…"
        }
    }
}

/// The empty form the checkboxes and the save button belong to. The project page places it
/// outside the project form.
pub(crate) fn form_anchor(key: &str) -> Markup {
    html! { form id=(FORM_ID) hx-post=(path(key)) hx-target={"#" (PANEL_ID)} hx-swap="outerHTML" {} }
}

fn content(servers: &[McpServerRecord], enabled: &[Uuid], saved: bool) -> Markup {
    html! {
        div id=(PANEL_ID) class="space-y-4" {
            (form_group("MCP servers", html! {
                p class="text-sm text-muted" { (INTRO) }
                @if servers.is_empty() {
                    div class="rounded-xl border border-dashed border-line-strong px-4 py-6 text-center" {
                        p class="font-medium" { "No MCP servers registered" }
                        p class="mt-1 text-sm text-muted" { "An admin registers MCP servers for the whole instance on the MCP servers page; they appear here to enable for this project." }
                        div class="mt-4 flex justify-center" { (link_button(ButtonKind::Secondary, "/mcp-servers", "Open MCP servers")) }
                    }
                } @else {
                    ul class="divide-y divide-line rounded-lg border border-line" {
                        @for server in servers { (row(server, enabled.contains(&server.id))) }
                    }
                    div class="flex items-center gap-3" {
                        button type="submit" form=(FORM_ID) class=(crate::components::action_class(ButtonKind::Primary)) { "Save MCP servers" }
                        @if saved {
                            span role="status" class="inline-flex items-center gap-1.5 text-sm text-accent-text" { (icon(Icon::Check)) "MCP servers saved" }
                        }
                    }
                }
            }))
        }
    }
}

fn row(server: &McpServerRecord, checked: bool) -> Markup {
    let input = &server.input;
    html! {
        li class="flex items-start gap-3 px-4 py-3" {
            input id={"mcp-" (server.id)} type="checkbox" name="server" value=(server.id) form=(FORM_ID) checked[checked]
                class="mt-1 size-4 accent-accent-text";
            label for={"mcp-" (server.id)} class="min-w-0 flex-1 cursor-pointer" {
                span class="flex flex-wrap items-center gap-2" {
                    span class="font-medium" { (input.name) }
                    @if input.private_network { (tone_badge(Tone::Queued, "Private network")) }
                }
                span class="block truncate text-[13px] text-muted" { (input.url) }
            }
        }
    }
}

async fn load(
    state: &WebState,
    key: &str,
) -> Result<(ProjectRecord, Vec<McpServerRecord>, Vec<Uuid>), Box<Response>> {
    let project = match df_db::catalog::get_project_by_key(&state.pool, key).await {
        Ok(Some(project)) => project,
        Ok(None) => return Err(Box::new(not_found("The project"))),
        Err(e) => return Err(Box::new(internal_error(&e))),
    };
    let (servers, enabled) = tokio::try_join!(
        df_db::mcp_servers::list_mcp_servers(&state.pool),
        df_db::mcp_servers::project_mcp_server_ids(&state.pool, project.id)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok((project, servers, enabled))
}

/// `GET /projects/{key}/mcp-servers`: the content of the tab; a plain browser request opens it.
async fn region(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !crate::htmx::is_htmx(&headers) {
        return Redirect::to(&format!("/projects/{key}?tab=mcp-servers")).into_response();
    }
    match load(&state, &key).await {
        Ok((_project, servers, enabled)) => content(&servers, &enabled, false).into_response(),
        Err(response) => *response,
    }
}

/// `POST /projects/{key}/mcp-servers`: makes the checked servers the enabled ones.
async fn save(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    body: Bytes,
) -> Response {
    let (project, _servers, _enabled) = match load(&state, &key).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let chosen: Vec<Uuid> = url::form_urlencoded::parse(&body)
        .filter(|(name, _value)| name == "server")
        .filter_map(|(_name, value)| Uuid::parse_str(&value).ok())
        .collect();
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::mcp_servers::set_project_mcp_servers(&state.pool, actor, project.id, &chosen).await
    {
        Ok(()) => {}
        Err(DbError::NotFound(_)) => return not_found("The MCP server"),
        Err(e) => return internal_error(&e),
    }
    match load(&state, &key).await {
        Ok((_project, servers, enabled)) => content(&servers, &enabled, true).into_response(),
        Err(response) => *response,
    }
}
