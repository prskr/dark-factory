# Design

## Context

See `proposal.md` for the why and for what is deliberately left out. Current state:

- `/egress/log` (`crates/df-web/src/egress/log.rs`): a GET filter form (`denied`, `project`, `task`, `persona`, `host`), an SSE connection (`/egress/log/events`) that sends `<hx-partial>` messages, one `Viewer` per connection holding the filter, a denied-hosts summary table and the set of open rows. Rows show time, task, `method host:port`, verdict badge with reason, bytes and duration.
- `/egress/allowlist` (`allowlist.rs`): `list_header` + `data_table` with Scope / Kind / Allows and a remove button (Close icon).
- A `FlowEvent` (`contract/proto/darkfactory/gateway/v1/gateway.proto`) has time, gateway pod, source IP, run id, task, project, persona id, method, host, port, SNI, verdict, reason, matched rule (pattern), bytes in/out, duration, HTTP status (plain HTTP only), phase and flow id. It has no path, query, header or content type, because HTTPS is tunnelled (`CONNECT`) and not intercepted.
- Frontend stack: maud, Tailwind, htmx + SSE extension, Alpine (vendored). Precedents: Alpine state with `localStorage` in `try/catch` (collapsible details column), `keyup changed delay:300ms` fragments (forge feedback), the phone profile menu (dropdown).
- Comparison method: the design was read from the canvas files (`Network`, `EgressRules` and their variants) and compared with the code. A browser comparison was not possible: the `obscura` MCP server was not on `PATH`, and `just serve-local` runs with `--no-cluster`, which leaves `state.flows` empty so `/egress/log` answers "Egress log unavailable". Task 9.1 closes that gap.

## Goals / Non-Goals

**Goals:**
- Match the design's structure, copy, spacing and states for the two pages, in light and dark theme, using existing tokens and components.
- Keep the stream model: server-rendered partials over SSE, no client-side event store and no new JavaScript dependency.
- Make nothing in the pages depend on data the gateway does not record.

**Non-Goals:**
- Anything in the "Deliberately not" table of the proposal. TLS interception and per-rule usage are rejected, not deferred: nothing here prepares for them (no hooks, no empty sections).
- Phone layouts, route changes, gateway or proto changes, new database tables.
- A second state machine on the client: filters, graph and counts are computed on the server.

## Decisions

### D1. Filters stay on the server, the filter bar swaps the traffic region

The filter bar sits outside a `#traffic` region (graph, divider, table, counts, SSE connect element). Changing a control sends an `hx-get` of the page with the filter parameters, `hx-select="#traffic"`, `hx-push-url="true"`; the swapped SSE element reconnects with the new filter and a fresh backlog. The search input uses `input changed delay:300ms`; segments and selects trigger on `change`.

*Alternative:* client-side filtering over `data-*` attributes (instant, as in the mockup). Rejected: segment counts and graph dimming would need a second implementation of the filter logic on the client next to the server one that the SSE stream already needs, and the ring buffer allows 10,000 rows per viewer. A 300 ms debounce plus one backlog replay is acceptable and the URL stays shareable.

### D2. The viewer keeps the graph and the counts

`Viewer` replaces its denied-hosts summary (removed, D10) with:
- `edges: HashMap<(SandboxKey, host, port), FlowEvent>`: the latest event per edge, from **all** events seen (not only the filtered ones), where `SandboxKey` is the run id, or the source IP if there is none. Dimming is `!filter.matches(latest)`; an edge is blocked when that latest event is denied. Nodes derive from the edges.
- `counts`: matching events per verdict *ignoring* the verdict filter, and `seen` (all events), for the segment counts and "N of M".

Because the filtered stream must still deliver non-matching events to the graph, `shown()` splits into "feeds the graph and counts" (every flow event) and "gets a row" (matches the filter). `gap` and `backlog_end` markers feed neither graph nor counts; a `gap` keeps its own row in the table regardless of the filter, as today.

Each batch that changes graph or counts morphs `#graph` and `#counts` as `<hx-partial>`s. Rendering an SVG with absolutely positioned nodes server-side follows the mockup's layout: sandbox column x=20/width 176, destinations right of it, 60 px top offset, 70 px destination pitch, 160 px sandbox pitch (the mockup's `Network.dc.html` numbers), bezier edges. The graph height is `max(rows) * pitch`; the panel scrolls.

Caps: the 12 most recently seen sandboxes and 30 most recently seen destinations are drawn, with a "+N not shown" note; the edge map follows the ring buffer (cleared at `MAX_OPEN_ROWS` like the open set).

*Alternative:* client-side graph from row data. Rejected for the same reason as D1.

### D3. No name lookups

Persona names and task titles are not resolved (personas dropped as not critical). The sandbox column shows the first 8 characters of the run id and the task key (`DF-7`), which is what the event carries. The existing persona-id filter stays under *Advanced*. No database access is added to the log page.

### D4. One `data-flow` attribute per row feeds the details panel

Each row (and its compact menu) carries the event as JSON in `data-flow`, escaped by maud. An Alpine component on the page reads it when a details button is pressed and fills a fixed panel markup from it; it keeps only the row id. On every `htmx:afterSwap` of the table the panel re-reads `document.getElementById(rowId)`, so an open tunnel that closes updates the panel. If the row is gone (filter change), the panel closes.

The panel is an overlay `aside` (500 px, right edge, `role="dialog"`), closes on Escape and the close button, and returns focus to the row's button. The reason code is mapped to a one-sentence explanation in Rust (a `match` on the documented codes; unknown codes show only the code).

