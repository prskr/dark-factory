//! The connection graph: sandboxes on the left, external destinations on the right, one edge per
//! sandbox and destination. The viewer keeps the latest event per edge ([`Edges`]); [`model`]
//! turns that into nodes (capped), [`layout`] positions them (pure) and [`render`] draws them.

use std::collections::HashMap;

use df_proto::gateway::v1::FlowEvent;
use maud::{Markup, html};

use super::filter::{Filter, is_blocked};
use super::rows::{sandbox_key, sandbox_lines};
use crate::components::{HostIconSize, Icon, ProjectHosts, host_icon, icon};

/// Sandboxes drawn at most, the most recently seen.
pub(super) const MAX_SANDBOXES: usize = 12;
/// Destinations drawn at most, the most recently seen.
pub(super) const MAX_DESTINATIONS: usize = 30;

pub(super) const SANDBOX_X: u32 = 20;
pub(super) const SANDBOX_W: u32 = 176;
const SANDBOX_H: u32 = 52;
const SANDBOX_PITCH: u32 = 160;
const DEST_H: u32 = 48;
const DEST_PITCH: u32 = 70;
/// Space above the first node for the column headings.
const TOP: u32 = 60;
const BOTTOM: u32 = 24;
const RIGHT_PAD: u32 = 20;
const MIN_GUTTER: u32 = 60;
/// Space between the sandbox group box and the nodes inside it.
const GROUP_PAD: u32 = 12;
/// Top of the sandbox group box above the column heading; also the heading's offset in the box.
const GROUP_TOP: u32 = 8;
/// Classes of a column heading.
const HEADING: &str = "text-xs font-medium uppercase tracking-wide text-faint";
/// Narrowest destination node; the graph minimum is built on it.
pub(super) const MIN_DEST_W: u32 = 120;
const MAX_DEST_W: u32 = 260;
/// Everything in a graph row but the destination column: the sandbox column, the minimum gutter
/// and the right padding.
pub(super) const DEST_OFFSET: u32 = SANDBOX_X + SANDBOX_W + MIN_GUTTER + RIGHT_PAD;
/// Narrowest graph that shows its nodes untruncated: 20 + 176 + 60 + 120 + 20.
pub(super) const MIN_WIDTH: u32 = DEST_OFFSET + MIN_DEST_W;
pub(super) const MAX_WIDTH: u32 = 960;
/// Width the server lays out for; the browser stretches the gutter to the real width.
pub(super) const DEFAULT_WIDTH: u32 = 520;

/// Classes of a node card (the border colour is the node's): large radius, a light shadow and
/// the `raised` fill, so that it reads as lifted off the panel in both themes.
const CARD: &str = "rounded-xl border bg-raised shadow-sm";

/// A sandbox, a destination and the latest event that connected them.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
struct EdgeKey {
    sandbox: String,
    host: String,
    port: u32,
}

#[derive(Debug)]
struct Edge {
    latest: FlowEvent,
    first: u64,
    last: u64,
}

/// The latest event per edge, from every event the viewer saw (not only those the filter lets
/// through: the graph dims the others).
#[derive(Debug, Default)]
pub(super) struct Edges {
    map: HashMap<EdgeKey, Edge>,
    seq: u64,
}

impl Edges {
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    pub(super) fn clear(&mut self) {
        self.map.clear();
    }

