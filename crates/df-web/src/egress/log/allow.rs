//! The "allow" action of the traffic page (`specs/egress-log`, "Allow from the log" and "Allow
//! from a graph node"): which blocked destinations offer it with which scopes (design D5), the
//! "Allow egress" dialog and the handler that creates the entry.

use std::collections::BTreeSet;

use axum::Extension;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use df_core::host::validate_exact_host;
use df_db::catalog::Actor;
use df_db::{DbError, EntryRecord, HostScope};
use df_proto::gateway::v1::FlowEvent;
use maud::{Markup, html};
use serde::{Deserialize, Deserializer};

use super::filter::is_blocked;
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{
    FormErrors, ICON_BUTTON_CLASS, Icon, field_error, form_error, icon, readonly_field_compact,
};
use crate::config_pages::{form_footer, modal, open_modal};
use crate::egress::allowlist::{ScopeName, validate_required_port};
use crate::responses::{internal_error, invalid};

/// The dialog (`GET`) and the entry it creates (`POST`).
pub(super) const ALLOW: &str = "/egress/log/allow";
/// The only denial reason the action is offered for: an IP literal or an unknown source cannot
/// be fixed with an `http-host` entry.
const NOT_ALLOWLISTED: &str = "not-allowlisted";

/// A blocked destination the dialog can allow, and the task and project scope it may offer
/// besides the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AllowTarget {
    pub(super) host: String,
    pub(super) port: u32,
    pub(super) task: Option<String>,
    pub(super) project: Option<String>,
}

impl AllowTarget {
    /// The target of a table row: a denied `not-allowlisted` event of a run, with its own task
    /// and project.
    pub(super) fn of_event(event: &FlowEvent) -> Option<Self> {
        Self::of_edges([event])
    }

    /// The target of a blocked graph node from the latest events of its edges (design D5): task
    /// scope only if all its denied `not-allowlisted` edges belong to one task, project scope
    /// only if they all belong to one project. `None` without such an edge.
    pub(super) fn of_edges<'a>(events: impl IntoIterator<Item = &'a FlowEvent>) -> Option<Self> {
        let denied: Vec<&FlowEvent> = events.into_iter().filter(|e| allowable(e)).collect();
        let first = denied.first()?;
        Some(Self {
            host: first.host.clone(),
            port: first.port,
            task: single(denied.iter().map(|e| e.task_key.as_str())),
            project: single(denied.iter().map(|e| e.project_key.as_str())),
        })
    }

    /// `host:port`, exactly as the event names it.
    pub(super) fn destination(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// The URL of the dialog.
    pub(super) fn href(&self) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("host", &self.host);
        query.append_pair("port", &self.port.to_string());
        if let Some(task) = &self.task {
            query.append_pair("task", task);
        }
        if let Some(project) = &self.project {
            query.append_pair("project", project);
        }
        format!("{ALLOW}?{}", query.finish())
    }

    /// A control that opens the dialog; `class` and `content` are the control's look. Its name
    /// includes the task, so rows of one host stay apart.
    pub(super) fn button(&self, class: &str, content: Markup) -> Markup {
        let label = match &self.task {
            Some(task) => format!("Allow {} for {task}", self.destination()),
            None => format!("Allow {}", self.destination()),
        };
        open_modal(&self.href(), &label, class, content)
    }
}

/// The task keys of the events, distinct and sorted, for "Blocked for DF-7 and DF-8".
pub(super) fn task_keys<'a>(events: impl IntoIterator<Item = &'a FlowEvent>) -> Vec<String> {
    events
        .into_iter()
        .filter(|e| allowable(e) && !e.task_key.is_empty())
        .map(|e| e.task_key.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// A scope the dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ScopeChoice {
    Task,
    Project,
    Instance,
}

impl ScopeChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Instance => "instance",
        }
    }
}

/// The dialog's fields; also the dialog's query, where `scope` is missing.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(in crate::egress) struct AllowForm {
    host: String,
    port: String,
    /// The task and project keys the dialog offers as scope.
    task: String,
    project: String,
    /// `None` if missing or not a scope; the form then asks for one.
    #[serde(deserialize_with = "scope_or_none")]
    scope: Option<ScopeChoice>,
}

/// `GET /egress/log/allow`: the dialog as a modal fragment.
pub(in crate::egress) async fn dialog(
    headers: HeaderMap,
    Query(form): Query<AllowForm>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/egress/log").into_response();
    }
    modal("Allow egress", allow_form(&form, &FormErrors::default()))
}