*Alternative:* fetch the details from the server by flow id. Rejected: flows live only in the gateway buffers and the viewer's SSE connection, not in the control plane.

### D5. Pause closes the stream

The connect element is inside an Alpine `x-if`. *Pause* removes it (htmx closes the SSE connection when the element leaves the DOM; to be verified in task 4.3, otherwise `hx-sse:close`), *Resume* re-inserts it and the backlog replaces the rows. The *Live* badge reads *Paused* meanwhile and *Reconnecting* on `htmx:sseError`.

*Alternative:* keep the connection and buffer messages in the browser. Rejected: unbounded memory for a long pause, and the gateways keep streaming to a page nobody reads.

### D6. Split view: CSS grid with a CSS variable

`#split` is a grid `var(--graph-w) 16px minmax(0, 1fr)`. The divider is a `role="separator"` with `aria-valuenow/min/max`, pointer capture for the drag, keys as in the mockup (32 px, 96 px with Shift, Home/End), double-click reset to 520 px. Limits: minimum 396 px + border for the graph (nodes untruncated), maximum so that the table keeps 520 px. The width lives in `localStorage` (`try/catch`, default when unavailable) and is applied by Alpine `x-init`, which also runs after the D1 swap.

### D7. Compact table by container query

The events panel is a Tailwind `@container`; below 680 px the `@[680px]:` variants hide the verdict column and the inline actions and show the row edge and the menu button. Markup contains both variants, CSS picks one, so no JavaScript depends on the width. The menu is the Alpine dropdown pattern of the phone profile menu. Its first iteration holds *Request details* and *Rule*; `iter2-egress-approvals` adds *Allow host…* there.

### D8. Link to a rule is a highlight, not an identifier

An event carries the matched pattern, not the entry id. The details panel and the row link to `/egress/allowlist?highlight=<matched_rule>:<port>`; the rules page highlights every row with that target. Several rows (instance and project scope) can match; that is shown as it is.

*Alternative:* resolve the entry id with a query per row. Rejected: the pattern identifies the rule well enough for a human, a query per event is wasteful, and ids would break once an entry is deleted between event and click.

### D9. Host icons: one component, static table

`host_icon(host, size)` in `components/` renders a rounded square with a brand monogram and colour from a static table (the mockup's: `api.anthropic.com` A, `github.com` and `objects.githubusercontent.com` GH, `registry.npmjs.org` n, `proxy.golang.org` Go, `pypi.org` and `files.pythonhosted.org` Py, `www.googleapis.com` G) or the globe icon. Wildcard entries use their suffix (`*.github.com` → GH). No logos are embedded, only letters. It is used by the rules table, the events table, graph nodes and the details panel. Colours are written as literal Tailwind classes so they are generated; the table is a module-level constant.

### D10. Naming and kept behaviour

UI label "Blocked" replaces "denied" (design); wire values, reasons and the spec's reason list keep "denied". The denied-hosts summary is removed: the graph (blocked destinations) and the *Blocked* segment cover spotting missing domains, and iteration 2 (`iter2-egress-approvals`) is going to supersede it with actions on the events table. Nobody uses the product yet, so no migration of the view is needed. The existing *project* and *task* filters stay under *Advanced* next to sandbox and the persona-id filter (the design shows persona and sandbox). Routes and URLs of both pages stay.

### D11. Development-only code behind a Cargo feature

`--no-cluster` (with `UnconfiguredBackend`) and `--demo-flows` (with the scripted `FlowSource`) exist for `just serve-local` only, so they are compiled only with a `dev-tools` feature of the `dark-factory` crate, off by default. A release binary then has neither flag nor stub. The fixture lives in the `dark-factory` crate (it fills `WebState.flows`), so `df-web` needs no feature. The pre-push clippy run uses `--all-features`, so the gated code stays linted; a default-features test pins that the flags are absent. Bazel's `rust_binary` sets no `crate_features`, so Bazel-built images never contain them.

*Alternative:* `cfg(debug_assertions)`. Needs no `Cargo.toml` or Justfile change, but ties the flags to the optimisation level, so a release-mode test run or a Bazel `fastbuild` image would silently differ from what is intended.

## Risks / Trade-offs

- [Server-rendered graph re-sent on every batch] → The graph partial is only sent when the batch changes an edge or a node (new edge, verdict change, new last-seen is not drawn), and batches are already coalesced by `ready_chunks(CHUNK)`.
- [Debounced reconnect on every filter change replays up to 10,000 events] → The backlog is sent as one message already; measured in task 9.1 with a 10,000-event fixture, and the debounce can grow if needed.
- [Details panel reads row attributes: a large `data-flow` per row bloats the DOM] → About 600 bytes per row; the table already holds up to 10,000 rows. Trimming rows is a follow-up if it shows in task 9.1.
- [Container queries need a recent browser] → Same support level as the rest of the Tailwind v4 output the project already ships.
- [Pause relies on htmx closing the SSE connection] → Task 4.3 verifies it in a test against the rendered markup and in the browser check; fallback is the extension's close attribute.
- [The graph dims by "latest event per edge"] → An edge that was denied once and allowed later turns grey; that is what "current state" means and is simpler than keeping two edges.
- [Design and `iter2-egress-approvals` diverge] → The action slot and the menu are reserved here; the proposal recommends updating that change to the design's dialog and toast.

## Migration Plan

No data or contract change. Deploy with the control plane. Old bookmarks keep working: `/egress/log?denied=1&project=…` is read as *Blocked* plus project filter, and both routes are unchanged. Rollback is a redeploy.