    /// Records `event` as the latest of its edge; a closing event replaces the opening one.
    pub(super) fn update(&mut self, event: &FlowEvent) {
        self.seq += 1;
        let key = EdgeKey {
            sandbox: sandbox_key(event).to_string(),
            host: event.host.clone(),
            port: event.port,
        };
        let seq = self.seq;
        let edge = self.map.entry(key).or_insert_with(|| Edge {
            latest: event.clone(),
            first: seq,
            last: seq,
        });
        edge.latest = event.clone();
        edge.last = seq;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SandboxNode {
    pub(super) label: String,
    pub(super) task: String,
    pub(super) dimmed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DestinationNode {
    pub(super) host: String,
    pub(super) port: u32,
    pub(super) blocked: bool,
    pub(super) dimmed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EdgeLine {
    pub(super) sandbox: usize,
    pub(super) destination: usize,
    pub(super) blocked: bool,
    pub(super) dimmed: bool,
}

/// What the graph shows, before positions.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Model {
    pub(super) sandboxes: Vec<SandboxNode>,
    pub(super) destinations: Vec<DestinationNode>,
    pub(super) edges: Vec<EdgeLine>,
    /// Seen but not drawn because of the caps.
    pub(super) hidden_sandboxes: usize,
    pub(super) hidden_destinations: usize,
}

/// The `limit` most recently seen of `(key, first, last)`, in the order they first appeared.
fn newest<K: Clone + Eq + std::hash::Hash>(seen: &HashMap<K, (u64, u64)>, limit: usize) -> Vec<K> {
    let mut all: Vec<(&K, &(u64, u64))> = seen.iter().collect();
    all.sort_by_key(|(_, (_, last))| std::cmp::Reverse(*last));
    all.truncate(limit);
    all.sort_by_key(|(_, (first, _))| *first);
    all.into_iter().map(|(key, _)| key.clone()).collect()
}

/// Builds the model: at most [`MAX_SANDBOXES`] and [`MAX_DESTINATIONS`] nodes, a node is dimmed
/// when none of its edges matches `filter`, a destination is blocked when any edge to it is.
pub(super) fn model(edges: &Edges, filter: &Filter) -> Model {
    let mut sandboxes: HashMap<String, (u64, u64)> = HashMap::new();
    let mut destinations: HashMap<(String, u32), (u64, u64)> = HashMap::new();
    for (key, edge) in &edges.map {
        let bump = |(first, last): &mut (u64, u64)| {
            *first = (*first).min(edge.first);
            *last = (*last).max(edge.last);
        };
        bump(
            sandboxes
                .entry(key.sandbox.clone())
                .or_insert((u64::MAX, 0)),
        );
        bump(
            destinations
                .entry((key.host.clone(), key.port))
                .or_insert((u64::MAX, 0)),
        );
    }
    let shown_sandboxes = newest(&sandboxes, MAX_SANDBOXES);
    let shown_destinations = newest(&destinations, MAX_DESTINATIONS);
    let sandbox_at: HashMap<&String, usize> = shown_sandboxes
        .iter()
        .enumerate()
        .map(|(i, k)| (k, i))
        .collect();
    let destination_at: HashMap<&(String, u32), usize> = shown_destinations
        .iter()
        .enumerate()
        .map(|(i, k)| (k, i))
        .collect();

    let mut model = Model {
        hidden_sandboxes: sandboxes.len() - shown_sandboxes.len(),
        hidden_destinations: destinations.len() - shown_destinations.len(),
        ..Model::default()
    };
    // A sandbox node with the sequence number of the event whose task it shows.
    let mut sandbox_nodes: Vec<(u64, SandboxNode)> = shown_sandboxes
        .iter()
        .map(|_| {
            (
                0,
                SandboxNode {
                    label: String::new(),
                    task: String::new(),
                    dimmed: true,
                },
            )
        })
        .collect();
    let mut destination_nodes: Vec<DestinationNode> = shown_destinations
        .iter()
        .map(|(host, port)| DestinationNode {
            host: host.clone(),
            port: *port,
            blocked: false,
            dimmed: true,
        })
        .collect();
    for (key, edge) in &edges.map {
        let (Some(&s), Some(&d)) = (
            sandbox_at.get(&key.sandbox),
            destination_at.get(&(key.host.clone(), key.port)),
        ) else {
            continue;
        };
        let (Some((seen, sandbox)), Some(destination)) =
            (sandbox_nodes.get_mut(s), destination_nodes.get_mut(d))
        else {
            continue;
        };
        let matches = filter.matches(&edge.latest);
        let blocked = is_blocked(&edge.latest);
        sandbox.dimmed &= !matches;
        destination.dimmed &= !matches;
        destination.blocked |= blocked;
        // The label and task of a sandbox node are those of its latest event.
        if edge.last >= *seen {
            *seen = edge.last;
            (sandbox.label, sandbox.task) = sandbox_lines(&edge.latest);
        }
        model.edges.push(EdgeLine {
            sandbox: s,
            destination: d,
            blocked,
            dimmed: !matches,
        });
    }
    // The map's order is arbitrary: sort the edges so that equal states render equal.
    model.edges.sort_by_key(|e| (e.sandbox, e.destination));
    model.sandboxes = sandbox_nodes.into_iter().map(|(_, node)| node).collect();
    model.destinations = destination_nodes;
    model
}

/// A node's vertical position.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Placed {
    pub(super) y: u32,
}

/// An edge from a sandbox's right side to a destination's left side, as a path in the gutter's
/// own coordinates (`0..gutter` wide).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Path {
    pub(super) d: String,
    pub(super) blocked: bool,
    pub(super) dimmed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) height: u32,
    /// Width of the space between the two columns.
    pub(super) gutter: u32,
    pub(super) sandboxes: Vec<Placed>,
    pub(super) destinations: Vec<Placed>,
    pub(super) paths: Vec<Path>,
}

