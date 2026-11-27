//! `/egress/allowlist`: instance, project and task entries of kind `http-host` and
//! `cluster-service` (task entries are only `http-host`) and deny rules (`http-deny`, instance
//! only), with search and pages. The dialogs to add and edit them are in [`dialogs`].

use axum::extract::{Query, State};
use axum::response::Response;
use df_core::host::{DEFAULT_HOST_PORT, is_dns_label};
use df_db::{EntryKind, EntryRecord};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::components::forms::INPUT_CLASS;
use crate::components::{
    ButtonKind, Icon, ProjectHosts, action_class, data_table, empty_panel, icon, link_button,
    page_header_with_subtitle,
};
use crate::config_pages::{create_button, list_page, open_modal};
use crate::htmx::Region;
use crate::layout::{Nav, Shell};
use crate::responses::internal_error;

pub(super) mod dialogs;
mod origin;
pub(super) mod paging;
pub(super) mod rows;

pub(super) use origin::Origin;
use paging::View;
use rows::{RowEnv, scope_label, target};

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

/// The fields of the dialogs of all rule kinds; each kind reads its own.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct RuleForm {
    effect: Effect,
    /// "Applies to": empty for the instance, a project key, or `task:<KEY>` for a task rule.
    project: String,
    host: String,
    port: String,
    namespace: String,
    service: String,
    ports: String,
    allow_dns: Option<String>,
    #[serde(flatten)]
    origin: Origin,
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

impl RuleForm {
    fn is_deny(&self) -> bool {
        self.effect == Effect::Deny
    }
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

/// Whether the target of `record` (`host:port`, `host:any` for a deny rule, `namespace/service`
/// for a cluster service) is the one `?highlight=` names.
fn is_highlighted(record: &EntryRecord, highlight: Option<&str>) -> bool {
    highlight == Some(target(record).as_str())
}

/// The title row, laid out like the other list pages: the actions stay at the right of the row.
fn header(count: usize) -> Markup {
    page_header_with_subtitle(
        "Egress rules",
        Some(count),
        Some("External hosts and cluster services that sandboxes may reach."),
        html! {
            div class="flex shrink-0 items-center gap-2" {
                (link_button(ButtonKind::Secondary, "/egress/log", "Live traffic"))
                (open_modal(
                    "/egress/allowlist/service/new",
                    "",
                    action_class(ButtonKind::Secondary),
                    html! { "Add cluster service" },
                ))
                (create_button("/egress/allowlist/host/new", "New egress rule"))
            }
        },
    )
}

/// The page's live region: the header, the search, the table of the page `view` selects and the
/// line below it. The highlighted rows are tinted and the first of them scrolled into view.
fn region(entries: &[EntryRecord], view: &View, projects: &ProjectHosts) -> Markup {
    let page = view.page(entries, |entry| is_highlighted(entry, view.highlight()));
    html! {
        div id=(Region::ConfigList.id()) {
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
    let query = view.query(page.number);
    let env = RowEnv {
        projects,
        query: &query,
        region: Region::ConfigList.selector(),
    };
    html! {
        div id="rules-results" {
            @if page.matched == 0 {
                (empty_panel(&format!("No rules match \"{}\"", view.q.trim()), "", html! {}))
            } @else {
                // Every column but Target has a fixed width so columns stay put across pages; the
                // `nth-child` indices follow the header order below.
                div class="[&_table]:table-fixed [&_th:nth-child(2)]:w-28 [&_th:nth-child(3)]:w-28 [&_th:nth-child(4)]:w-44 [&_th:nth-child(5)]:w-20" {
                    (data_table(
                        rows::head(),
                        html! {
                            @for entry in &page.rows {
                                @let marked = is_highlighted(entry, view.highlight());
                                @let scroll = marked && !std::mem::replace(&mut scrolled, true);
                                tr class={"border-t border-line" @if marked { " bg-accent-soft" }}
                                    aria-current=[marked.then_some("true")]
                                    x-data=[scroll.then_some("")]
                                    x-init=[scroll.then_some("$el.scrollIntoView({ block: 'center' })")] {
                                    (rows::cells(entry, &env, html! {}))
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

pub(super) async fn list(
    State(state): State<WebState>,
    shell: Shell,
    Query(view): Query<View>,
) -> Response {
    let entries = match df_db::list_entries(&state.pool).await {
        Ok(entries) => entries,
        Err(e) => return internal_error(&e),
    };
    let projects = super::project_hosts(&state).await;
    list_page(
        "Egress rules",
        Nav::Allowlist,
        &shell,
        region(&entries, &view, &projects),
    )
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