/// `POST /egress/log/allow`: creates the entry like the allowlist form, through
/// [`df_db::create_http_host_entry`], which also settles the pending `network-access` requests it
/// covers. The scope is checked here, not taken from the offered options.
pub(in crate::egress) async fn create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<AllowForm>,
) -> Response {
    let mut errors = FormErrors::default();
    let host = validate_exact_host(&form.host).map_err(|m| errors.add("destination", m));
    let port = validate_required_port(&form.port).map_err(|m| errors.add("destination", m));
    let scope = match resolve_scope(&state, &form).await {
        Ok(scope) => scope.map_err(|m| errors.add("scope", m)),
        Err(response) => return *response,
    };
    if let (Ok(host), Ok(port), Ok(scope)) = (host, port, scope) {
        match store(&state, &user, scope, &host, port).await {
            Ok(record) => return created(&host, port, &record),
            Err(DbError::Conflict(message)) => errors.add("", message),
            Err(DbError::NotFound(_)) => errors.add("scope", "The task or project is gone."),
            Err(e) => return internal_error(&e),
        }
    }
    invalid(allow_form(&form, &errors))
}

/// Whether the event offers the action: denied for `not-allowlisted`, from a run.
fn allowable(event: &FlowEvent) -> bool {
    is_blocked(event) && event.reason == NOT_ALLOWLISTED && !event.run_id.is_empty()
}

/// The value if all `values` are the same and not empty.
fn single<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let distinct: BTreeSet<&str> = values.collect();
    match distinct.into_iter().collect::<Vec<_>>().as_slice() {
        [only] if !only.is_empty() => Some((*only).to_string()),
        _ => None,
    }
}

/// A posted scope that is not one of [`ScopeChoice`] is no scope, not a rejected request.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature `deserialize_with` requires"
)]
fn scope_or_none<'de, D: Deserializer<'de>>(value: D) -> Result<Option<ScopeChoice>, D::Error> {
    Ok(ScopeChoice::deserialize(value).ok())
}

/// The scopes the dialog offers with their labels: the task and the project if the target
/// names one, and always the instance.
fn scope_options(form: &AllowForm) -> Vec<(ScopeChoice, String)> {
    let (task, project) = (form.task.trim(), form.project.trim());
    let mut options = Vec::new();
    if !task.is_empty() {
        options.push((ScopeChoice::Task, ScopeName::Task(task).label()));
    }
    if !project.is_empty() {
        options.push((ScopeChoice::Project, ScopeName::Project(project).label()));
    }
    options.push((ScopeChoice::Instance, ScopeName::Instance.label()));
    options
}

fn allow_form(form: &AllowForm, errors: &FormErrors) -> Markup {
    let options = scope_options(form);
    // The posted choice if it is offered, else the first option.
    let checked = options
        .iter()
        .map(|(choice, _)| *choice)
        .find(|choice| Some(*choice) == form.scope)
        .or_else(|| options.first().map(|(choice, _)| *choice));
    let destination = format!("{}:{}", form.host.trim(), form.port.trim());
    html! {
        form id="allow-form" class="mt-3 space-y-4 border-t border-line pt-4" hx-post=(ALLOW) hx-target="#allow-form" hx-swap="outerHTML" {
            p class="text-sm text-muted" { "Allows the destination for the chosen scope. The agent's next attempt goes through." }
            (form_error(errors))
            (readonly_field_compact("Destination", "destination", &destination, errors, None))
            input type="hidden" name="host" value=(form.host);
            input type="hidden" name="port" value=(form.port);
            input type="hidden" name="task" value=(form.task);
            input type="hidden" name="project" value=(form.project);
            fieldset class="space-y-1" {
                legend class="mb-1 block text-sm font-medium text-muted" { "Scope" }
                div class="flex flex-wrap gap-x-4 gap-y-1" {
                    @for (choice, label) in &options {
                        label class="flex min-h-11 items-center gap-2 text-sm lg:min-h-9" {
                            input type="radio" name="scope" value=(choice.as_str()) checked[Some(*choice) == checked];
                            (label)
                        }
                    }
                }
                (field_error(errors.get("scope")))
            }
            (form_footer("Create rule"))
        }
    }
}