/// Positions the nodes for `width` (limited to [`MIN_WIDTH`]..=[`MAX_WIDTH`]): sandboxes at
/// `x = 20` and 176 px wide, a 160 px pitch; destinations right-aligned with a 70 px pitch; the
/// first node 60 px from the top.
pub(super) fn layout(model: &Model, width: u32) -> Layout {
    let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
    let destination_width = (width - DEST_OFFSET).clamp(MIN_DEST_W, MAX_DEST_W);
    let gutter = MIN_GUTTER + (width - DEST_OFFSET - destination_width);
    // The shorter column is centred on the taller one.
    let sandbox_span = span(model.sandboxes.len(), SANDBOX_PITCH, SANDBOX_H);
    let destination_span = span(model.destinations.len(), DEST_PITCH, DEST_H);
    let content = sandbox_span.max(destination_span);
    let place = |count: usize, pitch: u32, span: u32| -> Vec<Placed> {
        let top = TOP + (content - span) / 2;
        (0..count)
            .map(|i| Placed {
                y: top + i as u32 * pitch,
            })
            .collect()
    };
    let sandboxes = place(model.sandboxes.len(), SANDBOX_PITCH, sandbox_span);
    let destinations = place(model.destinations.len(), DEST_PITCH, destination_span);
    let rows = (model.sandboxes.len() as u32 * SANDBOX_PITCH)
        .max(model.destinations.len() as u32 * DEST_PITCH);
    let paths = model
        .edges
        .iter()
        .filter_map(|edge| {
            let y1 = sandboxes.get(edge.sandbox)?.y + SANDBOX_H / 2;
            let y2 = destinations.get(edge.destination)?.y + DEST_H / 2;
            let mid = gutter / 2;
            Some(Path {
                d: format!("M0 {y1} C{mid} {y1} {mid} {y2} {gutter} {y2}"),
                blocked: edge.blocked,
                dimmed: edge.dimmed,
            })
        })
        .collect();
    Layout {
        height: TOP + rows + BOTTOM,
        gutter,
        sandboxes,
        destinations,
        paths,
    }
}

/// Height of a column of `count` nodes: from the top of the first to the bottom of the last.
fn span(count: usize, pitch: u32, node_height: u32) -> u32 {
    match count {
        0 => 0,
        n => (n as u32 - 1) * pitch + node_height,
    }
}

fn dim(dimmed: bool) -> &'static str {
    if dimmed { " opacity-30" } else { "" }
}

