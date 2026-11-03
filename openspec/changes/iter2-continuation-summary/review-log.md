# Review log: iter2-continuation-summary

- BASE: fc60cee653103089c835bffd9c33695efeed0c67
- Store: none
- Status: done
- Implementer agent: ae8806c9c19dea435 (rework)

## Round 1 (2026-11-03)

- Implementer: 165c630, tasks 16/16, checks fmt/clippy/machete ok, df-agent-channel tests pass (lib 185, integration 28); `.sqlx` unchanged (runtime queries only); deviation from D2: stored text holds entries only, header rendered at read time
- Container tests: ran (df-agent-channel DB tests); df-db tests and Bazel test targets not run
- Reviewed range: fc60cee...165c630

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1+SP5 | Spec | Blocking | `summary.rs:265`, `session.rs:110` | `Assign` on a mid-run reconnect folds the active run's events and advances the cursor; a later `tool_call_update` is dropped (stale status), agent message splits; no run-end→Assign→run-end test | fix-now | fixed (57913b4) |
| SP3 | Spec | Blocking | `summary/tests.rs:298` | Scenario "Run without a stored summary": no assertion that the row is stored | fix-now | fixed (57913b4) |
| S1 | Standards | P1 | `summary.rs:32-238` | Private helpers before public items (AGENTS.md file structure) | fix-now | fixed (57913b4) |
| SP2 | Spec | P1 | design.md D2/D7, `design/agent-run.md` | Docs say the header is stored; implementation stores entries only | fix-now | fixed (57913b4) |
| SP4 | Spec | P1 | `summary/tests.rs` | Scenario "Old history survives the size cap" only tested with 20 events, not >500 across runs | fix-now | fixed (57913b4) |
| S3 | Standards | P1 | `summary.rs:60-72,137-141,176` | Event kind string matched in three places; parse once into enum | fix-now | fixed (57913b4) |
| S7 | Standards | P1 | `session.rs:247` | Warning lacks `task_id` field | fix-now | fixed (57913b4) |
| S2 | Standards | P3 | `summary.rs:24,56,89,258` | Visibility wider than needed | cosmetic | cosmetic |
| S4 | Standards | P3 | `summary/tests.rs:64` | `CAP` const after helpers | cosmetic | cosmetic |
| S5 | Standards | P3 | `summary/tests.rs:75` | Magic `2` instead of `SEPARATOR.len()` | fix-now (user) | fixed (57913b4) |
| S6 | Standards | P3 | `session/tests.rs:1209-1257` | `.unwrap()` vs `Result` in tests; matches file style | cosmetic | cosmetic |
| X* | Security | — | — | No findings | — | — |

User decision: fix blocking + P1, plus S5 (SP1, SP3, S1, SP2, SP4, S3, S7, S5). S2, S4, S6 cosmetic.

## Round 2 (2026-11-03)

- Implementer: 57913b4 + abfbf9f, tasks 16/16, checks fmt/clippy/machete ok, df-agent-channel lib 187, integration 28 pass
- Container tests: ran (df-agent-channel DB tests); df-db tests and Bazel test targets not run
- Reviewed range: 165c630...abfbf9f

Ledger check: S1, S3, S5, S7, SP2, SP3, SP4 fixed. SP1+SP5 fixed for the reconnect case, regression test verified by reviewer to fail on old code; edge case split out as SP6.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP6 | Spec | Blocking | `summary.rs:97-121` | Assign catch-up filters out only the current run's rows; a late batch of an ended run (documented in `lock_run_and_task`) committed after run 2's events moves the cursor past them on a reconnect Assign, and run 2's events never enter the summary. Doc comment "runs are sequential" is wrong. | fix-now | fixed (cdf43d5) |
| SP7 | Spec | P1 | `specs/run-orchestration/spec.md:3` | Spec says Assign folds "any events after that id"; code excludes the run being assigned | fix-now | fixed (cdf43d5) |
| S8 | Standards | P1 | `session/tests.rs:1270` | Test closure duplicates `text_event()` helper | fix-now | fixed (cdf43d5) |
| S9 | Standards | P3 | `summary.rs:105` | Kind strings repeated in SQL filter and `EventKind::parse` | cosmetic | cosmetic |
| X* | Security | — | — | No findings | — | — |

