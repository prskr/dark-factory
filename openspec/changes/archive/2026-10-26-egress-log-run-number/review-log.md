# Review log: egress-log-run-number

- BASE: 8d0d16c1d608ebff08c6258988f2632df09cf6e8
- Store: none
- Status: done
- Implementer agent: ad08e16c4eb0615cd

## Round 1 (2026-10-25)

- Implementer: a42d3a2, tasks 6/6, checks fmt/clippy/machete/openspec validate ok, workspace tests pass except 4 `dark-factory` bin tests (`PoolTimedOut` in df-test-postgres; environmental suspicion, ENOSPC during session), container tests ran (podman)
- Reviewed range: BASE...a42d3a2

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs:682` | "Node content" scenario: no rendered test shows `>#2<` over `>DF-7<`; render test uses run_number 0 | fix-now | fixed (b976592) |
| P2 | Spec | P1 | `crates/df-gateway/src/policy.rs:139` | `sandbox_from_entry` copy of `run_number` untested (code verified correct) | fix-now | fixed (b976592) |
| S1 | Standards | P1 | `crates/df-web/src/egress/log/rows.rs:80-88` | private `has_run_number`/`short_key` placed between `pub(super)` fns (AGENTS.md file structure) | fix-now | fixed (cd375b9) |
| P3 | Spec | P1 | `tasks.md` 2.2 | `just serve-local` check ticked but not done | fix-now | fixed (b976592) |
| S2 | Standards | P3 | `crates/df-web/src/egress/log/rows.rs:91-107` | possible Duplicated Code: numbered-or-not branch in `sandbox_label`/`sandbox_lines`/`row` | cosmetic | cosmetic |
| S3 | Standards | P3 | `crates/df-gateway/src/proxy.rs:606-615` | possible Data Clumps: 5-tuple destructure of sandbox fields (pre-existing, extended) | cosmetic | cosmetic |

User decision: Fix blocking + P1

Recheck by orchestrator: the 4 `dark-factory` bin tests (see Implementer line) rerun with podman socket.
- Result: still `PoolTimedOut`; a df-db container test (`active_runs`, passed earlier) now fails the same way -> environment (shared podman test PostgreSQL), not the change. CI covers them.

## Round 2 (2026-10-25)

- Implementer: b976592, tasks 5/6 (2.2 unchecked: serve-local check not done, PostgreSQL unavailable), checks fmt/clippy/machete ok, df-web `egress::log` + df-gateway `policy` tests pass, container tests skipped (environment)
- Reviewed range: a42d3a2...b976592
- Round 1 rows: P1, P2, S1 fixed (b976592); P3 resolved by unchecking 2.2 with a note (b976592)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `crates/df-web/src/egress/log/rows.rs:111-118` | partially fixed: helpers still before `duration_label`, `bytes_label`, `rule_href`, `row` | fix-now | fixed (cd375b9) |

Round 1 row status: P1 fixed (b976592), P2 fixed (b976592), P3 fixed (b976592, 2.2 unchecked with note; serve-local check still open), S1 partial (see above), S2/S3 cosmetic.

User decision: Fix blocking + P1

## Round 3 (2026-10-25)

- Implementer: cd375b9, tasks 5/6, checks fmt/clippy/machete ok, df-web `egress::log` tests pass, container tests skipped (environment)
- Reviewed range: b976592...cd375b9, verified by orchestrator: pure move of two functions (`git diff --color-moved`, no non-moved lines); reviewers not spawned
- S1: fixed (cd375b9)

## Result

- No blocking findings open. Tech debt: none (no P2 findings).
- Task 2.2: `just serve-local` check done manually by the user (2026-10-26); tasks 6/6.
- Container tests not run in this environment (podman test PostgreSQL broken, `PoolTimedOut`): all df-db/df-web/dark-factory container-backed tests after round 1; CI covers them.

## Round 4 (2026-10-26): scope extension

