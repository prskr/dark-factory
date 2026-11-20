//! One rule as the cells of a table row, shared by the rules page and the Egress tab of a project:
//! target, effect, port, scope and the edit and delete buttons.

use df_db::{EntryKind, EntryRecord};
use maud::{Markup, html};

use super::{ANY_PORT, ScopeName};
use crate::components::{
    Cell, HostIconSize, ICON_BUTTON_CLASS, Icon, ProjectHosts, Tone, host_icon, icon, td, th,
    tone_badge,
};
use crate::config_pages::edit_button;

/// What the cells of a region need.
pub(in crate::egress) struct RowEnv<'a> {
    pub(in crate::egress) projects: &'a ProjectHosts,
    /// The query string of the context, appended to the URLs of the edit dialog and of the delete:
    /// `?q=…&page=…` on the rules page, `?return=project:DF` on a project tab.
    pub(in crate::egress) query: &'a str,
    /// The selector of the region that a delete replaces.
    pub(in crate::egress) region: &'static str,
}

/// The scope of an entry by its keys, as the rules page and the dialogs name it.
pub(in crate::egress) fn scope_label(record: &EntryRecord) -> String {
    ScopeName::of(record.project_key.as_deref(), record.task_key.as_deref()).label()
}

/// `host:port` of an `http-host` entry, `host:any` of a deny rule, `namespace/service` of a
/// cluster service: the target as named in accessible labels and matched by `?highlight=`.
pub(in crate::egress) fn target(record: &EntryRecord) -> String {
    match &record.kind {
        EntryKind::HttpHost { host, port } => format!("{host}:{port}"),
        EntryKind::HttpDeny { host } => format!("{host}:{ANY_PORT}"),
        EntryKind::ClusterService {
            namespace, service, ..
        } => format!("{namespace}/{service}"),
    }
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

pub(in crate::egress) fn ports_cell(record: &EntryRecord) -> String {
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

pub(in crate::egress) fn effect_badge(record: &EntryRecord) -> Markup {
    match record.kind {
        EntryKind::HttpDeny { .. } => tone_badge(Tone::Failed, "Deny"),
        EntryKind::HttpHost { .. } | EntryKind::ClusterService { .. } => {
            tone_badge(Tone::Completed, "Allow")
        }
    }
}

/// The host (with its icon) or the cluster service of a rule; `below` goes on a line under it.
pub(in crate::egress) fn target_cell(
    record: &EntryRecord,
    projects: &ProjectHosts,
    below: Markup,
) -> Markup {
    let line = match &record.kind {
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
    };
    html! {
        (line)
        // Under the host icon's text, not under the icon.
        div class="ml-9 empty:hidden" { (below) }
    }
}

/// The edit and the delete button of a rule, named after its target.
pub(in crate::egress) fn actions(record: &EntryRecord, env: &RowEnv<'_>) -> Markup {
    let target = target(record);
    html! {
        (edit_button(
            &format!("/egress/allowlist/{}/edit{}", record.id, env.query),
            &format!("Edit egress rule {target}"),
        ))
        button type="button" hx-delete={"/egress/allowlist/" (record.id) (env.query)}
            hx-target=(env.region) hx-swap="outerHTML"
            aria-label={"Delete egress rule " (target)} title="Delete" class=(ICON_BUTTON_CLASS) {
            (icon(Icon::Trash))
        }
    }
}

/// The header cells that go with [`cells`].
pub(in crate::egress) fn head() -> Markup {
    html! {
        (th(Cell::First, html! { "Target" })) (th(Cell::Text, html! { "Effect" }))
        (th(Cell::Mono, html! { "Port" })) (th(Cell::Text, html! { "Scope" }))
        (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
    }
}

/// The five cells of a rule's row: target, effect, port, scope, actions.
pub(in crate::egress) fn cells(record: &EntryRecord, env: &RowEnv<'_>, below: Markup) -> Markup {
    html! {
        (td(Cell::First, target_cell(record, env.projects, below)))
        (td(Cell::Text, effect_badge(record)))
        (td(Cell::Mono, html! { (ports_cell(record)) }))
        (td(Cell::Text, html! { (scope_label(record)) }))
        (td(Cell::Actions, html! { span class="flex items-center justify-end" { (actions(record, env)) } }))
    }
}
