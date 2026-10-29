//! The markup of `/egress/log`: header, filter bar, the split view and the Alpine state that
//! drives pause, divider and details panel.

use maud::{Markup, PreEscaped, html};

use super::details;
use super::filter::{ADVANCED_FIELDS, Filter, ROW_LIMITS, VerdictFilter};
use super::graph::{DEFAULT_WIDTH as DEFAULT_GRAPH_WIDTH, MIN_WIDTH};
use super::viewer::{Backlog, Counts, empty_state, event_count};
use crate::components::forms::INPUT_CLASS;
use crate::components::{ButtonKind, Icon, action_class, icon};

/// The controls of the *Advanced* row: like a form input, but as wide as their content, so a
/// label and its control stay on one line.
const FILTER_CONTROL: &str = "min-h-11 rounded-lg border border-line-strong bg-panel px-3 py-2 text-sm text-ink placeholder:text-faint lg:min-h-10";
/// Width of the divider in px.
const DIVIDER_WIDTH: u32 = 16;
/// The events table keeps at least this much.
const MIN_TABLE_WIDTH: u32 = 520;
/// Border of the graph panel in px, on each side.
const PANEL_BORDER: u32 = 1;
/// Narrowest graph panel: the graph's own minimum plus the panel border on both sides.
const MIN_GRAPH_PANEL: u32 = MIN_WIDTH + 2 * PANEL_BORDER;
/// Where the browser remembers the graph width.
const WIDTH_KEY: &str = "df.traffic.graph-w";

/// The argument of `egressLog(...)` in `x-data`: the server values the component needs.
/// `flow` is the event shown in the details panel; `dirty` shows *Clear filters* while any filter
/// is set.
fn state_args(advanced: usize, dirty: bool) -> String {
    serde_json::json!({
        "advanced": advanced,
        "dirty": dirty,
        "defaultWidth": DEFAULT_GRAPH_WIDTH,
        "widthKey": WIDTH_KEY,
        "advancedFields": ADVANCED_FIELDS,
        "minGraphPanel": MIN_GRAPH_PANEL,
        "dividerWidth": DIVIDER_WIDTH,
        "minTableWidth": MIN_TABLE_WIDTH,
    })
    .to_string()
}

/// The page; `personas` (id, name) are the choices of the persona filter, `backlog` is what the
/// stream's first message will show. The Alpine component `egressLog` of the page is
/// `assets/egress-log.js`; the layout loads it before Alpine.
pub(super) fn page_body(
    filter: &Filter,
    personas: &[(String, String)],
    backlog: &Backlog,
) -> Markup {
    let events = format!("/egress/log/events{}", filter.query());
    html! {
        div class="md:flex md:h-[calc(100dvh-4rem)] md:flex-col" x-data={"egressLog(" (state_args(filter.advanced_count(), !filter.is_empty())) ")"} ":class"="(dragging || graphDragging) && 'select-none'" {
            (header())
            (filter_bar(filter, personas, &backlog.counts))
            (traffic(&events, filter, backlog))
            (details::panel())
        }
    }
}

fn header() -> Markup {
    html! {
        header class="mb-5 flex shrink-0 flex-wrap items-center justify-between gap-4" {
            div class="flex items-center gap-3" {
                h1 class="text-2xl font-semibold" { "Network traffic" }
                span role="status" class="inline-flex items-center gap-1.5 rounded-full bg-status-completed-bg px-2.5 py-0.5 text-xs font-medium text-status-completed-fg"
                    ":class"="paused ? 'opacity-70' : ''" {
                    span aria-hidden="true" class="size-1.5 rounded-full bg-status-completed-fg" {}
                    span "x-text"="badge()" { "Live" }
                }
            }
            div class="flex items-center gap-2" {
                // 16 px glyphs, as in the design.
                button type="button" "x-on:click"="togglePause()" class={(action_class(ButtonKind::Secondary)) " [&_svg]:size-4"} {
                    span class="inline-flex" "x-show"="!paused" { (icon(Icon::Pause)) }
                    span class="inline-flex" "x-show"="paused" x-cloak { (icon(Icon::Play)) }
                    span "x-text"="paused ? 'Resume' : 'Pause'" { "Pause" }
                }
                a href="/egress/allowlist" class={(action_class(ButtonKind::Secondary)) " [&_svg]:size-4"} {
                    (icon(Icon::Shield))
                    "Egress rules"
                }
            }
        }
    }
}

