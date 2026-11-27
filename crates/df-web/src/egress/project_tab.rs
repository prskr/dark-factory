//! The Egress tab of the project settings page (egress-rule-editing): the rules of the project and
//! of its tasks in the layout of the rules page, the read-only instance rules below them, and the
//! hints "Also allowed for project …" (with "Make global") and "Already allowed everywhere" (with
//! "Remove").
//!
//! The tab is a placeholder in the project form that loads [`region_markup`] when it is first
//! shown. Its dialogs and actions answer with the region again (`#project-egress`). Because the
//! tab sits inside the project `<form>`, htmx would send the project's fields with a `POST` of the
//! tab (htmx 4 sends the enclosing form of every request that has a body); the buttons that post
//! belong to the empty form of [`form_anchor`] instead (`form` attribute), so they send their own
//! values only. `DELETE` and `GET` requests never include an enclosing form.

use std::collections::HashMap;

use axum::Extension;
use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use df_db::catalog::Actor;
use df_db::{DbError, EntryKind, EntryRecord};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::allowlist::dialogs::{Answer, respond};
use super::allowlist::rows::{self, RowEnv, scope_label, target};
use super::allowlist::{Origin, validate_required_port};
use crate::WebState;
use crate::auth::{CurrentUser, error_page};
use crate::components::{
    ButtonKind, Cell, Icon, ProjectHosts, action_class, data_table, empty_panel, icon, td, th,
};
use crate::config_pages::open_modal;
use crate::htmx::Region;
use crate::responses::{internal_error, not_found};

/// Id of the empty form that the posting buttons of the tab belong to.
const ACTIONS_FORM: &str = "egress-actions";

/// Classes of an action in a hint line: an underlined link that is a button.
const HINT_ACTION: &str =
    "font-medium text-ink underline underline-offset-2 hover:text-accent-text";

/// The project form's panel of the tab: loads the region when the tab is first shown.
pub(crate) fn placeholder(key: &str) -> Markup {
    html! {
        div hx-get={"/projects/" (key) "/egress"} hx-trigger="intersect once" hx-target="this" hx-swap="outerHTML"
            class="text-sm text-muted" { "Loading egress rules…" }
    }
}

/// The empty form that the posting buttons of the tab belong to; the project page places it
/// outside the project form.
pub(crate) fn form_anchor() -> Markup {
    html! { form id=(ACTIONS_FORM) {} }
}

/// `GET /projects/{key}/egress`: the region of the tab; a plain browser request opens the tab.
pub(super) async fn region(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !crate::htmx::is_htmx(&headers) {
        return Redirect::to(&format!("/projects/{key}?tab=egress")).into_response();
    }
    match region_markup(&state, &key).await {
        Ok(region) => region.into_response(),
        Err(response) => *response,
    }
}

/// The region of the tab of project `key`: [`not_found`] for an unknown project.
pub(super) async fn region_markup(state: &WebState, key: &str) -> Result<Markup, Box<Response>> {
    match df_db::catalog::get_project_by_key(&state.pool, key).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(Box::new(not_found("The project"))),
        Err(e) => return Err(Box::new(internal_error(&e))),
    }
    let entries = df_db::list_entries(&state.pool)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(render(key, &entries, &super::project_hosts(state).await))
}

/// The fields of `POST /egress/allowlist/make-global`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct MakeGlobalForm {
    host: String,
    port: String,
    #[serde(flatten)]
    origin: Origin,
}

/// `POST /egress/allowlist/make-global`: replaces the project rules for the host and port by one
/// instance rule and answers with the region the request came from. An instance rule that exists
/// already, or no project rule left, changes nothing; the region shows the current state.
pub(super) async fn make_global(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<MakeGlobalForm>,
) -> Response {
    let Ok(port) = validate_required_port(&form.port) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "Bad request",
            "The port is not valid.",
        );
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::make_global(&state.pool, actor, &form.host, port).await {
        Ok(_) | Err(DbError::Conflict(_) | DbError::NotFound(_)) => {
            respond(&state, form.origin.resolve(), Answer::Refreshed).await
        }
        Err(e) => internal_error(&e),
    }
}

/// What a project rule has to say about the same rule elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hint {
    /// Other projects have a rule for the same host and port.
    Elsewhere(Vec<String>),
    /// An instance rule covers it.
    Everywhere,
}