/// The contents of `#graph`. Positions are inline styles, so the region is replaced
/// (`innerHTML`), not morphed. Nodes are text; the drawing is decoration (`aria-hidden`).
pub(super) fn render(model: &Model, projects: &ProjectHosts) -> Markup {
    let layout = layout(model, DEFAULT_WIDTH);
    let gutter_left = SANDBOX_X + SANDBOX_W;
    let destination_width = destination_width_css();
    html! {
        div class="relative" style={"height:" (layout.height) "px;container-type:inline-size"} {
            @let sandboxes = html! { "Sandboxes " (model.sandboxes.len() + model.hidden_sandboxes) };
            // As in the design: the sandbox column sits in a dashed box that holds its heading
            // and grows only as far as the nodes need, so it moves with centred sandboxes.
            @if let (Some(first), Some(last)) = (layout.sandboxes.first(), layout.sandboxes.last()) {
                @let top = first.y - (TOP - GROUP_TOP);
                div data-group="sandboxes" class="absolute rounded-xl border border-dashed border-line-strong bg-sunk/50"
                    style={"left:" (SANDBOX_X - GROUP_PAD) "px;top:" (top) "px;width:" (SANDBOX_W + 2 * GROUP_PAD) "px;height:" (last.y + SANDBOX_H + GROUP_PAD - top) "px"} {
                    span class={"absolute " (HEADING)} style={"left:" (GROUP_PAD - 1) "px;top:" (GROUP_TOP - 1) "px"} { (sandboxes) }
                }
            }
            div class={"absolute inset-x-0 top-0 flex items-baseline px-5 pt-4 " (HEADING)} {
                @if layout.sandboxes.is_empty() {
                    span style={"width:" (SANDBOX_W) "px"} { (sandboxes) }
                }
                span class="ml-auto" style={"width:" (destination_width)} { "Destinations " (model.destinations.len() + model.hidden_destinations) }
            }
            // An SVG does not stretch between `left` and `right` (`width:auto` keeps its own
            // width), so the width is set explicitly: the gutter then always ends at the
            // destinations. It stretches horizontally only; the arrowheads belong to the
            // destination nodes, so the stretch never distorts them.
            svg aria-hidden="true" class="absolute top-0 overflow-visible" preserveAspectRatio="none"
                viewBox={"0 0 " (layout.gutter) " " (layout.height)}
                style={"left:" (gutter_left) "px;width:calc(100% - " (gutter_left + RIGHT_PAD) "px - " (destination_width) ");height:" (layout.height) "px"} {
                @for path in &layout.paths {
                    @let class = if path.blocked { "fill-none stroke-danger [stroke-dasharray:5_4]" } else { "fill-none stroke-faint" };
                    path d=(path.d) vector-effect="non-scaling-stroke" stroke-width="1.5"
                        class={(class) (dim(path.dimmed))} {}
                }
            }
            @for (node, placed) in model.sandboxes.iter().zip(&layout.sandboxes) {
                div data-node="sandbox" class={"absolute flex items-center gap-2.5 " (CARD) " border-line-strong px-2" (dim(node.dimmed))}
                    style={"left:" (SANDBOX_X) "px;top:" (placed.y) "px;width:" (SANDBOX_W) "px;height:" (SANDBOX_H) "px"} {
                    span aria-hidden="true" data-sandbox-icon class="inline-flex size-9 shrink-0 items-center justify-center rounded-lg bg-rail text-accent" { (icon(Icon::Box)) }
                    div class="min-w-0" {
                        div class="truncate font-mono text-[13px] font-medium" { (node.label) }
                        div class="truncate text-xs text-muted" { (node.task) }
                    }
                }
            }
            @for (node, placed) in model.destinations.iter().zip(&layout.destinations) {
                div data-node="destination" data-blocked=[node.blocked.then_some("true")]
                    class={"absolute flex items-center gap-2 " (CARD) " px-2 " (if node.blocked { "border-danger" } else { "border-line-strong" }) (dim(node.dimmed))}
                    style={"right:" (RIGHT_PAD) "px;top:" (placed.y) "px;width:" (destination_width) ";height:" (DEST_H) "px"} {
                    // Where the edges arrive: the tip touches the card's outer left edge.
                    svg data-arrow class="absolute -left-[8px] top-1/2 -translate-y-1/2" aria-hidden="true" width="7" height="10" viewBox="0 0 7 10" {
                        path d="M0 0 7 5 0 10z" class=(if node.blocked { "fill-danger" } else { "fill-faint" }) {}
                    }
                    (host_icon(&node.host, HostIconSize::Graph, projects))
                    span class="min-w-0 flex-1 truncate font-mono text-[13px]" title=(node.host) { (node.host) }
                    span class="shrink-0 rounded-md bg-sunk px-1.5 py-0.5 font-mono text-xs text-muted" { (node.port) }
                }
            }
            @if model.hidden_sandboxes > 0 || model.hidden_destinations > 0 {
                p class="absolute inset-x-0 bottom-0 px-5 pb-1 text-xs text-faint" {
                    @if model.hidden_sandboxes > 0 { "+" (model.hidden_sandboxes) " sandboxes not shown. " }
                    @if model.hidden_destinations > 0 { "+" (model.hidden_destinations) " destinations not shown." }
                }
            }
        }
        div class="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-line px-5 py-3 text-xs text-muted" {
            span class="inline-flex items-center gap-2" {
                svg aria-hidden="true" width="24" height="6" viewBox="0 0 24 6" { path d="M0 3h24" class="stroke-faint" stroke-width="1.5" {} }
                "Allowed"
            }
            span class="inline-flex items-center gap-2" {
                svg aria-hidden="true" width="24" height="6" viewBox="0 0 24 6" { path d="M0 3h24" class="stroke-danger [stroke-dasharray:5_4]" stroke-width="1.5" {} }
                "Blocked"
            }
            span { "Dimmed: excluded by the filters" }
        }
    }
}