- User found: below 520 px panel width the graph gutter collapses; below ~460 px sandbox and destination nodes overlap. Cause: `graph::render` lays out for a fixed 520 px.
- User decision: fold into this change. Artifacts updated (spec scenario "Narrowest graph", design D4, task 2.3).
- Implementer: a8cbb2c (task 2.3), checks fmt/clippy/machete ok, df-web `egress::log` 31 pass, container tests skipped (environment); browser check at 397 px not done (PostgreSQL unavailable)
- Reviewed range: 87039e5...a8cbb2c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:21` | `MIN_GRAPH_PANEL = MIN_WIDTH + 1` counts one border side; 397 px panel leaves a 395 px graph, gutter 59 px < 60 ("Narrowest graph"); verified | fix-now | fixed (afe4528411b96c16658429a3af958da8e7f12f5c) |
| P5 | Spec | P1 | `graph.rs:624`, `tasks.md` 2.3 | no test ties panel minimum, border and 276 offset; 2.3 ticked though browser check open | fix-now | fixed (afe4528411b96c16658429a3af958da8e7f12f5c) |
| S6 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:267-285` | `Layout.destination_width` (and `width`) only read by tests now; misleading next to the CSS width | fix-now | fixed (afe4528411b96c16658429a3af958da8e7f12f5c) |
| S7 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:39,244-248,283-285` | offset rule `SANDBOX_X+SANDBOX_W+MIN_GUTTER+RIGHT_PAD` written three times | fix-now | fixed (afe4528411b96c16658429a3af958da8e7f12f5c) |
| S8 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:240-248` | private `destination_width_css()` between pub(super) items | fix-now | fixed (afe4528411b96c16658429a3af958da8e7f12f5c) |
| S9 | Standards | P3 | `crates/df-web/src/egress/log/graph.rs:622-655` | SVG width asserted twice across tests | cosmetic | cosmetic |
| S10 | Standards | P3 | `crates/df-web/src/egress/log/graph.rs:624` | test not seen failing first (TDD process deviation); asserts do discriminate old/new | cosmetic | cosmetic |

User decision: Fix blocking + P1. Orchestrator updated artifacts: scenario and D4 to 398 px, task 2.3 unticked (browser check open).

## Round 5 (2026-10-26)

- Implementer: afe4528411b96c16658429a3af958da8e7f12f5c, tasks 6/7 (2.3 open: browser check), checks fmt/clippy/machete ok, df-web `egress::log` 32 pass (new P5 test seen failing first), container tests skipped (environment)
- Reviewed range: be507e6...afe4528411b96c16658429a3af958da8e7f12f5c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P6 | Spec | Blocking | `crates/df-web/src/egress/tests.rs:851` | container-backed test still asserts `aria-valuemin="397"`; page emits 398 → fails (verified) | fix-now | fixed (26be721) |
| S12 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:275` | gutter line re-spells `SANDBOX_X + SANDBOX_W + RIGHT_PAD` beside `DEST_OFFSET` | fix-now | fixed (26be721) |
| S11/P7 | Both | P3 | `crates/df-web/src/egress/log/page.rs:335-342` | new panel test is constant arithmetic (true by definition, not tied to the `border` class); it did fail against `+ 1`, so it guards P4 | cosmetic | cosmetic |
| S13 | Standards | P3 | `crates/df-web/src/egress/log/graph.rs:517-520` | expected gutter repeats the implementation's arithmetic | cosmetic | cosmetic |

Round 4 rows: P4, P5, S6, S7, S8 fixed (afe4528411b96c16658429a3af958da8e7f12f5c).

User decision: Fix blocking + P1

## Round 6 (2026-10-26)

- Implementer: 26be721, checks fmt/clippy/machete ok, df-web `egress::log` 32 pass, container tests ran: `cargo test -p df-web egress` 86 pass (PostgreSQL works again)
- Reviewed range: afe4528411b96c16658429a3af958da8e7f12f5c...26be721, verified by orchestrator (two lines: 397 → 398 in the test; gutter derived from `DEST_OFFSET`, algebraically equal); reviewers not spawned
- P6: fixed (26be721). S12: fixed (26be721).
- Orchestrator: `cargo test --workspace` with podman socket passes (incl. the 4 `dark-factory` tests and df-db container tests that hit `PoolTimedOut` earlier).

## Result (2026-10-26)

- No blocking findings open. Tech debt: none (no P2 findings).
- Task 2.3: browser check done by the user (2026-10-26); tasks 7/7.