/// The hint of every `http-host` rule of a project or a task that has one. "Already allowed
/// everywhere" wins; task rules are never "also allowed" elsewhere and do not count for others.
fn hints(entries: &[EntryRecord]) -> HashMap<Uuid, Hint> {
    let is_instance = |e: &EntryRecord| e.project_key.is_none() && e.task_key.is_none();
    // Rules by host and port, built once: one scan per rule would be quadratic.
    let mut by_target: HashMap<_, Vec<&EntryRecord>> = HashMap::new();
    for entry in entries {
        if let EntryKind::HttpHost { host, port } = &entry.kind {
            by_target.entry((host, port)).or_default().push(entry);
        }
    }
    let mut hints = HashMap::new();
    for entry in entries {
        let EntryKind::HttpHost { host, port } = &entry.kind else {
            continue;
        };
        if is_instance(entry) {
            continue;
        }
        let Some(same) = by_target.get(&(host, port)) else {
            continue;
        };
        if same.iter().any(|other| is_instance(other)) {
            hints.insert(entry.id, Hint::Everywhere);
            continue;
        }
        if entry.task_key.is_some() {
            continue;
        }
        let mut others: Vec<String> = same
            .iter()
            .filter(|other| other.task_key.is_none() && other.project_key != entry.project_key)
            .filter_map(|other| other.project_key.clone())
            .collect();
        others.sort();
        others.dedup();
        if !others.is_empty() {
            hints.insert(entry.id, Hint::Elsewhere(others));
        }
    }
    hints
}