/// CSS width of the destination nodes: the graph width (`cqw` of the graph container) less
/// the sandbox column, the minimum gutter and the right padding, between the node limits. The
/// nodes, the *Destinations* heading and the gutter SVG share it, so the browser keeps them
/// apart at any panel width.
fn destination_width_css() -> String {
    format!("clamp({MIN_DEST_W}px, 100cqw - {DEST_OFFSET}px, {MAX_DEST_W}px)")
}

#[cfg(test)]
mod tests {
    use super::super::filter::VerdictFilter;
    use super::*;
    use df_proto::gateway::v1::{Phase, Verdict};

    /// The graph without project hosts.
    fn render(model: &Model) -> Markup {
        super::render(model, &ProjectHosts::default())
    }

    fn event(run: &str, host: &str, verdict: Verdict) -> FlowEvent {
        FlowEvent {
            run_id: run.into(),
            task_key: "DF-7".into(),
            host: host.into(),
            port: 443,
            verdict: verdict.into(),
            ..Default::default()
        }
    }

    fn edges(events: &[FlowEvent]) -> Edges {
        let mut edges = Edges::default();
        for event in events {
            edges.update(event);
        }
        edges
    }

    fn model_of(events: &[FlowEvent]) -> Model {
        model(&edges(events), &Filter::default())
    }

    #[test]
    fn no_sandbox_means_no_nodes_and_a_small_graph() {
        let empty = model_of(&[]);
        assert_eq!(empty, Model::default());
        let l = layout(&empty, DEFAULT_WIDTH);
        assert!(l.sandboxes.is_empty() && l.destinations.is_empty() && l.paths.is_empty());
        assert_eq!(l.height, TOP + BOTTOM);
    }

    #[test]
    fn one_sandbox_one_destination() {
        let model = model_of(&[event("a4c2e7f01", "pypi.org", Verdict::Allowed)]);
        let l = layout(&model, DEFAULT_WIDTH);
        // The shorter column is centred on the taller one: both middles are at the same height.
        assert_eq!(l.sandboxes, [Placed { y: TOP }]);
        assert_eq!(l.destinations, [Placed { y: TOP + 2 }]);
        assert_eq!(l.height, TOP + SANDBOX_PITCH + BOTTOM);
        // From the middle of the sandbox box to the middle of the destination box.
        assert_eq!(
            l.paths[0].d,
            format!(
                "M0 {} C{} {} {} {} {} {}",
                TOP + 26,
                l.gutter / 2,
                TOP + 26,
                l.gutter / 2,
                TOP + 26,
                l.gutter,
                TOP + 26
            )
        );
        assert_eq!(model.sandboxes[0].label, "a4c2e7f0");
        assert_eq!(model.sandboxes[0].task, "DF-7");
    }

