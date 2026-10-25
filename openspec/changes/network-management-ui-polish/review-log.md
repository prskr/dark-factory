# Review log: network-management-ui-polish

- BASE: 2babb366dd061e1a75eb9e68c18c1cb741b8cec5
- Store: none
- Status: done
- Implementer agent: a640b3b8d00390aff

## Round 1 (2026-10-25)

- Implementer: f5ad780, tasks 28/28, checks fmt/clippy/machete ok, df-web 282 pass, dark-factory tests pass (default + dev-tools), container tests ran (podman); not browser-verified: SSE-driven rows/graph/counts, pause/resume stream close, divider drag/keys, details panel, compact menu, themes, 1280px canvas comparison
- Reviewed range: BASE...f5ad780

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:52` | Pause removes SSE element via plain DOM; htmx has no MutationObserver, cleanup never runs, idle stream stays open | fix-now | fixed (8917014) |
| S5 | Standards | Blocking | `crates/df-web/src/egress/log/graph.rs:192-194,278-279` | Unchecked indexing in production path | fix-now | fixed (8917014) |
| P2 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:199-205` | Advanced filter badge stale after swap | fix-now | fixed (8917014) |
| P3 | Spec | Blocking | `crates/df-web/src/egress/log/rows.rs:194-201` | Blocked tint on first cell only, not row | fix-now | fixed (8917014) |
| P4 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:37-41` | Stored divider width not clamped (table < 520 px) | fix-now | fixed (8917014) |
| P5 | Spec | Blocking | `crates/df-web/src/egress/log/filter.rs:128` | Host search case-sensitive | fix-now | fixed (8917014) |
| P8 | Spec | P1 | `crates/df-web/src/egress/log/page.rs:62-66` | Filter change while paused resumes; undocumented | fix-now (document in D5) | fixed (8917014) |
| P9 | Spec | P1 | `crates/df-web/src/egress/tests.rs:738` | Empty state test misses no-graph-nodes assertion | fix-now | fixed (8917014) |
| S1 | Standards | P1 | `crates/df-web/src/egress/log/viewer.rs:34,232` | Split `impl Counts` | fix-now | fixed (e8c959e, after regression) |
| S2 | Standards | P1 | `crates/df-web/src/egress/log/viewer.rs:72,218` | Private helpers between type and impl | fix-now | fixed (8917014) |
| S3 | Standards | P1 | `crates/dark-factory/src/demo_flows.rs:78-146` | Consts/helpers between struct and impl | fix-now | fixed (e8c959e) |
| S4 | Standards | P1 | `crates/df-web/src/egress/log/rows.rs:176`, `details.rs:35` | Consts mid-file | fix-now | fixed (8917014) |
| S6 | Standards | P1 | `crates/df-web/src/egress/log/viewer.rs:146` | `rows[at[&id]]` indexing | fix-now | fixed (8917014) |
| S7 | Standards | P1 | `crates/dark-factory/src/main.rs:1028-1045` | let-else panic guards in tests, duplicated | fix-now | fixed (8917014) |
| S8 | Standards | P1 | `crates/df-web/src/egress/log/viewer.rs:138-200` | Duplicated row-collect loop | fix-now | fixed (8917014) |
| S9 | Standards | P1 | `crates/df-web/src/egress/log/filter.rs:44-58` | `verdict: String` parsed by hand (keep legacy params, spec requires `denied=1`) | fix-now | fixed (8917014) |
| S10 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:36`, `page.rs:14,227` | 520 defined three times | fix-now | fixed (8917014) |
| P6 | Spec | P2 | client-side JS | JS scenarios (open/close, drag, remembered width) only markup-tested; no browser SSE check | tech-debt | tech-debt (TD-56) |
| S11 | Standards | P3 | `crates/df-web/src/egress/log/viewer.rs:97` | `MAX_OPEN_ROWS` reused as edge cap | fix-now (user) | fixed (8917014) |
| S12 | Standards | P3 | `crates/df-web/src/egress/allowlist.rs:140,182`, `rows.rs:154,193` | Duplicated host:port formatting | fix-now (user) | fixed (8917014) |
| S13 | Standards | P3 | `crates/df-web/src/egress/log/graph.rs:172` | Tuple instead of struct | fix-now (user) | fixed (8917014) |
| S14 | Standards | P3 | `crates/df-web/src/egress/tests.rs:686` | Mysterious test helper name | fix-now (user) | fixed (8917014) |
| P7 | Spec | Rejected | `crates/dark-factory/tests/cli.rs:58` | Task wording | - | rejected (covered by main.rs parser test) |

