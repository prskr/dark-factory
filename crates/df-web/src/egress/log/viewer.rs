//! What one viewer connection has been sent, and the messages that bring it up to date.
//!
//! The stream is unfiltered. The viewer sees every event: all of them feed the graph and the
//! counts, the ones that match the filter also become rows.

use std::collections::{HashMap, VecDeque};

use df_proto::gateway::v1::{FlowEvent, Phase};
use maud::{Markup, html};

use super::filter::Filter;
use super::graph::{self, Edges};
use super::rows::{row, row_id};
use crate::components::{ButtonKind, action_class};

/// Flows a viewer keeps track of (open ones and their edges). A gateway pod keeps 10,000
/// events, so more are rare; past the cap the set is cleared and the closes of those flows are
/// shown as new rows.
pub(in crate::egress) const MAX_OPEN_ROWS: usize = 10_000;
/// Edges of the graph a viewer keeps; past the cap the map starts over.
const MAX_EDGES: usize = 10_000;

/// Flows seen and shown, for the segments and "N of M". Gaps are not flows.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Counts {
    /// Every flow seen.
    pub(super) seen: usize,
    /// Flows that match everything but the verdict (the *All* segment).
    pub(super) all: usize,
    pub(super) blocked: usize,
    pub(super) allowed: usize,
    /// Flows that match the whole filter: the rows.
    pub(super) shown: usize,
}

impl Counts {
    fn add(&mut self, event: &FlowEvent, filter: &Filter) {
        self.seen += 1;
        if filter.matches_except_verdict(event) {
            self.all += 1;
            if super::filter::is_blocked(event) {
                self.blocked += 1;
            } else {
                self.allowed += 1;
            }
            if filter.matches(event) {
                self.shown += 1;
            }
        }
    }

    pub(super) fn emptiness(&self) -> Emptiness {
        if self.seen == 0 {
            Emptiness::NoTraffic
        } else if self.shown == 0 {
            Emptiness::NoMatch
        } else {
            Emptiness::Rows
        }
    }
}

/// Which empty state the table shows.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Emptiness {
    Rows,
    /// No event seen at all.
    NoTraffic,
    /// Events seen, none matches.
    NoMatch,
}

pub(in crate::egress) struct Viewer {
    filter: Filter,
    /// Flows whose opening event was seen and whose closing event is not yet: whether the table
    /// has a row for them.
    open: HashMap<String, bool>,
    /// The rows the page has, newest first, by row id (`None` for a gap row). The page drops
    /// rows past the row limit from the bottom, and so does this list.
    table: VecDeque<Option<String>>,
    edges: Edges,
    counts: Counts,
    sent_graph: Option<String>,
    sent_counts: Option<Counts>,
}

impl Viewer {
    pub(in crate::egress) fn new(filter: Filter) -> Self {
        Self {
            filter,
            open: HashMap::new(),
            table: VecDeque::new(),
            edges: Edges::default(),
            counts: Counts::default(),
            sent_graph: None,
            sent_counts: None,
        }
    }

    /// Takes `event` into the graph and the counts and says what it does to the rows.
    fn apply(&mut self, event: &FlowEvent) -> RowAction {
        if event.backlog_end {
            return RowAction::None;
        }
        if event.gap {
            // A gap has its own row whatever the filter says and is neither a flow nor an edge.
            return RowAction::Insert;
        }
        if self.edges.len() >= MAX_EDGES {
            self.edges.clear();
        }
        self.edges.update(event);
        let matching = self.filter.matches(event);
        let id = (event.flow_id != 0).then(|| row_id(event));
        let had_row = match (event.phase(), id) {
            (Phase::Closed, Some(id)) => self.open.remove(&id),
            (Phase::Opened, Some(id)) => {
                if self.open.len() >= MAX_OPEN_ROWS {
                    self.open.clear();
                }
                self.open.insert(id, matching);
                None
            }
            _ => None,
        };
        match had_row {
            // The opening event was counted already.
            Some(true) if matching => RowAction::Replace,
            Some(_) => {
                if matching {
                    RowAction::Insert
                } else {
                    RowAction::None
                }
            }
            None => {
                self.counts.add(event, &self.filter);
                if matching {
                    RowAction::Insert
                } else {
                    RowAction::None
                }
            }
        }
    }