/// The scope the form asks for; `Ok(Err)` names what is wrong with it.
async fn resolve_scope(
    state: &WebState,
    form: &AllowForm,
) -> Result<Result<HostScope, &'static str>, Box<Response>> {
    let lookup_failed = |e: DbError| Box::new(internal_error(&e));
    Ok(match form.scope {
        Some(ScopeChoice::Instance) => Ok(HostScope::Instance),
        Some(ScopeChoice::Task) => {
            df_db::task_views::get_task_detail_by_key(&state.pool, form.task.trim())
                .await
                .map_err(lookup_failed)?
                .map(|task| HostScope::Task(task.id))
                .ok_or("Unknown task.")
        }
        Some(ScopeChoice::Project) => {
            df_db::catalog::get_project_by_key(&state.pool, form.project.trim())
                .await
                .map_err(lookup_failed)?
                .map(|project| HostScope::Project(project.id))
                .ok_or("Unknown project.")
        }
        None => Err("Choose a scope."),
    })
}

async fn store(
    state: &WebState,
    user: &CurrentUser,
    scope: HostScope,
    host: &str,
    port: u16,
) -> Result<EntryRecord, DbError> {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let mut tx = state.pool.begin().await?;
    let record = df_db::create_http_host_entry(&mut tx, scope, host, port, actor, None).await?;
    tx.commit().await?;
    Ok(record)
}