User decision: Adjust: fix Blocking + P1 + S11–S14; P6 to tech debt.

## Round 2 (2026-10-25)

- Implementer: 8917014, tasks 28/28, checks fmt/clippy/machete ok, df-web 285 pass, dark-factory tests pass, container tests ran (podman); P1 pause abort and P4 clamp not browser-verified
- Reviewed range: f5ad780...8917014

Ledger check: P1–P5, P8, P9, S2, S4–S14 confirmed fixed (8917014). S1 regressed, S3 partly fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P10 | Spec+Standards | Blocking | `crates/df-web/src/egress/log/page.rs:240-241` | Regression from S10: Alpine string `:style` replaces static style, grid-template-columns lost, split view collapses | fix-now | fixed (e8c959e) |
| P11 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:42` | Width not re-clamped on window resize; table < 520 px | fix-now | fixed (e8c959e) |
| S1 | Standards | Blocking (regression) | `crates/df-web/src/egress/log/viewer.rs:25-46` | `enum Emptiness` between `struct Counts` and its impl | fix-now | fixed (e8c959e) |
| S3 | Standards | P1 | `crates/dark-factory/src/demo_flows.rs:138` | `impl FlowSource for DemoFlows` after private helpers | fix-now | fixed (e8c959e) |
| S15 | Standards | P1 | `crates/df-web/src/egress/log/filter.rs:36` | Private `lenient_verdict` between public items | fix-now | fixed (e8c959e) |
| S16 | Standards | P1 | `crates/df-web/src/egress/log/filter.rs:40-43` | Verdict mapping duplicates `param()` | fix-now | fixed (e8c959e) |
| S17 | Standards | P1 | `crates/df-web/src/egress/log/viewer.rs:84-92` | Private `RowBatch`/`RowAction` before public `Viewer` | fix-now | fixed (e8c959e) |
| S19 | Standards | P1 | `crates/df-web/src/egress/log/page.rs:77` | JS advanced-field list duplicates `advanced_count()` | fix-now | fixed (e8c959e) |
| S20 | Standards | P1 | `crates/df-web/src/egress/log/rows.rs:314-318`, `egress/tests.rs` | `a \|\| b` assertions don't pin output | fix-now | fixed (e8c959e) |
| S18 | Standards | P3 | `crates/df-web/src/egress/allowlist.rs:183` | `is_highlighted` target param coupled to entry | fix-now (user) | fixed (e8c959e) |
| P12 | Spec | P3 | `crates/df-web/src/egress/log/page.rs` togglePause | Pause during reconnect backoff leaves dead loop up to 60 s | - | cosmetic |

User decision: Adjust: fix Blocking + P1 + S18; P12 cosmetic.

## Round 3 (2026-10-25)

- Implementer: e8c959e, tasks 28/28, checks fmt/clippy/machete ok, df-web 285 pass, dark-factory tests pass, container tests ran (podman); Alpine :style merge and resize clamp not browser-verified
- Reviewed range: 8917014...e8c959e

Ledger check: P10, P11, S1, S3, S15–S20 confirmed fixed (e8c959e).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P13 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:41,90-94,242` | Init/resize clamp persists to localStorage, overwriting the remembered width | fix-now | fixed (022c373) |
| S21 | Standards | P1 | `crates/df-web/src/egress/log/filter.rs:48` | `ADVANCED_FIELDS` const mid-file | fix-now | fixed (022c373) |
| S22 | Standards | P1 | `crates/df-web/src/egress/log/filter.rs:87-97` | String match on field names with `_ => &self.task` catch-all | fix-now | fixed (022c373) |
| S23 | Standards | P3 | `crates/df-web/src/egress/log/page.rs:24` | JS array built from Rust Debug output | fix-now (user) | fixed (022c373) |

