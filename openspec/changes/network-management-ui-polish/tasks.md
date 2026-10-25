# Tasks

Tests come first in every task (write the failing test, then the code). Database-backed tests of `df-web` need a container socket; list the ones that could not run in your report.

## 1. Shared pieces and naming

- [ ] 1.1 Add `host_icon` (design D9: static monogram table, wildcard entries use their suffix, globe fallback, sizes for table/graph/panel) and the `Trash` and `Server` icons (Lucide, with licence note as for existing icons); verify unit tests for `github.com`, `*.github.com`, `ghcr.io` and an IP literal
- [ ] 1.2 Rename the navigation entries to "Egress rules" and "Network traffic" (labels, tooltips, page `<title>`s; routes unchanged); verify the `layout.rs` rail test asserts the new labels and the unchanged hrefs
- [ ] 1.3 Document `host_icon` and the two renamed entries in `docs/design-system.md`; verify the docs test that lists components passes

## 2. Egress rules page

- [ ] 2.1 Rebuild the header (title with count, subtitle, "Live traffic" link, "New egress rule", "Add cluster service") and the empty state with the design's copy and both actions; verify the `egress-allowlist` scenarios "Title and count", "Link to live traffic" and "No entries" in rendered-HTML tests
- [ ] 2.2 Rebuild the table (target with `host_icon` or "cluster service" badge, port(s), scope, trash delete button with accessible name); verify the scenarios "Known host", "Unknown host", "Cluster service" and "Delete" (existing delete/audit tests keep passing)
- [ ] 2.3 Add `?highlight=host:port` (tint, `aria-current`, scroll first match into view); verify the scenarios "Link from the traffic page" and "No match"

## 3. Viewer state on the server

- [ ] 3.1 Extend `Filter` with `q`, `verdict` (`all|blocked|allowed`) and `sandbox`, keep `denied=1` as `verdict=blocked`, map `blocked` to `Verdict::Denied` (no new wire value), and build the query string from them; the subscription stays unfiltered (`flows.subscribe()`) and `Filter` is applied in `Viewer`; verify unit tests for the scenarios "Shared link" and "Old link"
- [ ] 3.2 Remove the denied-hosts summary (`DeniedSummary`, its partial, its tests) and add the edge map and the counts to `Viewer` (design D2): graph/counts fed by all events, rows by matching ones, open connections count as allowed, `gap`/`backlog_end` markers feed neither, caps with a "not shown" count; verify the scenario "Missing domain" and tests for "Segment counts ignore the verdict", "Many destinations" and that a closing event replaces the opened edge state

## 4. Page shell, filters and pause

- [ ] 4.1 Render the header (title, stream badge, Pause/Resume, "Egress rules" link) ; verify the scenarios "Live stream" and "Link to the rules"
- [ ] 4.2 Render the filter bar outside `#traffic` (search, segmented control with counts, Advanced with count badge, Clear filters) and swap `#traffic` with `hx-get`/`hx-select`/`hx-push-url` (design D1); verify the scenarios "Search narrows rows and graph" and "Shared link" at HTML/handler level, and that the filter bar keeps focus semantics (the input is not inside the swapped region)
- [ ] 4.3 Implement Pause/Resume by removing and re-inserting the SSE connect element (design D5) and the *Reconnecting* state; verify the scenarios "Pause" and "Resume" (markup test for the Alpine toggle; the stream-closing behaviour is checked in 9.1)

## 5. Events table

- [ ] 5.1 Rebuild the table (header "Events · newest first" with "N of M", columns Time, Sandbox, Destination with `host_icon`, Verdict pill *Allowed/Blocked/Open*, actions) and move bytes/duration out of the table; verify the scenarios "Row content", "Open connections" and "Missed events"
- [ ] 5.2 Add the row actions (details button, "Rule" link only for allowed rows with a matched rule) and the empty states; verify the scenarios "Row actions", "No traffic yet" and "Nothing matches"
- [ ] 5.3 Add the container-query compact variant (design D7): row edge, hidden verdict column with an accessible verdict, actions menu; verify the scenarios "Narrow panel" and "Wide panel" by asserting both variants' markup and classes, and that the menu button has an accessible name with host, port and time

## 6. Connection graph

