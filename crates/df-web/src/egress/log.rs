//! `/egress/log`: the live view of every connection attempt through the egress proxy, with
//! filters and a summary of the hosts that were denied.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::convert::Infallible;

use axum::Extension;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chrono::DateTime;
use df_proto::gateway::v1::{FlowEvent, Phase, Verdict};
use futures::StreamExt;
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::INPUT_CLASS;
use crate::components::{
    ButtonKind, Cell, Tone, action_class, data_table, empty_state, page_header, td, th, tone_badge,
    tr, tr_with_id,
};
use crate::layout::{Nav, layout};

/// Events sent to the browser in one message while a backlog is being replayed.
const CHUNK: usize = 200;
/// Open rows a viewer keeps track of. A gateway pod keeps 10,000 events, so more are rare; past
/// the cap the set is cleared and the closes of those flows are shown as new rows.
pub(super) const MAX_OPEN_ROWS: usize = 10_000;

/// Filters of the log, as query parameters of the page and its event stream.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Filter {
    /// `1`: denied connections only.
    pub(super) denied: String,
    pub(super) project: String,
    pub(super) task: String,
    pub(super) persona: String,
    pub(super) host: String,
}

impl Filter {
    fn denied_only(&self) -> bool {
        !self.denied.is_empty()
    }

    /// `?key=value&...` of the set fields; empty if none is set.
    pub(super) fn query(&self) -> String {
        let fields = [
            ("denied", &self.denied),
            ("project", &self.project),
            ("task", &self.task),
            ("persona", &self.persona),
            ("host", &self.host),
        ];
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in fields {
            if !value.is_empty() {
                query.append_pair(name, value);
            }
        }
        let query = query.finish();
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }

    fn matches(&self, event: &FlowEvent) -> bool {
        let wanted = |want: &str, have: &str| want.is_empty() || want == have;
        (!self.denied_only() || event.verdict() == Verdict::Denied)
            && wanted(self.project.trim(), &event.project_key)
            && wanted(self.task.trim(), &event.task_key)
            && wanted(self.persona.trim(), &event.persona_id)
            && event.host.contains(self.host.trim())
    }
}

pub async fn page(State(state): State<WebState>, Query(filter): Query<Filter>) -> Response {
    if state.flows.is_none() {
        return crate::auth::error_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Egress log unavailable",
            "The gateway pods cannot be discovered: set `gateway.headlessService`.",
        );
    }
    layout("Egress log", Nav::EgressLog, page_body(&filter)).into_response()
}

fn page_body(filter: &Filter) -> Markup {
    let events = format!("/egress/log/events{}", filter.query());
    html! {
        (page_header("Egress log", None, html! {}))
        (filter_form(filter))
        div hx-sse:connect=(events) hx-swap="none" {}
        div class="mb-6" { (denied_hosts(&DeniedSummary::default())) }
        div id="flows" {
            (data_table(
                html! {
                    (th(Cell::Mono, html! { "Time (UTC)" })) (th(Cell::First, html! { "Task" }))
                    (th(Cell::Mono, html! { "Destination" })) (th(Cell::Text, html! { "Verdict" }))
                    (th(Cell::Number, html! { "Sent / received" }))
                    (th(Cell::Number, html! { "Duration" }))
                },
                html! {},
            ))
        }
    }
}