User decision: Adjust: fix Blocking + P1 + S23 (fix round 3 of 3).

## Round 4 (2026-10-25)

- Implementer: 022c373, tasks 28/28, checks fmt/clippy/machete ok, df-web 286 pass, dark-factory tests pass, container tests ran (podman); remembered-width behaviour not browser-verified
- Reviewed range: e8c959e...022c373

Ledger check: P10, P11, P13, S21–S23 confirmed fixed (022c373).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P14 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:96-102` | setW persists the unclamped input; remembered width can exceed anything shown | fix-now | fixed (2c64224) |
| S24 | Standards | P3 | `crates/df-web/src/egress/log/filter.rs:7,87` | ADVANCED_FIELDS ↔ advanced_count test only one-directional | fix-now (user) | fixed (2c64224) |
| S25 | Standards | P3 | `crates/df-web/src/egress/tests.rs:865` | `assert!(a && b)` combines two facts | fix-now (user) | fixed (2c64224) |

Fix-round limit (3) reached with P14 open. User decision: one more round, fix P14 + S24 + S25.

## Round 5 (2026-10-25)

- Implementer: 2c64224, tasks 28/28, checks fmt/clippy/machete ok, df-web 286 pass, dark-factory tests pass, container tests ran (podman); setW clamp/persist not browser-verified
- Reviewed range: 022c373...2c64224

Ledger check: P10, P11, P13, P14, S24, S25 confirmed fixed (2c64224).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S26 | Standards | P3 | `crates/df-web/src/egress/log/filter.rs:172-175` | Test repeats query-parsing lines | - | cosmetic |

User decision: Finish; write TD-56; S26 cosmetic.

## Result

- Status: done, no blocking finding open
- Tech debt: TD-56 (P6)

## Round 6 (2026-10-25): user feedback after finish

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| U1 | Spec (user) | Blocking | `crates/df-web/src/egress/log/graph.rs:337` | Sandbox node lacks the design's icon (dark rounded tile, green cube) and a visible card (same bg as panel, faint border) | fix-now, spec/design updated first | fixed (a9dfaab) |
| U2 | Spec (user) | Blocking | `crates/df-web/src/components/host_icon.rs` | Host icons: exact-name match only; subdomains of known providers and IP literals get the globe | fix-now: suffix match, Server icon for IP literals | fixed (a9dfaab) |

User decision: implement U1 + U2, then one review round.

- Implementer: a9dfaab, tasks 30/30 (10.1, 10.2 added), checks fmt/clippy/machete ok, df-web 288 pass, dark-factory tests pass, openspec validate --strict ok, container tests ran (podman); card contrast on sunk in light/dark not browser-verified
- Reviewed range: 4f72fc6...a9dfaab

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P15+S28 | Spec+Standards | Blocking | `crates/df-web/src/egress/log/graph.rs:39,340` | Sandbox card border has no colour (currentColor), inconsistent with destinations and D2 | fix-now | fixed (22fb0a4) |
| P17 | Spec | P1 | `crates/df-web/styles/input.css:26-27,65-68` | Dark theme: card darker than sunk canvas, shadow invisible; cards read recessed | fix-now | fixed (22fb0a4) |
| S27 | Standards | P1 | `crates/df-web/src/components/host_icon.rs:67-77` | Event hosts not normalized (case, trailing dot) before brand match | fix-now | fixed (22fb0a4) |
| P16 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs:345` | Task line `text-faint`, docs say muted | fix-now | fixed (22fb0a4) |
| S29 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:609-612` | Weak `rounded-xl` count assertion | fix-now | fixed (22fb0a4) |
| S30 | Standards | P1 | `crates/df-web/src/components/host_icon.rs:80-83` | `trim_matches` accepts unbalanced brackets | fix-now | fixed (22fb0a4) |
| S31 | Standards | P1 | `docs/design-system.md:98` | Icon reference misses Box / Server as IP literal | fix-now | fixed (22fb0a4) |
| P18 | Spec | P1 | `crates/df-web/src/egress/tests.rs` | No table-level IP-literal icon test | fix-now | fixed (22fb0a4) |
| P19 | Spec | P1 | `openspec/changes/network-management-ui-polish/specs/egress-log/spec.md:79` | SHALL text lacks node card/icon/sunk canvas | fix-now | fixed (22fb0a4) |

User decision: Fix blocking + P1.

## Round 7 (2026-10-25)

- Implementer: 22fb0a4, tasks 30/30, checks fmt/clippy/machete ok, df-web 289 pass, dark-factory default tests pass (dev-tools not rerun, no dark-factory change), openspec validate --strict ok, container tests ran (podman); dark-theme card look not browser-verified
- Reviewed range: a9dfaab...22fb0a4

Ledger check: P15+S28, P16–P19, S27, S29–S31 confirmed fixed (22fb0a4). Allowlist validation unchanged (only the error message for malformed brackets).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S32 | Standards | P1 | `crates/df-web/src/components/mod.rs:27`, `crates/df-web/src/egress/allowlist.rs:17,63,67` | Allowlist validation depends on UI component for host normalization | fix-now | fixed (5031da7) |
| S34 | Standards | P1 | `crates/df-web/src/egress/allowlist.rs:527` | Malformed bracket inputs untested on validation path | fix-now | fixed (5031da7) |
| P20 | Spec | P1 | `docs/design-system.md:49-60` | Contrast table lacks dark `raised` text pairs | fix-now | fixed (5031da7) |
| S33 | Standards | P2 | `crates/df-web/src/components/host_icon.rs:67`, `crates/df-gateway/src/policy.rs:147`, `crates/df-gateway/src/proxy.rs:704` | Three host normalizers (UI, stored allowlist, gateway policy) | fix-now (user) | fixed (5031da7) |

User decision: address all four (S32, S34, P20, S33).

## Round 8 (2026-10-25)

- Implementer: 5031da7, tasks 30/30, checks fmt/clippy/machete ok, df-core 39 / df-gateway 71 / df-web 289 pass, dark-factory tests pass, openspec validate --strict ok, container tests ran (podman). Shared `df_core::host` (no trim; one bracket pair); gateway now leaves malformed brackets unstripped
- Reviewed range: 22fb0a4...5031da7

Ledger check: S32, S33, S34, P20 confirmed fixed (5031da7). Gateway bracket change can only deny (both axes).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S36+P21 | Standards+Spec | P1 | `crates/df-gateway/src/proxy.rs:704` | No gateway test for malformed brackets (`[[::1]]` now denied as not-allowlisted) | fix-now | fixed (8d290f4) |
| S35 | Standards | P1 | `crates/df-gateway/src/proxy.rs:36` | `df_core::host` import in wrong group | fix-now | fixed (8d290f4) |
| S37 | Standards | P3 | `crates/df-web/src/egress/allowlist.rs:64`, `crates/df-web/src/components/host_icon.rs:67` | `normalize_host(x.trim())` at two sites | - | cosmetic |
| S38 | Standards | P3 | `crates/df-core/src/host.rs:44` | Duplicate `[[::1]]` assertion | - | cosmetic |

User decision: fix P1 (S36+P21, S35), then finish; orchestrator verifies the diff.

## Round 9 (2026-10-25)

- Implementer: 8d290f4, checks fmt/clippy/machete ok, df-gateway 73 pass, container tests ran (podman)
- Reviewed range: 5031da7...8d290f4 (orchestrator, per user decision): S35 import moved, S36+P21 host_of unit test and CONNECT wire test (malformed brackets → 400/403, never a tunnel; well-formed literal → ip-literal). No new findings.

## Result (after user feedback)

- Status: done, no blocking finding open
- Tech debt: TD-56 (P6) unchanged; no new entries (S33 fixed instead of deferred)
- Browser-verified by the user: see "Browser check" sections below (task 9.7)

## Browser check by the user (2026-10-25, task 9.7, partial)

Checked with `just serve-local` (dark theme). The events table works as expected, including the narrow variant with collapsed columns.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| B1 | Spec | Blocking | `crates/df-web/src/egress/log/graph.rs` | After resizing the split, the edges no longer end at the destination nodes: the gutter SVG had `width:auto` between `left` and `right`, which an SVG does not stretch | fix-now | fixed (open in PR) |
| B2 | Spec | P1 | `crates/df-web/src/egress/log/page.rs` | Filter bar left-aligned; the design aligns it right | fix-now | fixed (open in PR) |
| B3 | Spec | P1 | `crates/df-web/src/egress/log/page.rs` | *Advanced* does not match the design (card with four text fields); the user wants persona as a select by name and only the task besides it | fix-now (spec changed: sandbox and project filters dropped) | fixed (open in PR) |

Fixes: the SVG width is `calc(100% - columns)` and the arrowheads are drawn by the destination nodes. The filter bar is right-aligned in the design's order, and *Clear filters* shows only while a filter is set. *Advanced* is an inline row with a *Persona* select (personas by name) and a *Task* field (any case). Spec "Traffic filters" and "Connection graph" (new scenario "Resized graph") are updated, as are the design and `docs/design-system.md`.

Still open in 9.7: re-check of the three fixes, the light theme, pause/resume, divider keys and reload, and the details panel.

## Browser check by the user, second pass (2026-10-25, task 9.7)

Graph and table work as intended after B1 to B3. Two deviations from the design remained:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| B4 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs`, `page.rs` | The whole graph area was `sunk` (a lighter box under the "Connections" header). The design puts the graph on the panel and the sandbox column in a dashed group box | fix-now | fixed (open in PR) |
| B5 | Spec | P1 | `crates/df-web/src/egress/log/page.rs` | *Advanced* row stacked vertically: its controls used `INPUT_CLASS`, which is `w-full` | fix-now | fixed (open in PR) |