    /// Applies `events` (oldest first) and collects what happens to the rows. A closing event
    /// supersedes the opening event of its flow: in place if both are in `events`, otherwise as
    /// a replacement of the row the page has.
    fn collect_rows<'a>(&mut self, events: &'a [FlowEvent]) -> RowBatch<'a> {
        let mut batch = RowBatch::default();
        for event in events {
            let id = (event.flow_id != 0).then(|| row_id(event));
            match (self.apply(event), id) {
                (RowAction::None, _) => {}
                (RowAction::Replace, Some(id)) => match batch.at.get(&id) {
                    Some(&index) => {
                        if let Some(slot) = batch.rows.get_mut(index) {
                            *slot = event;
                        }
                    }
                    None => batch.replacements.push_str(
                        &partial(&format!("#{id}"), "outerMorph", &row(event)).into_string(),
                    ),
                },
                (_, id) => {
                    if let Some(id) = id {
                        batch.at.insert(id, batch.rows.len());
                    }
                    batch.rows.push(event);
                }
            }
        }
        batch
    }

    /// The first message of a connection: the backlog (oldest first) replaces the table body, one
    /// row per flow, and graph and counts are sent whatever the backlog holds, so that what an
    /// earlier connection of the page showed is cleared.
    pub(in crate::egress) fn backlog_message(&mut self, backlog: &[FlowEvent]) -> String {
        // The page has no rows of an earlier connection to replace: only `rows` is used.
        let batch = self.collect_rows(backlog);
        // Only the newest rows up to the limit; the counts and the graph still cover all.
        let rows: Vec<&FlowEvent> = batch.rows.into_iter().rev().collect();
        let (kept, dropped) = rows.split_at(rows.len().min(self.filter.row_limit()));
        self.table = kept.iter().map(|event| row_key(event)).collect();
        for event in dropped {
            self.forget_row(row_key(event));
        }
        let rows = html! { @for event in kept { (row(event)) } };
        let mut body = partial("#flows tbody", "innerHTML", &rows).into_string();
        self.sent_graph = None;
        self.sent_counts = None;
        body.push_str(&self.state_partials());
        body
    }

    /// A message for events that arrived after the backlog (oldest first): new rows on top, the
    /// rows of flows that closed replaced in place, and graph and counts if they changed. `None`
    /// if nothing changed.
    pub(in crate::egress) fn live_message(&mut self, batch: &[FlowEvent]) -> Option<String> {
        let RowBatch {
            rows, replacements, ..
        } = self.collect_rows(batch);
        let mut body = String::new();
        if !rows.is_empty() {
            for event in &rows {
                self.table.push_front(row_key(event));
            }
            while self.table.len() > self.filter.row_limit() {
                let trimmed = self.table.pop_back().flatten();
                self.forget_row(trimmed);
            }
            let rows = html! { @for event in rows.iter().rev() { (row(event)) } };
            body.push_str(&partial("#flows tbody", "afterbegin", &rows).into_string());
        }
        body.push_str(&replacements);
        body.push_str(&self.state_partials());
        (!body.is_empty()).then_some(body)
    }

    /// The page has no row for `id` any more: a later close of that flow becomes a new row.
    fn forget_row(&mut self, id: Option<String>) {
        if let Some(has_row) = id.and_then(|id| self.open.get_mut(&id)) {
            *has_row = false;
        }
    }

    /// The graph and the counts, each only if it differs from what the page has.
    fn state_partials(&mut self) -> String {
        let mut body = String::new();
        let graph = graph::render(&graph::model(&self.edges, &self.filter)).into_string();
        if self.sent_graph.as_ref() != Some(&graph) {
            body.push_str(&partial_str("#graph", "innerHTML", &graph));
            self.sent_graph = Some(graph);
        }
        if self.sent_counts.as_ref() != Some(&self.counts) {
            body.push_str(&counts_partials(&self.counts, &self.filter));
            self.sent_counts = Some(self.counts.clone());
        }
        body
    }
}

/// The empty state under the table (`#flows-empty`).
pub(super) fn empty_state(counts: &Counts, filter: &Filter) -> Markup {
    html! {
        @match counts.emptiness() {
            Emptiness::Rows => {}
            Emptiness::NoTraffic => {
                div class="px-6 py-12 text-center" {
                    p class="text-[15px] font-semibold" { "No network traffic yet" }
                    p class="mt-1 text-sm text-muted" { "Nothing has tried to reach the network yet." }
                }
            }
            Emptiness::NoMatch => {
                div class="px-6 py-12 text-center" {
                    p class="text-[15px] font-semibold" { "No events match these filters" }
                    @if !filter.is_empty() {
                        button type="button" "x-on:click"="clearFilters(document.getElementById('filters'))"
                            class={(action_class(ButtonKind::Secondary)) " mt-3"} { "Clear filters" }
                    }
                }
            }
        }
    }
}

pub(super) fn event_count(counts: &Counts) -> Markup {
    html! { (counts.shown) " of " (counts.seen) }
}

/// What an event does to the table.
#[derive(Debug, PartialEq, Eq)]
enum RowAction {
    None,
    /// A new row on top.
    Insert,
    /// Replaces the row of the flow's opening event.
    Replace,
}

/// The row changes of a run of events.
#[derive(Default)]
struct RowBatch<'a> {
    /// New rows, oldest first.
    rows: Vec<&'a FlowEvent>,
    /// Index in `rows` by row id.
    at: HashMap<String, usize>,
    /// `<hx-partial>`s that replace rows the page already has.
    replacements: String,
}

/// The text of the segments and of "N of M", as partials into their spans.
fn counts_partials(counts: &Counts, filter: &Filter) -> String {
    let mut body = String::new();
    for (target, number) in [
        ("#count-all", counts.all),
        ("#count-blocked", counts.blocked),
        ("#count-allowed", counts.allowed),
    ] {
        body.push_str(&partial(target, "innerHTML", &html! { (number) }).into_string());
    }
    body.push_str(&partial("#event-count", "innerHTML", &event_count(counts)).into_string());
    body.push_str(
        &partial("#flows-empty", "innerHTML", &empty_state(counts, filter)).into_string(),
    );
    body
}

/// The id of an event's row; gaps and events without a flow id have none.
fn row_key(event: &FlowEvent) -> Option<String> {
    (event.flow_id != 0).then(|| row_id(event))
}

fn partial(target: &str, swap: &str, inner: &Markup) -> Markup {
    html! { hx-partial hx-target=(target) hx-swap=(swap) { (inner) } }
}

fn partial_str(target: &str, swap: &str, inner: &str) -> String {
    format!("<hx-partial hx-target=\"{target}\" hx-swap=\"{swap}\">{inner}</hx-partial>")
}