/// `A`, `A and B`, `A, B and C`.
fn join_keys(keys: &[String]) -> String {
    match keys {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn render(key: &str, entries: &[EntryRecord], projects: &ProjectHosts) -> Markup {
    let query = Origin::project_query(key);
    let env = RowEnv {
        projects,
        query: &query,
        region: Region::ProjectEgress.selector(),
    };
    let own: Vec<&EntryRecord> = entries
        .iter()
        .filter(|e| {
            e.project_key.as_deref() == Some(key) || e.task_project_key.as_deref() == Some(key)
        })
        .collect();
    let instance: Vec<&EntryRecord> = entries
        .iter()
        .filter(|e| e.project_key.is_none() && e.task_key.is_none())
        .collect();
    let hints = hints(entries);
    html! {
        div id=(Region::ProjectEgress.id()) x-data="{ confirming: '' }" class="space-y-6" {
            div {
                h2 class="text-base font-semibold" { "Egress rules" }
                p class="mt-1 text-sm text-muted" {
                    "Hosts and cluster services this project's sandboxes may reach, in addition to the instance rules below. Rules save on their own."
                }
            }
            div class="space-y-4" {
                div class="flex flex-wrap items-center justify-between gap-3" {
                    h3 class="text-sm font-medium" {
                        "Project and task rules " span class="font-normal text-faint" { (own.len()) }
                    }
                    div class="flex flex-wrap gap-2" {
                        (open_modal(
                            &format!("/egress/allowlist/service/new{query}"),
                            "",
                            action_class(ButtonKind::Secondary),
                            html! { "Add cluster service" },
                        ))
                        (open_modal(
                            &format!("/egress/allowlist/host/new{query}"),
                            "",
                            action_class(ButtonKind::Secondary),
                            html! { "New egress rule" },
                        ))
                    }
                }
                @if own.is_empty() {
                    (empty_panel("No rules for this project yet", "", html! {}))
                } @else {
                    (own_rules(key, &own, &hints, &env))
                }
            }
            (instance_rules(&instance, projects))
        }
    }
}

/// The own rules: a table from 768 px, stacked cards below.
fn own_rules(
    key: &str,
    own: &[&EntryRecord],
    hints: &HashMap<Uuid, Hint>,
    env: &RowEnv<'_>,
) -> Markup {
    html! {
        div class="hidden md:block" {
            (data_table(
                rows::head(),
                html! {
                    @for entry in own {
                        @let hint = hints.get(&entry.id);
                        tr class="border-t border-line" x-show={"confirming !== '" (entry.id) "'"} {
                            (rows::cells(entry, env, hint_line(entry, hint, env, false)))
                        }
                        @if let Some(Hint::Elsewhere(others)) = hint {
                            tr class="border-t border-line bg-sunk" x-show={"confirming === '" (entry.id) "'"} x-cloak {
                                td colspan="5" class="px-5 py-3.5" { (confirmation(key, entry, others, env)) }
                            }
                        }
                    }
                },
            ))
        }
        ul class="space-y-3 md:hidden" {
            @for entry in own {
                @let hint = hints.get(&entry.id);
                li class="rounded-xl border border-line bg-panel" {
                    div x-show={"confirming !== '" (entry.id) "'"} class="flex items-start justify-between gap-2 py-3 pr-2 pl-4" {
                        div class="min-w-0 flex-1" {
                            (rows::target_cell(entry, env.projects, html! {
                                p class="mt-0.5 text-xs text-muted" {
                                    (effect_text(entry)) " · " (rows::ports_cell(entry)) " · " (scope_label(entry))
                                }
                                (hint_line(entry, hint, env, true))
                            }))
                        }
                        span class="flex shrink-0 items-center" { (rows::actions(entry, env)) }
                    }
                    @if let Some(Hint::Elsewhere(others)) = hint {
                        div x-show={"confirming === '" (entry.id) "'"} x-cloak class="rounded-xl bg-sunk px-4 py-3.5" {
                            (confirmation(key, entry, others, env))
                        }
                    }
                }
            }
        }
    }
}

fn effect_text(entry: &EntryRecord) -> &'static str {
    match entry.kind {
        EntryKind::HttpDeny { .. } => "Deny",
        EntryKind::HttpHost { .. } | EntryKind::ClusterService { .. } => "Allow",
    }
}

/// The hint line under the host: the text and its action, on one line, or with the action on its
/// own line (`stacked`, phones). Nothing for a rule without a hint.
fn hint_line(entry: &EntryRecord, hint: Option<&Hint>, env: &RowEnv<'_>, stacked: bool) -> Markup {
    let Some(hint) = hint else {
        return html! {};
    };
    let (text, action) = match hint {
        Hint::Elsewhere(others) => (
            format!("Also allowed for project {}", others.join(", ")),
            html! {
                button type="button" x-on:click={"confirming = '" (entry.id) "'"} class=(HINT_ACTION) { "Make global" }
            },
        ),
        Hint::Everywhere => (
            "Already allowed everywhere".to_string(),
            html! {
                button type="button" hx-delete={"/egress/allowlist/" (entry.id) (env.query)}
                    hx-target=(Region::ProjectEgress.selector()) hx-swap="outerHTML" class=(HINT_ACTION) { "Remove" }
            },
        ),
    };
    html! {
        @if stacked {
            p class="mt-1 text-xs text-muted" { (text) }
            div class="mt-1 text-xs" { (action) }
        } @else {
            p class="mt-1 text-xs text-muted" { (text) " · " span class="text-xs" { (action) } }
        }
    }
}

/// The confirmation that replaces a row (or a card): it names the projects whose rules the
/// instance rule replaces.
fn confirmation(key: &str, entry: &EntryRecord, others: &[String], env: &RowEnv<'_>) -> Markup {
    let EntryKind::HttpHost { host, port } = &entry.kind else {
        return html! {};
    };
    let mut affected = others.to_vec();
    affected.push(key.to_string());
    affected.sort();
    let values = serde_json::json!({
        "host": host,
        "port": port.to_string(),
        "return": format!("project:{key}"),
    });
    html! {
        div class="flex flex-wrap items-center justify-between gap-3" {
            div class="min-w-0 space-y-0.5" {
                p class="break-all font-mono text-[13px]" { (target(entry)) }
                p class="text-sm" {
                    "Make global? The rules of " (join_keys(&affected))
                    " are replaced by one instance rule; every project may then reach it."
                }
            }
            span class="flex shrink-0 gap-2" {
                button type="button" x-on:click="confirming = ''" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                button type="button" form=(ACTIONS_FORM) hx-post="/egress/allowlist/make-global" hx-vals=(values.to_string())
                    hx-target=(env.region) hx-swap="outerHTML" class=(action_class(ButtonKind::Primary)) { "Make global" }
            }
        }
    }
}

/// The collapsible, read-only section of the instance rules; every row links to the rule on the
/// rules page.
fn instance_rules(instance: &[&EntryRecord], projects: &ProjectHosts) -> Markup {
    html! {
        section id="project-egress-instance" x-data="{ open: false }" class="border-t border-line pt-5" {
            button type="button" aria-expanded="false" ":aria-expanded"="open" x-on:click="open = !open"
                class="flex min-h-11 w-full items-center gap-2 text-left lg:min-h-9" {
                span aria-hidden="true" class="text-muted transition-transform" ":class"="open ? '' : '-rotate-90'" { (icon(Icon::ChevronDown)) }
                h3 class="text-sm font-medium" {
                    "Also applies (instance) " span class="font-normal text-faint" { (instance.len()) }
                }
            }
            p class="mt-1 text-sm text-muted" {
                "Instance allow rules and deny rules apply to every project. Change them on the egress rules page."
            }
            div x-show="open" x-cloak class="mt-4" {
                @if instance.is_empty() {
                    (empty_panel("No instance rules", "", html! {}))
                } @else {
                    (data_table(
                        html! {
                            (th(Cell::First, html! { "Target" })) (th(Cell::Text, html! { "Effect" }))
                            (th(Cell::Mono, html! { "Port" }))
                            (th(Cell::Actions, html! { span class="sr-only" { "Open" } }))
                        },
                        html! {
                            @for entry in instance {
                                tr class="border-t border-line" {
                                    (td(Cell::First, html! {
                                        a href=(rule_link(entry)) class="block hover:underline" {
                                            (rows::target_cell(entry, projects, html! {}))
                                        }
                                    }))
                                    (td(Cell::Text, rows::effect_badge(entry)))
                                    (td(Cell::Mono, html! { (rows::ports_cell(entry)) }))
                                    (td(Cell::Actions, html! { span aria-hidden="true" class="flex justify-end text-faint" { (icon(Icon::ChevronRight)) } }))
                                }
                            }
                        },
                    ))
                }
            }
        }
    }
}

/// The rules page with `entry` highlighted.
fn rule_link(entry: &EntryRecord) -> String {
    format!("/egress/allowlist?highlight={}", target(entry))
}