Edge contrast on the panel is recomputed in `docs/design-system.md`.

## Browser check by the user, third pass (2026-10-25, task 9.7)

Group box and filter row as in the design. Two refinements:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| B6 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs` | Sandboxes started at the top while the destination column was taller | fix-now: the shorter column is centred on the taller one, and the group box spans the full content | fixed (open in PR) |
| B7 | Spec | P1 | `crates/df-web/src/egress/log/page.rs` | *Advanced* had no filter icon (design: three lines) | fix-now: `Icon::Filter` (Lucide `list-filter`) | fixed (open in PR) |
| B8 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs` | The sandbox box spanned the full graph height after B6; it should only grow as far as the sandboxes need | fix-now: the box hugs the centred sandboxes and holds its heading | fixed (open in PR) |

## Browser check result (2026-10-25)

Task 9.7 ticked at the user's request after B1 to B8. The user reported the graph, the table (including the narrow variant) and the filters as working; the user confirmed that the stream request ends on *Pause*. The 10,000-event replay (`--demo-flows bulk`) was not checked. TD-56 (no automated runtime test of the page's client behaviour) stays open.

## Row limit (2026-10-25)

The 10,000-event question showed that the table had no row limit: a full backlog sent up to 10,000 rows (about 6 MB) on every filter change, and live rows were never removed. The user chose a configurable cap: a *Rows* select (100, 250, 500, 1000, 2500, 5000; default 500). The server mirrors the trim in the browser, so the close of a dropped open flow is a new row (task 9.8, requirement "Row limit"). The user then ran `--demo-flows bulk` with the limit at 5000: fast load, responsive page, no issues.