/// The answer to a created entry: the dialog closes and the page's `#allow-toast` says so.
fn created(host: &str, port: u16, record: &EntryRecord) -> Response {
    let scope = ScopeName::of(record.project_key.as_deref(), record.task_key.as_deref()).phrase();
    let mut rule = url::form_urlencoded::Serializer::new(String::new());
    rule.append_pair("highlight", &format!("{host}:{port}"));
    let toast = html! {
        div x-data="{ open: true }" x-show="open" class="flex w-80 items-start gap-3 rounded-xl border border-line bg-panel p-4 text-sm shadow-lg" {
            span aria-hidden="true" class="mt-0.5 text-status-completed-fg" { (icon(Icon::Check)) }
            div class="min-w-0 flex-1" {
                p class="font-semibold" { "Rule created" }
                p class="mt-0.5 text-muted" { span class="font-mono" { (host) ":" (port) } " is allowed " (scope) "." }
                a href={"/egress/allowlist?" (rule.finish())} class="mt-1 inline-block text-accent-text hover:underline" { "View rule" }
            }
            button type="button" aria-label="Dismiss" "x-on:click"="open = false" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
        }
    };
    let mut response = toast.into_response();
    let headers = response.headers_mut();
    headers.insert("HX-Retarget", HeaderValue::from_static("#allow-toast"));
    headers.insert("HX-Reswap", HeaderValue::from_static("innerHTML"));
    headers.insert("HX-Trigger", HeaderValue::from_static("modal-close"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_proto::gateway::v1::Verdict;

    fn denied(task: &str, project: &str) -> FlowEvent {
        FlowEvent {
            run_id: format!("run-{task}"),
            task_key: task.into(),
            project_key: project.into(),
            host: "statsig.anthropic.com".into(),
            port: 443,
            verdict: Verdict::Denied.into(),
            reason: NOT_ALLOWLISTED.into(),
            ..Default::default()
        }
    }

    fn form(task: &str, project: &str) -> AllowForm {
        AllowForm {
            host: "statsig.anthropic.com".into(),
            port: "443".into(),
            task: task.into(),
            project: project.into(),
            scope: None,
        }
    }

    fn labels(form: &AllowForm) -> Vec<String> {
        scope_options(form).into_iter().map(|(_, l)| l).collect()
    }

    #[test]
    fn only_not_allowlisted_denials_of_a_run_offer_the_action() {
        let event = denied("DF-7", "DF");
        assert_eq!(
            AllowTarget::of_event(&event),
            Some(AllowTarget {
                host: "statsig.anthropic.com".into(),
                port: 443,
                task: Some("DF-7".into()),
                project: Some("DF".into()),
            })
        );
        let ip = FlowEvent {
            reason: "ip-literal".into(),
            ..denied("DF-7", "DF")
        };
        let unknown = FlowEvent {
            run_id: String::new(),
            ..denied("", "")
        };
        let allowed = FlowEvent {
            verdict: Verdict::Allowed.into(),
            ..denied("DF-7", "DF")
        };
        for event in [ip, unknown, allowed] {
            assert_eq!(AllowTarget::of_event(&event), None, "{event:?}");
        }
    }

    #[test]
    fn a_graph_node_of_one_task_offers_task_project_and_instance() {
        let edges = [denied("DF-7", "DF"), denied("DF-7", "DF")];
        let target = AllowTarget::of_edges(&edges).unwrap();
        assert_eq!(
            (target.task.as_deref(), target.project.as_deref()),
            (Some("DF-7"), Some("DF"))
        );
        assert_eq!(task_keys(&edges), ["DF-7"]);
    }

    #[test]
    fn a_graph_node_across_tasks_of_one_project_offers_no_task_scope() {
        let edges = [denied("DF-7", "DF"), denied("DF-8", "DF")];
        let target = AllowTarget::of_edges(&edges).unwrap();
        assert_eq!(
            (target.task.as_deref(), target.project.as_deref()),
            (None, Some("DF"))
        );
        assert_eq!(task_keys(&edges), ["DF-7", "DF-8"]);
    }

    #[test]
    fn a_graph_node_across_projects_offers_only_the_instance() {
        let edges = [denied("DF-7", "DF"), denied("OPS-1", "OPS")];
        let target = AllowTarget::of_edges(&edges).unwrap();
        assert_eq!((target.task, target.project), (None, None));
        // An edge without task key (an older gateway) rules out the task scope as well.
        let edges = [denied("DF-7", "DF"), denied("", "DF")];
        assert_eq!(AllowTarget::of_edges(&edges).unwrap().task, None);
    }

    #[test]
    fn the_dialog_offers_the_named_scopes_and_preselects_the_first() {
        assert_eq!(
            labels(&form("DF-7", "DF")),
            ["Task DF-7", "Project DF", "Everywhere"]
        );
        assert_eq!(labels(&form("", "DF")), ["Project DF", "Everywhere"]);
        assert_eq!(labels(&form("", "")), ["Everywhere"]);
        let html = allow_form(&form("", "DF"), &FormErrors::default()).into_string();
        assert!(
            html.contains("value=\"project\" checked") && !html.contains("value=\"task\""),
            "{html}"
        );
        assert!(html.contains("value=\"statsig.anthropic.com:443\" readonly"));
    }

    #[test]
    fn the_dialog_url_names_host_port_task_and_project() {
        let target = AllowTarget::of_event(&denied("DF-7", "DF")).unwrap();
        assert_eq!(
            target.href(),
            "/egress/log/allow?host=statsig.anthropic.com&port=443&task=DF-7&project=DF"
        );
        let target = AllowTarget::of_edges(&[denied("DF-7", "DF"), denied("OPS-1", "OPS")]);
        assert_eq!(
            target.unwrap().href(),
            "/egress/log/allow?host=statsig.anthropic.com&port=443"
        );
    }

    #[test]
    fn the_button_names_the_destination_and_the_task() {
        let row = AllowTarget::of_event(&denied("DF-7", "DF")).unwrap();
        assert!(
            row.button("", html! {})
                .into_string()
                .contains("aria-label=\"Allow statsig.anthropic.com:443 for DF-7\"")
        );
        let node = AllowTarget::of_edges(&[denied("DF-7", "DF"), denied("DF-8", "DF")]).unwrap();
        assert!(
            node.button("", html! {})
                .into_string()
                .contains("aria-label=\"Allow statsig.anthropic.com:443\"")
        );
    }

    #[test]
    fn the_dialog_has_a_divider_a_compact_destination_and_scopes_in_one_row() {
        let html = allow_form(&form("DF-7", "DF"), &FormErrors::default()).into_string();
        let form_tag = &html[..html.find('>').unwrap()];
        assert!(form_tag.contains("border-t border-line"), "{form_tag}");
        let at = html.find("name=\"destination\"").unwrap();
        let input = &html[html[..at].rfind("<input").unwrap()..];
        let input = &input[..input.find('>').unwrap()];
        assert!(
            input.contains("h-9")
                && input.contains("bg-sunk")
                && input.contains("font-mono text-[13px]")
                && !input.contains("border"),
            "{input}"
        );
        let radios = &html[html.find("</legend>").unwrap()..];
        assert!(
            radios.starts_with("</legend><div class=\"flex flex-wrap gap-x-4 gap-y-1\">"),
            "{radios}"
        );
    }
}