- [ ] 6.1 Add the pure layout function (positions of sandboxes, destinations and edge paths from the viewer state and the available width, design D2/D6 numbers) with unit tests for 0, 1 and 3 sandboxes, truncation to the caps and the width limits
- [ ] 6.2 Render the graph partial (`#graph`: SVG edges with arrowheads, node boxes with `host_icon` and port chip, headings with counts, legend, dimming) and send it with batches that change it; verify the scenarios "Blocked destination", "Live update" and "Filtered out" in viewer tests (messages contain the partial exactly when the graph changed)
- [ ] 6.3 Verify the graph in both themes with the existing tokens and add text alternatives (nodes are text, SVG decoration `aria-hidden`); check the red and grey strokes against 3:1 and record the pairs in `docs/design-system.md`

## 7. Split view

- [ ] 7.1 Add the grid, the separator and the Alpine component (pointer capture, keyboard, double-click, `localStorage` in `try/catch`, limits from design D6); verify markup tests for `role="separator"`, `aria-valuenow/min/max` and the scenarios "Keyboard" and "Reset" as far as they are testable without a browser, the rest in 9.1
- [ ] 7.2 Re-apply the stored width after the D1 swap; verify the scenario "Remembered width" in the manual check (9.1) and that the markup has no dependency on the stored value

## 8. Request details

- [ ] 8.1 Put the event as escaped JSON into each row's `data-flow`, map reason codes to explanations in Rust and render the panel markup (header, summary bar, connection, reason, matched rule, "Requests inside TLS tunnels are not inspected." note); verify the scenarios "Allowed tunnel" and "Denied request" at HTML level, that no URL/query/header section exists, and that every documented reason has an explanation
- [ ] 8.2 Add the rule link `/egress/allowlist?highlight=<matched_rule>:<port>` to panel and row; verify the scenario "Link to the matched rule" together with 2.3
- [ ] 8.3 Add open/close behaviour (Escape, close button, focus return, re-read on `htmx:afterSwap`, close when the row disappears); verify the scenarios "Open and close" and "Open connection closes" in the manual check (9.1) and that the markup has `role="dialog"` and an accessible name

## 9. Documentation, fixture and integration check

- [ ] 9.1 Add a local-debugging fixture: a scripted `FlowSource` (a mix of allowed, denied, open and gap events, several runs, a 10,000-event mode) in the `dark-factory` crate, compiled only with the Cargo feature `dev-tools` (design D11) and used only when `serve --demo-flows` is given (needs `--no-cluster`); verify with `cargo test -p dark-factory --features dev-tools` that the flag fills `state.flows`, then check that `just serve-local` shows the page and compare it with the canvas boards `Network`, `NetworkAllow`, `NetworkCompact`, `EgressRules`, `EmptyEgress` (not the "Rejected" row) at 1280 px in light and dark and check pause/resume, divider (drag, keys, reload), filters, panel and compact menu; record deviations in the change's `review-log.md`
- [ ] 9.2 Document the pages in `crates/df-web/README.md` or `docs/design-system.md` (graph, divider, details panel, what the panel cannot show and why) and update `design/egress.md` §8 (UI sketch: graph, events table, details panel); verify the docs test passes and the text names the TLS limitation
- [ ] 9.3 Record the rejections in `design/egress.md`: replace the "Later, per-host opt-in interception" sentence in §7 by "Rejected: TLS interception" with the reasons (CA in every trust store, redaction, retention), and add a "Rejected: per-rule usage (last matched, request counts)" note to §8; add `egress-rule-editing` to `product/iterations/roadmap.md`; verify no document still presents interception or per-rule usage as planned (`grep -ri interception design docs product`)
- [ ] 9.4 Gate the development-only code behind the feature `dev-tools` (design D11): (WIP commits are fine while iterating; when the work is squashed, this task becomes its own commit, separate from the UI polish) `serve --no-cluster`, `--demo-flows`, the `UnconfiguredBackend` wiring in `main.rs` and its `unconfigured` module (kept for tests via `cfg(any(test, feature = "dev-tools"))`), and add `--features dev-tools` to `just serve-local`; update `docs/development.md`; verify that `cargo test -p dark-factory` (default features) contains a test that `serve --no-cluster` is rejected by the parser, that `cargo build -p dark-factory --release` binary's `serve --help` lists neither flag, and that `cargo clippy --all-targets --all-features -- -D warnings` still passes
- [ ] 9.5 Update `iter2-egress-approvals` (`proposal.md`, `design.md`, `tasks.md` 3.1 and its `egress-log` spec): remove the "denied-hosts summary row" part of "Allow from the log" and attach the actions to events table rows and graph nodes, using the action slot and menu of this change; verify `openspec validate iter2-egress-approvals --strict` passes and no text there still mentions the summary
- [ ] 9.6 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, `cargo-machete`, the `df-web` tests that need no container and the container-backed ones where a socket exists; verify all pass and list the tests that could not run