fn filter_form(filter: &Filter) -> Markup {
    let input = |label: &str, name: &str, value: &str| {
        html! {
            label class="block text-sm text-muted" {
                (label)
                input name=(name) value=(value) class={(INPUT_CLASS) " mt-1"};
            }
        }
    };
    html! {
        form method="get" action="/egress/log" class="mb-4 flex flex-wrap items-end gap-3" {
            label class="flex min-h-11 items-center gap-2 text-sm" {
                input type="checkbox" name="denied" value="1" checked[filter.denied_only()];
                "Denied only"
            }
            (input("Project", "project", &filter.project))
            (input("Task", "task", &filter.task))
            (input("Persona id", "persona", &filter.persona))
            (input("Host contains", "host", &filter.host))
            button type="submit" class=(action_class(ButtonKind::Secondary)) { "Apply" }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rows and the denied-hosts summary
// ---------------------------------------------------------------------------------------------

fn time_label(event: &FlowEvent) -> String {
    event
        .time
        .as_ref()
        .and_then(|t| DateTime::from_timestamp(t.seconds, u32::try_from(t.nanos).unwrap_or(0)))
        .map_or_else(String::new, |time| time.format("%H:%M:%S").to_string())
}

/// The element id of a flow's row: unique per gateway pod and flow. The pod name is hex encoded
/// so that any name makes a valid id.
fn row_id(event: &FlowEvent) -> String {
    format!("flow-{}-{}", hex::encode(&event.gateway_pod), event.flow_id)
}

fn row(event: &FlowEvent) -> Markup {
    if event.gap {
        return html! {
            tr class="border-t border-line" {
                td colspan="6" class="px-5 py-2 text-center text-sm text-faint" {
                    "Events are missing: the page could not keep up."
                }
            }
        };
    }
    let who = if event.task_key.is_empty() {
        format!("unknown source {}", event.source_ip)
    } else {
        event.task_key.clone()
    };
    let (tone, label) = match (event.verdict(), event.phase()) {
        (Verdict::Denied, _) => (Tone::Failed, "denied"),
        // Still open: the row is replaced when the closing event arrives.
        (_, Phase::Opened) => (Tone::Running, "open"),
        _ => (Tone::Completed, "allowed"),
    };
    let id = (event.flow_id != 0).then(|| row_id(event));
    tr_with_id(
        id.as_deref(),
        html! {
            (td(Cell::Mono, html! { (time_label(event)) }))
            (td(Cell::First, html! { (who) }))
            (td(Cell::Mono, html! { (event.method) " " (event.host) ":" (event.port) }))
            (td(Cell::Text, html! {
                (tone_badge(tone, label))
                @if !event.reason.is_empty() { " " span class="text-xs text-faint" { (event.reason) } }
            }))
            (td(Cell::Number, html! { span class="font-mono text-[13px] text-muted" { (event.bytes_out) " / " (event.bytes_in) } }))
            (td(Cell::Number, html! { span class="font-mono text-[13px] text-muted" { (duration_label(event)) } }))
        },
    )
}

/// How long the connection lasted: `12 ms`, `3.4 s`, `2:05`. Empty while it is open.
fn duration_label(event: &FlowEvent) -> String {
    let Some(duration) = event
        .duration
        .as_ref()
        .filter(|_| event.phase() == Phase::Closed)
    else {
        return String::new();
    };
    // gRPC values are not trusted to be sane: clamp instead of overflowing or showing a minus.
    let millis = duration
        .seconds
        .saturating_mul(1000)
        .saturating_add(i64::from(duration.nanos) / 1_000_000)
        .max(0);
    match millis {
        ..1000 => format!("{millis} ms"),
        1000..60_000 => format!("{:.1} s", millis as f64 / 1000.0),
        _ => format!("{}:{:02}", millis / 60_000, millis / 1000 % 60),
    }
}

/// Hosts that were denied since the page was opened.
#[derive(Debug, Default)]
pub(super) struct DeniedSummary {
    hosts: BTreeMap<(String, u32), DeniedHost>,
}

#[derive(Debug, Default)]
struct DeniedHost {
    count: u64,
    last_seen: String,
    projects: BTreeSet<String>,
}

impl DeniedSummary {
    fn add(&mut self, event: &FlowEvent) {
        let host = self
            .hosts
            .entry((event.host.clone(), event.port))
            .or_default();
        host.count += 1;
        host.last_seen = time_label(event);
        if !event.project_key.is_empty() {
            host.projects.insert(event.project_key.clone());
        }
    }
}

fn denied_hosts(summary: &DeniedSummary) -> Markup {
    let mut hosts: Vec<_> = summary.hosts.iter().collect();
    hosts.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    html! {
        div id="denied-hosts" {
            h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-faint" { "Denied hosts" }
            @if hosts.is_empty() {
                (empty_state("No denied connections yet."))
            } @else {
                (data_table(
                    html! {
                        (th(Cell::First, html! { "Host" })) (th(Cell::Mono, html! { "Port" }))
                        (th(Cell::Number, html! { "Count" })) (th(Cell::Mono, html! { "Last seen (UTC)" }))
                        (th(Cell::Text, html! { "Projects" }))
                    },
                    html! {
                        @for ((host, port), entry) in hosts {
                            (tr(html! {
                                (td(Cell::First, html! { (host) }))
                                (td(Cell::Mono, html! { (port) }))
                                (td(Cell::Number, html! { (entry.count) }))
                                (td(Cell::Mono, html! { (entry.last_seen) }))
                                (td(Cell::Text, html! { (entry.projects.iter().cloned().collect::<Vec<_>>().join(", ")) }))
                            }))
                        }
                    },
                ))
            }
        }
    }
}

fn partial(target: &str, swap: &str, inner: &Markup) -> Markup {
    html! { hx-partial hx-target=(target) hx-swap=(swap) { (inner) } }
}

/// What one viewer connection has been sent: the filter, the denied hosts so far and the rows
/// that are still shown as open (their closing event replaces them).
pub(super) struct Viewer {
    filter: Filter,
    summary: DeniedSummary,
    open: HashSet<String>,
}

impl Viewer {
    pub(super) fn new(filter: Filter) -> Self {
        Self {
            filter,
            summary: DeniedSummary::default(),
            open: HashSet::new(),
        }
    }

    fn shown<'a>(&self, events: &'a [FlowEvent]) -> Vec<&'a FlowEvent> {
        events
            .iter()
            .filter(|event| !event.backlog_end && (event.gap || self.filter.matches(event)))
            .collect()
    }

    fn count_denied(&mut self, shown: &[&FlowEvent]) -> bool {
        let mut denied = false;
        for event in shown {
            if !event.gap && event.verdict() == Verdict::Denied {
                self.summary.add(event);
                denied = true;
            }
        }
        denied
    }

    fn summary_partial(&self) -> String {
        partial("#denied-hosts", "outerMorph", &denied_hosts(&self.summary)).into_string()
    }

    /// The first message of a connection: the backlog (oldest first) replaces the table body, one
    /// row per flow, and the summary is sent whatever the backlog holds, so that what an earlier
    /// connection of the page showed is cleared.
    pub(super) fn backlog_message(&mut self, backlog: &[FlowEvent]) -> String {
        let shown = self.shown(backlog);
        self.count_denied(&shown);
        // A closing event supersedes the opening event of its flow, in place.
        let mut rows: Vec<&FlowEvent> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for event in shown {
            let tracked = (event.flow_id != 0).then(|| row_id(event));
            match tracked.as_ref().and_then(|id| at.get(id)) {
                Some(&index) if event.phase() == Phase::Closed => rows[index] = event,
                _ => {
                    if let Some(id) = tracked {
                        at.insert(id, rows.len());
                    }
                    rows.push(event);
                }
            }
        }
        self.open = rows
            .iter()
            .filter(|event| event.phase() == Phase::Opened && event.flow_id != 0)
            .map(|event| row_id(event))
            .collect();
        let rows = html! { @for event in rows.iter().rev() { (row(event)) } };
        let mut body = partial("#flows tbody", "innerHTML", &rows).into_string();
        body.push_str(&self.summary_partial());
        body
    }

    /// A message for events that arrived after the backlog (oldest first): new rows on top, the
    /// rows of flows that closed replaced in place, and the summary if something was denied.
    /// `None` if nothing passes the filter.
    pub(super) fn live_message(&mut self, batch: &[FlowEvent]) -> Option<String> {
        let shown = self.shown(batch);
        if shown.is_empty() {
            return None;
        }
        let denied = self.count_denied(&shown);
        let mut new_rows: Vec<&FlowEvent> = Vec::new();
        let mut in_batch: HashMap<String, usize> = HashMap::new();
        let mut replacements = String::new();
        for event in shown {
            let id = (event.flow_id != 0).then(|| row_id(event));
            match (event.phase(), id) {
                (Phase::Closed, Some(id)) => {
                    if let Some(slot) = in_batch.get(&id).and_then(|&at| new_rows.get_mut(at)) {
                        // Opened and closed in one batch: one closed row.
                        *slot = event;
                        self.open.remove(&id);
                    } else if self.open.remove(&id) {
                        replacements.push_str(
                            &partial(&format!("#{id}"), "outerMorph", &row(event)).into_string(),
                        );
                    } else {
                        new_rows.push(event);
                    }
                }
                (Phase::Opened, Some(id)) => {
                    if self.open.len() >= MAX_OPEN_ROWS {
                        self.open.clear();
                    }
                    self.open.insert(id.clone());
                    in_batch.insert(id, new_rows.len());
                    new_rows.push(event);
                }
                _ => new_rows.push(event),
            }
        }
        let rows = html! { @for event in new_rows.iter().rev() { (row(event)) } };
        let mut body = String::new();
        if !new_rows.is_empty() {
            body.push_str(&partial("#flows tbody", "afterbegin", &rows).into_string());
        }
        body.push_str(&replacements);
        if denied {
            body.push_str(&self.summary_partial());
        }
        Some(body)
    }
}

/// `GET /egress/log/events`: the viewer's rows and summary as `<hx-partial>` messages.
pub async fn events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(filter): Query<Filter>,
) -> Response {
    let Some(flows) = &state.flows else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let mut source = flows.subscribe();
    // The source sends the backlog, a `backlog_end` marker, then live events. The backlog is the
    // first message and replaces the table body: after a reconnect the page already shows rows.
    let stream = futures::stream::once(async move {
        let mut backlog = Vec::new();
        while let Some(event) = source.next().await {
            if event.backlog_end {
                break;
            }
            backlog.push(event);
        }
        let mut viewer = Viewer::new(filter);
        let first = viewer.backlog_message(&backlog);
        let live = source.ready_chunks(CHUNK).filter_map(move |batch| {
            let body = viewer.live_message(&batch);
            async move { body.map(|body| Ok::<_, Infallible>(Event::default().data(body))) }
        });
        futures::stream::once(std::future::ready(Ok::<_, Infallible>(
            Event::default().data(first),
        )))
        .chain(live)
    })
    .flatten();
    // Ends when the session's token is due for refresh, like the other streams: the reconnect
    // passes the auth middleware again.
    Sse::new(stream.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}