User decision: fix blocking + P1 (SP6, SP7, S8). S9 cosmetic.

## Round 3 (2026-11-03)

- Implementer: cdf43d5, tasks 16/16, checks fmt/clippy/machete ok, df-agent-channel lib 188, integration 28 pass; SP6 test seen failing before the fix
- Container tests: ran (df-agent-channel DB tests); df-db tests and Bazel test targets not run
- Reviewed range: abfbf9f...cdf43d5

Ledger check: SP6, SP7, S8 fixed (reviewer verified SQL bounds, task-lock id ordering, forward-only upsert; late-batch test fails on old code).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP8 | Spec | P1 | `spec.md:4` | No scenario for a late batch of an ended run reaching the summary (DB test exists) | fix-now | fixed (ff9c9a1) |
| SP9 | Spec | P1 | `design.md:38,41` | D5/D6 say late batches fold at next run end; also at next `Assign` when the run ends without report | fix-now | fixed (ff9c9a1) |
| SP10 | Spec | P1 | `design.md:26` | D3 query text lacks the `Assign` upper bound | fix-now | fixed (ff9c9a1) |
| S10 | Standards | P3 | `summary.rs:107` | Bare `9223372036854775807` sentinel in SQL | cosmetic | cosmetic |
| X* | Security | — | — | No findings | — | — |

User decision: fix blocking + P1 (SP8, SP9, SP10). S10 cosmetic.

## Round 4 (2026-11-03)

- Implementer: ff9c9a1 (docs only), `openspec validate` ok
- Reviewed range: cdf43d5...ff9c9a1, checked by orchestrator (docs-only diff, no reviewer spawn); SP8, SP9, SP10 fixed, no new findings

## Finish (2026-11-03)

- Container tests: `cargo test -p df-db` 124 pass; `bazel --bazelrc=.bazelrc.agents test //crates/... //plugins/...` 36/36 pass (DOCKER_HOST podman socket). Earlier rounds: df-agent-channel DB tests ran each round.
- Bazel lockfile build: not needed (no Cargo.toml/Cargo.lock change).
- Tech debt: none (no P2 findings). Deferred work: F3 compaction plugins already in roadmap backlog.
- Result: fixed SP1+SP5, SP3, SP6, SP2, SP4, SP7, SP8, SP9, SP10, S1, S3, S5, S7, S8; cosmetic S2, S4, S6, S9, S10; rejected none.

## Rework (2026-11-03)

The first implementation was squashed into afbbf0c. After review, the change was revised with `/opsx:update`:
- the stored summary (`task_summaries`, cursor, update at run end, catch-up in `Assign`) is dropped; `Assign` rebuilds the summary from all transcript events except the assigned run's
- consecutive successful tool calls collapse to `Tool: <n> calls completed`
- the cap is 64 KiB; tool lines and agent messages are dropped before user messages

Findings above refer to the stored-summary design; SP1, SP6 and their tests are superseded by design D1 (rebuild without the assigned run).

- BASE (rework): fc60cee (round 1 reviews the whole change against `main`)
- Next step: implement the open tasks of tasks.md (1.3–1.5, 2.1–2.2, 3.1–3.2, 4.1, 4.2, 4.4)

## Rework round 1 (2026-11-03)