    #[test]
    fn three_sandboxes_use_the_pitches_and_the_taller_column_sets_the_height() {
        let model = model_of(&[
            event("run-a", "a.com", Verdict::Allowed),
            event("run-b", "a.com", Verdict::Allowed),
            event("run-c", "a.com", Verdict::Denied),
        ]);
        let l = layout(&model, DEFAULT_WIDTH);
        let ys: Vec<u32> = l.sandboxes.iter().map(|p| p.y).collect();
        assert_eq!(ys, [60, 220, 380]);
        // The one destination is centred on the middle sandbox (both middles at 246).
        assert_eq!(l.destinations, [Placed { y: 222 }]);
        assert_eq!(l.height, 60 + 3 * 160 + BOTTOM);
        assert_eq!(l.paths.len(), 3);
        // Many destinations of one sandbox: the destination column is taller.
        let hosts: Vec<_> = (0..10)
            .map(|i| event("run-a", &format!("h{i}.com"), Verdict::Allowed))
            .collect();
        let l = layout(&model_of(&hosts), DEFAULT_WIDTH);
        assert_eq!(l.height, 60 + 10 * 70 + BOTTOM);
        // The one sandbox is centred on the destinations: (9 * 70 + 48 - 52) / 2 below the top.
        assert_eq!(l.sandboxes, [Placed { y: 60 + 313 }]);
    }

    #[test]
    fn width_is_limited_and_the_gutter_never_collapses() {
        let model = model_of(&[event("a", "x.com", Verdict::Allowed)]);
        let narrow = layout(&model, 100);
        assert_eq!(MIN_WIDTH, 396);
        assert_eq!(narrow.gutter, MIN_GUTTER);
        let wide = layout(&model, 5000);
        // The destinations stop growing at their maximum: the gutter takes the rest.
        assert_eq!(
            wide.gutter,
            MAX_WIDTH - DEST_OFFSET + MIN_GUTTER - MAX_DEST_W
        );
    }

    #[test]
    fn the_caps_keep_the_most_recently_seen_and_count_the_rest() {
        let mut events: Vec<FlowEvent> = (0..35)
            .map(|i| event("run-a", &format!("h{i:02}.com"), Verdict::Allowed))
            .collect();
        // h00 was seen first but again last: it stays.
        events.push(event("run-a", "h00.com", Verdict::Allowed));
        let model = model_of(&events);
        assert_eq!(model.destinations.len(), MAX_DESTINATIONS);
        assert_eq!(model.hidden_destinations, 5);
        let hosts: Vec<&str> = model.destinations.iter().map(|d| d.host.as_str()).collect();
        assert!(
            hosts.contains(&"h00.com") && !hosts.contains(&"h01.com") && hosts.contains(&"h34.com")
        );
        // Drawn in the order they first appeared.
        assert_eq!(hosts[0], "h00.com");
        assert!(model.edges.iter().all(|e| e.destination < MAX_DESTINATIONS));

        let many: Vec<FlowEvent> = (0..15)
            .map(|i| event(&format!("run-{i:02}"), "a.com", Verdict::Allowed))
            .collect();
        let model = model_of(&many);
        assert_eq!(
            (model.sandboxes.len(), model.hidden_sandboxes),
            (MAX_SANDBOXES, 3)
        );
        let html = render(&model).into_string();
        assert!(html.contains("+3 sandboxes not shown."), "{html}");
        assert!(html.contains("Sandboxes 15"), "{html}");
    }

    #[test]
    fn a_denied_edge_is_blocked_and_marks_its_destination() {
        let model = model_of(&[
            event("run-a", "files.pythonhosted.org", Verdict::Denied),
            event("run-a", "pypi.org", Verdict::Allowed),
        ]);
        let blocked: Vec<_> = model
            .destinations
            .iter()
            .filter(|d| d.blocked)
            .map(|d| d.host.as_str())
            .collect();
        assert_eq!(blocked, ["files.pythonhosted.org"]);
        let html = render(&model).into_string();
        assert!(
            html.contains("[stroke-dasharray:5_4]") && html.contains("class=\"fill-danger\""),
            "{html}"
        );
        assert!(html.contains("border-danger") && html.contains("data-blocked=\"true\""));
        // One arrowhead per destination, drawn by the node itself in the node's colour.
        assert_eq!(html.matches("data-arrow").count(), 2, "{html}");
        assert!(
            html.contains("data-arrow class=\"absolute -left-[8px]")
                && html.contains("fill-danger")
        );
    }