fn segment(value: VerdictFilter, label: &str, selected: VerdictFilter, count: usize) -> Markup {
    let id = format!("count-{}", value.param());
    html! {
        label class="relative" {
            input type="radio" name="verdict" value=(value.param()) checked[value == selected] class="peer sr-only";
            span class="inline-flex min-h-11 cursor-pointer items-center gap-1.5 px-4 text-sm font-medium text-muted peer-checked:bg-accent-soft peer-checked:text-accent-text peer-focus-visible:outline-2 lg:min-h-9" {
                (label) " " span id=(id) class="font-mono text-xs" { (count) }
            }
        }
    }
}

fn filter_bar(filter: &Filter, personas: &[(String, String)], counts: &Counts) -> Markup {
    let selected = filter.verdict_filter();
    let advanced = filter.advanced_count();
    let open = advanced > 0;
    let persona = filter.persona.trim();
    // A persona that no longer exists stays selectable, so the link keeps its meaning.
    let unknown = !persona.is_empty() && !personas.iter().any(|(id, _)| id == persona);
    html! {
        form id="filters" method="get" action="/egress/log" class="mb-4 flex shrink-0 flex-col items-end gap-2"
            "x-on:input"="sync($el)" "x-on:change"="sync($el)"
            hx-get="/egress/log" hx-trigger="input changed delay:300ms, change" hx-target="#traffic" hx-select="#traffic"
            hx-select-oob="#count-all,#count-blocked,#count-allowed" hx-swap="outerHTML" hx-push-url="true" {
            div id="filter-row" class="flex flex-wrap items-center justify-end gap-2" {
                button type="button" "x-show"="dirty" x-cloak[filter.is_empty()] "x-on:click"="clearFilters($el.form)"
                    class={(action_class(ButtonKind::Ghost)) " whitespace-nowrap"} { "Clear filters" }
                label class="relative block w-full sm:w-[300px]" {
                    span class="sr-only" { "Search domain or IP" }
                    span aria-hidden="true" class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-faint" { (icon(Icon::Search)) }
                    input type="search" name="q" value=(filter.search()) placeholder="Search domain or IP"
                        class={(INPUT_CLASS) " pl-10 font-mono text-sm"};
                }
                fieldset class="inline-flex overflow-hidden rounded-lg border border-line-strong bg-panel" {
                    legend class="sr-only" { "Verdict" }
                    (segment(VerdictFilter::All, "All", selected, counts.all))
                    (segment(VerdictFilter::Blocked, "Blocked", selected, counts.blocked))
                    (segment(VerdictFilter::Allowed, "Allowed", selected, counts.allowed))
                }
                button type="button" "x-on:click"="advOpen = !advOpen" aria-controls="adv-filters"
                    aria-expanded=(if open { "true" } else { "false" }) ":aria-expanded"="advOpen"
                    class={(action_class(ButtonKind::Secondary)) " whitespace-nowrap"} {
                    (icon(Icon::Filter))
                    "Advanced"
                    span class="rounded-full bg-ink px-1.5 text-xs text-panel"
                        "x-show"="adv > 0" "x-text"="adv" x-cloak[advanced == 0] { (advanced) }
                }
                // A view setting, not a filter: Clear filters keeps it.
                label for="rows-limit" class="ml-2 text-sm text-muted" { "Rows" }
                select id="rows-limit" name="rows" class=(FILTER_CONTROL) {
                    @for limit in ROW_LIMITS {
                        option value=(limit) selected[limit == filter.row_limit()] { (limit) }
                    }
                }
            }
            div id="adv-filters" class="flex flex-wrap items-center justify-end gap-2" "x-show"="advOpen" x-cloak[!open] {
                label for="persona-filter" class="text-sm text-muted" { "Persona" }
                select id="persona-filter" name="persona" class=(FILTER_CONTROL) {
                    option value="" { "All personas" }
                    @for (id, name) in personas {
                        option value=(id) selected[id == persona] { (name) }
                    }
                    @if unknown {
                        option value=(persona) selected { (persona) }
                    }
                }
                label for="task-filter" class="ml-2 text-sm text-muted" { "Task" }
                input id="task-filter" type="text" name="task" value=(filter.task.trim()) placeholder="DF-7"
                    class={(FILTER_CONTROL) " w-32 font-mono"};
            }
        }
    }
}