- Implementer: 7d6b73c, tasks 13/13, checks fmt/clippy/machete ok, df-agent-channel 190 lib + 28 integration, df-db 124 pass
- Container tests: ran (df-agent-channel DB tests, df-db)
- Reviewed range: fc60cee...7d6b73c (whole change against main)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R-S2+R-SP1 | Standards/Spec | P1 | `summary.rs:112-133`, `summary/tests.rs` | SQL rebuilds the camelCase payload so the fold can reuse JSON pointers (two places to edit per field); no test feeds real serialized tool events through the query (key names verified to match df-web usage, so no current bug) | fix-now | fixed (1595159) |
| R-S1 | Standards | P1 | `summary.rs:141,160,250-256` | Tool status as string with `""` sentinel; prefix stripping duplicated from `metrics.rs`/`df-web`; use `ToolCallStatus` | fix-now | fixed (1595159) |
| R-SP2 | Spec | P1 | `summary/tests.rs:192` | Scenario "User messages kept at the cap" never shows the older agent message dropped before the user message | fix-now | fixed (1595159) |
| R-SP3 | Spec | P1 | `summary.rs:293`, `spec.md` | Newest-entry rule overrides user priority; untested and not stated in spec | fix-now | fixed (1595159) |
| R-S3 | Standards | P1 | `summary.rs:112` | `EVENTS_QUERY` const after functions | fix-now | fixed (1595159) |
| R-S4 | Standards | P1 | `summary.rs:280` | Stale comment ("batch") from cursor design | fix-now | fixed (1595159) |
| R-S5 | Standards | P1 | `summary/tests.rs:401` | Test name describes removed 500-row window | fix-now | fixed (1595159) |
| R-SP4 | Spec | P1 | `product/context.md:159` | F3 output lacks "entries text, no header" | fix-now | fixed (1595159) |
| R-S6 | Standards | P3 | `summary/tests.rs:331-372`, `session/tests.rs:1213` | Duplicated Assign-of-run-2 test setup | cosmetic | cosmetic |
| R-SP5 | Spec | P3 | `summary.rs:230` | No test with FACTORY origin | cosmetic | cosmetic |
| R-X* | Security | — | — | No findings | — | — |

User decision: fix blocking + P1 (R-S2+R-SP1, R-S1, R-SP2, R-SP3, R-S3, R-S4, R-S5, R-SP4). R-S6, R-SP5 cosmetic.

## Rework round 2 (2026-11-03)

- Implementer: 1595159, checks fmt/clippy/machete ok, df-agent-channel 192 lib + 28 integration pass
- Container tests: ran (df-agent-channel DB tests); df-db unchanged this round
- Reviewed range: 7d6b73c...1595159

Ledger check: all 8 fix-now items fixed. Hidden-block filter in SQL verified by spec and security reviewers (at least as strict as before; tested with real serialized events).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R-S7 | Standards | P1 | `summary.rs:120` | Query row is an 8-tuple of mostly `String`; swapped SQL columns compile silently | fix-now | fixed (c788ef0) |
| R-S8 | Standards | P1 | `summary.rs:23-37,252` | `title` uses `""` for absent (SQL `COALESCE ''` + empty filter) | fix-now | fixed (c788ef0) |
| R-S9 | Standards | P3 | `summary/tests.rs:40-60,91` | Test helpers take status as a string name | cosmetic | cosmetic |
| R-X* | Security | — | — | No findings | — | — |

User decision: fix blocking + P1 (R-S7, R-S8). R-S9 cosmetic.

## Rework round 3 (2026-11-03)

- Implementer: c788ef0, checks fmt/clippy/machete ok, df-agent-channel 192 lib + 28 integration pass (DB tests ran)
- Reviewed range: 1595159...c788ef0, checked by orchestrator (mechanical refactor: named `EventRow`, `title: Option<String>`); R-S7, R-S8 fixed, no new findings

## Finish (rework, 2026-11-03)

- Container tests: ran every round (df-agent-channel DB tests; df-db 124 pass in rework round 1, unchanged after). Bazel test suite not rerun after the rework (no Cargo/BUILD/MODULE change).
- Bazel lockfile build: not needed (no Cargo.toml/Cargo.lock change).
- Tech debt: none (no P2). Deferred work: stored summary with cursor and model-based compaction are F3, already in the roadmap backlog.
- Result: fixed R-S1, R-S2+R-SP1, R-S3, R-S4, R-S5, R-S7, R-S8, R-SP2, R-SP3, R-SP4; cosmetic R-S6, R-S9, R-SP5; rejected none.