    #[test]
    fn the_sandbox_column_has_a_dashed_group_box_around_its_heading_and_nodes() {
        let model = model_of(&[
            event("run-a", "pypi.org", Verdict::Allowed),
            event("run-b", "pypi.org", Verdict::Allowed),
        ]);
        let html = render(&model).into_string();
        // From 12 px left of and above the heading to 12 px below the last sandbox.
        let height = TOP + SANDBOX_PITCH + SANDBOX_H + 12 - 8;
        assert!(
            html.contains(&format!(
                "data-group=\"sandboxes\" class=\"absolute rounded-xl border border-dashed border-line-strong bg-sunk/50\" style=\"left:8px;top:8px;width:200px;height:{height}px\""
            )),
            "{html}"
        );
        assert!(!render(&model_of(&[])).into_string().contains("data-group"));
        // A taller destination column: the box only grows as far as the sandboxes need and
        // moves with the centred sandbox, its heading inside it.
        let hosts: Vec<_> = (0..10)
            .map(|i| event("run-a", &format!("h{i}.com"), Verdict::Allowed))
            .collect();
        let html = render(&model_of(&hosts)).into_string();
        let top = 60 + 313 - (TOP - 8);
        let height = (TOP - 8) + SANDBOX_H + 12;
        let group = &html[html.find("data-group").unwrap()..];
        assert!(
            group.contains(&format!("top:{top}px;width:200px;height:{height}px")),
            "{group}"
        );
        assert!(
            group[..group.find("</div>").unwrap()].contains("Sandboxes 1"),
            "{group}"
        );
    }

    #[test]
    fn the_destination_width_follows_the_graph_width_in_css() {
        let model = model_of(&[event("run-a", "pypi.org", Verdict::Allowed)]);
        let html = render(&model).into_string();
        // 276 = 20 + 176 + 60 + 20: sandbox column, minimum gutter and right padding.
        let width = "clamp(120px, 100cqw - 276px, 260px)";
        assert_eq!(destination_width_css(), width);
        assert!(html.contains("container-type:inline-size"), "{html}");
        // Destination node, heading and gutter SVG share the one width.
        assert_eq!(html.matches(width).count(), 3, "{html}");
        let node = &html[html.find("data-node=\"destination\"").unwrap()..];
        assert!(
            node.contains(&format!("width:{width};height:48px")),
            "{node}"
        );
        assert!(html.contains(&format!("style=\"width:{width}\"")), "{html}");
    }

    #[test]
    fn edges_stretch_with_the_panel_and_end_at_the_destination_nodes() {
        let model = model_of(&[event("run-a", "pypi.org", Verdict::Allowed)]);
        let html = render(&model).into_string();
        // An SVG does not stretch between `left` and `right`: its width is set explicitly, so
        // the gutter always reaches the destinations, which are aligned to the right edge.
        let left = SANDBOX_X + SANDBOX_W;
        assert!(
            html.contains(&format!(
                "left:{left}px;width:calc(100% - {}px - {});height:",
                left + RIGHT_PAD,
                destination_width_css()
            )),
            "{html}"
        );
        assert!(
            !html.contains("width:auto") && !html.contains("marker-end"),
            "{html}"
        );
        // The arrowheads are part of the node, so stretching never distorts them.
        let node = &html[html.find("data-node=\"destination\"").unwrap()..];
        assert!(node.find("data-arrow").unwrap() < node.find("pypi.org").unwrap());
    }