fn traffic(events_url: &str, filter: &Filter, backlog: &Backlog) -> Markup {
    html! {
        div id="traffic" class="flex min-h-0 flex-1 flex-col" {
            div id="sse-slot" data-url=(events_url) {
                div hx-sse:connect=(events_url) hx-swap="none" {}
            }
            div id="split" x-ref="split"
                class="grid min-h-0 flex-1 grid-rows-[minmax(0,1fr)] items-stretch gap-0"
                style={"grid-template-columns:var(--graph-w," (DEFAULT_GRAPH_WIDTH) "px) " (DIVIDER_WIDTH) "px minmax(0,1fr)"}
                ":style"="{'--graph-w': w + 'px'}" "x-on:resize.window"="setW(want, false)" {
                section aria-labelledby="connections-title" class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-xl border border-line bg-panel" {
                    h2 id="connections-title" class="shrink-0 border-b border-line px-5 py-3 text-xs font-medium uppercase tracking-wide text-faint" { "Connections" }
                    // Outside `#graph`, which is morphed: the scroll position survives updates.
                    div id="graph-scroll" role="region" aria-label="Connections graph" tabindex="0"
                        class="relative min-h-0 flex-1 overflow-auto cursor-grab focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent-text"
                        "x-on:pointerdown"="graphDragStart($event)" "x-on:pointermove"="graphDragMove($event)"
                        "x-on:pointerup"="graphDragEnd()" "x-on:pointercancel"="graphDragEnd()" {
                        div id="graph" { (PreEscaped(&backlog.graph)) }
                    }
                }
                div role="separator" aria-orientation="vertical" aria-label="Resize connections and events"
                    aria-valuenow=(DEFAULT_GRAPH_WIDTH) aria-valuemin=(MIN_GRAPH_PANEL) aria-valuemax=(DEFAULT_GRAPH_WIDTH + 240)
                    ":aria-valuenow"="w" ":aria-valuemax"="limits()[1]" tabindex="0"
                    class="group flex h-full min-h-40 cursor-col-resize touch-none items-center justify-center focus-visible:outline-2"
                    "x-on:pointerdown"="dragStart($event)" "x-on:pointermove"="dragMove($event)"
                    "x-on:pointerup"="dragEnd()" "x-on:pointercancel"="dragEnd()"
                    "x-on:keydown"="divKey($event)" "x-on:dblclick"=(format!("setW({DEFAULT_GRAPH_WIDTH})")) {
                    span aria-hidden="true" class="h-12 w-1 rounded-full bg-line-strong/50 group-hover:bg-line-strong group-focus-visible:bg-accent-text" {}
                }
                (events_panel(backlog, filter))
            }
        }
    }
}