    #[test]
    fn the_latest_event_of_an_edge_wins_and_a_closing_event_replaces_the_opening_one() {
        let mut opened = event("run-a", "a.com", Verdict::Allowed);
        opened.phase = Phase::Opened.into();
        let denied_later = event("run-a", "a.com", Verdict::Denied);
        let model = model_of(&[opened.clone(), denied_later]);
        assert!(model.edges[0].blocked, "turned from allowed to denied");
        assert_eq!(model.edges.len(), 1);
        let mut edges = edges(std::slice::from_ref(&opened));
        let mut closed = opened;
        closed.phase = Phase::Closed.into();
        closed.bytes_out = 5;
        edges.update(&closed);
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges.map.values().next().unwrap().latest.phase(),
            Phase::Closed
        );
    }

    #[test]
    fn nodes_and_edges_the_filter_excludes_are_dimmed_not_removed() {
        let events = [
            event("run-a", "bad.com", Verdict::Denied),
            event("run-b", "good.com", Verdict::Allowed),
        ];
        let filter = Filter {
            verdict: Some(VerdictFilter::Blocked),
            ..Default::default()
        };
        let model = model(&edges(&events), &filter);
        assert_eq!((model.sandboxes.len(), model.destinations.len()), (2, 2));
        let dimmed: Vec<_> = model
            .destinations
            .iter()
            .filter(|d| d.dimmed)
            .map(|d| d.host.as_str())
            .collect();
        assert_eq!(dimmed, ["good.com"]);
        assert_eq!(model.sandboxes.iter().filter(|s| s.dimmed).count(), 1);
        assert_eq!(model.edges.iter().filter(|e| e.dimmed).count(), 1);
        assert!(render(&model).into_string().contains("opacity-30"));
    }

    #[test]
    fn a_node_shows_the_run_number_over_the_task_key() {
        let mut numbered = event("a4c2e7f01", "pypi.org", Verdict::Allowed);
        numbered.run_number = 2;
        let model = model_of(&[numbered]);
        assert_eq!(model.sandboxes[0].label, "#2");
        assert_eq!(model.sandboxes[0].task, "DF-7");
    }

    #[test]
    fn unknown_sources_are_nodes_by_address() {
        let mut unknown = event("", "a.com", Verdict::Denied);
        unknown.source_ip = "10.1.2.3".into();
        unknown.task_key.clear();
        let model = model_of(&[unknown]);
        assert_eq!(model.sandboxes[0].label, "10.1.2.3");
        assert_eq!(model.sandboxes[0].task, "unknown source");
    }

    #[test]
    fn a_numbered_node_renders_the_run_number_over_the_task_key() {
        let mut numbered = event("a4c2e7f01", "pypi.org", Verdict::Allowed);
        numbered.run_number = 2;
        let html = render(&model_of(&[numbered])).into_string();
        let node = &html[html.find("data-node=\"sandbox\"").unwrap()..];
        let node = &node[..node.find("data-node=\"destination\"").unwrap()];
        assert!(node.contains(">#2<") && node.contains(">DF-7<"), "{node}");
        assert!(!node.contains("a4c2e7f0"), "{node}");
    }

    #[test]
    fn a_sandbox_node_is_a_card_with_an_icon_tile_and_two_lines() {
        let html = render(&model_of(&[event(
            "a4c2e7f01",
            "pypi.org",
            Verdict::Allowed,
        )]))
        .into_string();
        let node = &html[html.find("data-node=\"sandbox\"").unwrap()..];
        let node = &node[..node.find("data-node=\"destination\"").unwrap()];
        assert!(
            node.contains("rounded-xl") && node.contains("shadow-sm"),
            "{node}"
        );
        assert!(node.contains("bg-rail text-accent") && node.contains("data-sandbox-icon"));
        assert!(
            node.contains("<svg") && node.contains("M12 22V12"),
            "box icon: {node}"
        );
        assert!(node.contains(">a4c2e7f0<") && node.contains(">DF-7<"));
        assert!(node.contains("truncate"), "{node}");
        assert!(
            node.contains("border-line-strong") && node.contains("bg-raised"),
            "{node}"
        );
        assert!(
            node.contains("text-xs text-muted"),
            "task line is muted: {node}"
        );
        // Destination cards look alike.
        let destination = &html[html.find("data-node=\"destination\"").unwrap()..];
        let destination = &destination[..destination.find("</div>").unwrap()];
        for class in ["rounded-xl", "shadow-sm", "border-line-strong", "bg-raised"] {
            assert!(destination.contains(class), "{class}: {destination}");
        }
    }

    #[test]
    fn the_drawing_is_decoration_and_the_nodes_are_text() {
        let html =
            render(&model_of(&[event("run-a", "github.com", Verdict::Allowed)])).into_string();
        assert!(html.contains("<svg aria-hidden=\"true\""));
        assert!(
            html.contains(">github.com<")
                && html.contains(">443<")
                && html.contains("data-host-icon=\"GH\"")
        );
        assert!(html.contains("Sandboxes 1") && html.contains("Destinations 1"));
        assert!(
            html.contains(">Allowed<") && html.contains(">Blocked<"),
            "legend"
        );
        assert!(html.contains("stroke-faint"));
    }
}