fn events_panel(backlog: &Backlog, filter: &Filter) -> Markup {
    let counts = &backlog.counts;
    let th = "sticky top-0 bg-panel px-3 py-3 font-medium";
    html! {
        section id="events" aria-labelledby="events-title" class="@container flex min-h-0 min-w-0 flex-col rounded-xl border border-line bg-panel" {
            div class="flex shrink-0 items-baseline justify-between gap-3 border-b border-line px-5 py-3" {
                h2 id="events-title" class="text-xs font-medium uppercase tracking-wide text-faint" { "Events · newest first" }
                span id="event-count" class="font-mono text-xs text-faint" { (event_count(counts)) }
            }
            div id="flows-scroll" class="relative min-h-0 flex-1 overflow-auto has-[+#flows-empty:not(:empty)]:flex-none" {
                table id="flows" class="w-full border-collapse text-left text-sm" {
                    thead {
                        tr class="text-xs font-medium uppercase tracking-wide text-faint" {
                            th scope="col" class=(th) { "Time" }
                            th scope="col" class=(th) { "Sandbox" }
                            th scope="col" class=(th) { "Destination" }
                            th scope="col" class={"hidden @[680px]:table-cell " (th)} { "Verdict" }
                            th scope="col" class="sticky top-0 w-11 bg-panel py-3 pr-4 pl-3" { span class="sr-only" { "Actions" } }
                        }
                    }
                    tbody data-limit=(filter.row_limit()) { (backlog.rows) }
                }
            }
            div id="flows-empty" class="flex shrink-0 grow items-center justify-center empty:grow-0" { (empty_state(counts, filter)) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egress::log::graph::{DEST_OFFSET, MIN_DEST_W};
    use crate::egress::tests::component_script;

    #[test]
    fn the_narrowest_panel_leaves_the_destinations_their_minimum_and_the_gutter_its_own() {
        // The panel has a border on both sides; the graph gets what is left.
        let graph = MIN_GRAPH_PANEL - 2 * PANEL_BORDER;
        assert!(graph >= DEST_OFFSET + MIN_DEST_W, "graph is {graph} px");
    }

    #[test]
    fn a_filter_change_also_swaps_the_segment_counts() {
        // The counts depend on the filter but sit outside `#traffic`, the form's swap target.
        let form = filter_bar(&Filter::default(), &[], &Counts::default()).into_string();
        assert!(form.contains(r##"hx-select-oob="#count-all,#count-blocked,#count-allowed""##));
    }

    #[test]
    fn page_passes_the_server_values_to_the_component() {
        let html = page();
        let root = tag(&html, "x-data=");
        assert!(
            root.contains("egressLog(") && root.contains("&quot;widthKey&quot;"),
            "{root}"
        );
        assert!(component_script().contains("Alpine.data('egressLog'"));
    }

    fn page() -> String {
        page_body(
            &Filter::default(),
            &[],
            &Backlog {
                rows: html! {},
                graph: String::new(),
                counts: Counts::default(),
            },
        )
        .into_string()
    }

    /// The tag that carries `marker`, without its content.
    fn tag<'a>(html: &'a str, marker: &str) -> &'a str {
        let at = html
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} missing"));
        let start = html[..at].rfind('<').unwrap();
        &html[start..=(at + html[at..].find('>').unwrap())]
    }

    #[test]
    fn from_md_up_the_page_is_a_viewport_high_column_and_the_split_fills_it() {
        let html = page();
        let root = tag(&html, "x-data=");
        assert!(
            root.contains("md:flex")
                && root.contains("md:h-[calc(100dvh-4rem)]")
                && root.contains("md:flex-col")
        );
        assert!(tag(&html, "<header").contains("shrink-0"));
        assert!(tag(&html, "id=\"filters\"").contains("shrink-0"));
        let traffic = tag(&html, "id=\"traffic\"");
        assert!(traffic.contains("flex min-h-0 flex-1 flex-col"));
        let split = tag(&html, "id=\"split\"");
        assert!(split.contains("min-h-0 flex-1") && split.contains("items-stretch"));
        assert!(!split.contains("items-start"));
    }

    #[test]
    fn both_panel_bodies_scroll_and_the_table_header_is_pinned() {
        let html = page();
        for marker in ["id=\"graph-scroll\"", "id=\"flows-scroll\""] {
            let body = tag(&html, marker);
            assert!(body.contains("min-h-0 flex-1 overflow-auto"), "{body}");
            // The panel is the containing block of the table's `sr-only` spans, which would
            // otherwise escape the clip and make the page scroll.
            assert!(body.contains("relative"), "{body}");
        }
        assert!(html.contains("sticky top-0 bg-panel"));
    }

    #[test]
    fn the_graph_scroll_area_is_keyboard_focusable_and_named() {
        let html = page();
        // No focusable control inside the graph: without a tab stop, keyboard users (Safari)
        // cannot scroll to the lower nodes.
        let scroll = tag(&html, "id=\"graph-scroll\"");
        assert!(scroll.contains("tabindex=\"0\""), "{scroll}");
        assert!(scroll.contains("role=\"region\""), "{scroll}");
        assert!(
            scroll.contains("aria-label=\"Connections graph\""),
            "{scroll}"
        );
    }

    #[test]
    fn the_divider_is_a_short_rounded_grip_in_a_wide_hit_area() {
        let html = page();
        let separator = tag(&html, "role=\"separator\"");
        // The design: a 16 px wide handle with a 4 x 48 px pill grip in the middle.
        assert!(
            separator.contains("items-center justify-center"),
            "{separator}"
        );
        let at = html.find("role=\"separator\"").unwrap();
        let grip = tag(&html[at..], "aria-hidden=\"true\"");
        assert!(
            grip.contains("h-12 w-1 rounded-full bg-line-strong/50"),
            "{grip}"
        );
        assert!(!grip.contains("w-0.5"), "{grip}");
    }

    #[test]
    fn pause_and_egress_rules_carry_their_design_icons() {
        let html = header().into_string();
        let pause = &html
            [html.find("togglePause()").unwrap()..html.find("href=\"/egress/allowlist\"").unwrap()];
        // Pause shows the pause glyph, Resume the play glyph.
        assert!(pause.matches("<svg").count() == 2, "{pause}");
        assert!(pause.contains("[&amp;_svg]:size-4"), "{pause}");
        let rules = &html[html.find("href=\"/egress/allowlist\"").unwrap()..];
        assert!(
            rules.contains("<svg") && rules.contains("[&amp;_svg]:size-4"),
            "{rules}"
        );
    }

    #[test]
    fn the_empty_state_fills_and_centres_in_the_space_under_the_table_header() {
        let html = page();
        // With an empty state the rows box shrinks to the header and the empty state takes the
        // rest; without one it stays out of the way.
        let scroll = tag(&html, "id=\"flows-scroll\"");
        assert!(
            scroll.contains("has-[+#flows-empty:not(:empty)]:flex-none"),
            "{scroll}"
        );
        let empty = tag(&html, "id=\"flows-empty\"");
        assert!(
            empty.contains("grow items-center justify-center empty:grow-0"),
            "{empty}"
        );
    }

    #[test]
    fn the_graph_scroll_container_sits_outside_the_morphed_graph_and_drags() {
        let html = page();
        let scroll = html.find("id=\"graph-scroll\"").unwrap();
        assert!(scroll < html.find("id=\"graph\"").unwrap());
        let scroll = tag(&html, "id=\"graph-scroll\"");
        assert!(scroll.contains("pointerdown") && scroll.contains("graphDragStart"));
        assert!(scroll.contains("graphDragMove") && scroll.contains("cursor-grab"));
        let js = component_script();
        assert!(js.contains("graphDragStart(e)") && js.contains("[data-node]"));
        // Relative to the scroll box, not to the event target (the tall graph inside it).
        assert!(js.contains("e.clientX - rect.left - box.clientLeft >= box.clientWidth"));
        assert!(!js.contains("offsetX") && !js.contains("offsetY"));
    }
}
