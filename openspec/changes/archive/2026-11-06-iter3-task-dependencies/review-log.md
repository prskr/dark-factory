# Review log: iter3-task-dependencies

- BASE: 29d51d72863735413adb7a111a4bc581ff9c2c6d
- Store: none
- Status: done
- Implementer agent: implementer (spawned 2026-11-05)

## Design (2026-11-05)

- User chose "Design first in PenPot" for the change's UI (creator, parent link, "Waiting for" list, `awaiting dependencies` chip, persona description). Roadmap boards (Sub-tasks, Artifacts tabs) stay on `Tasks - Roadmap`: they belong to `iter3-task-tree` / `iter3-artifacts`.
- Approved design (after two iterations):
  - `Status badge` variant `Awaiting dependencies` (Queued tone).
  - `Tasks - Details Sidebar / Desktop · awaiting dependencies` and `Desktop · dependencies collapsed`. Now, Links and Branch as the app renders them; Created by and Parent rows after Branch; a collapsible "Dependencies" section between the properties (and Description) and Runs, with groups "Waits for" (dependencies, satisfied or waiting) and "Needed by" (dependants), open while the task is blocked, collapsed otherwise and kept after release.
  - A collapsed "Dependencies" row added to every other task board with a details sidebar or details sheet (Tasks, Tasks - Status, Tasks - Queue, Tasks - Details Sidebar, Tasks - Phone).
  - Persona Description field (counter `n / 500`) in all four persona edit boards; descriptions on `Settings · personas`. Outdated `Personas · cards` (2 columns, app renders 3 at `xl`) deleted.
- Spec deltas the design requires (to fold in with `/opsx:update` before implementation), `task-ui` "Dependencies in the task details":
  - the section stays after release (collapsed), instead of disappearing;
  - a "Needed by" group lists the dependants (key linked, kind, title, status);
  - open while `awaiting_dependencies`, collapsed otherwise; hidden when the task has neither dependencies nor dependants (assumption, as Runs).
- Status: stopped after design for `/opsx:update`; spec revised in 8116ade8, run resumed.

## Round 1 (2026-11-05)

- Implementer: 4b899270..be98758a (HEAD_1 be98758a), tasks 21/21, checks: fmt, clippy, machete, check-file-length, offline build, `just docs-build` ok; cargo tests green (df-core, df-db, df-agent-channel, df-web, fake-acp-agent, df-e2e seed), not re-run after the last small edits; Bazel `//...` 49/50, the failing `agent_port_test` fixed and only `//crates/df-agent-channel/...` re-run. Container tests ran (podman). Skipped: `#[ignore]` df-e2e scenarios incl. `tests/delegation.rs` (needs `just e2e-local`); 6.4 browser check at 1280 px only via accessibility snapshots, phone details sheet not checked, no screenshots.
- Implementer deviations: `created_by` column DEFAULT (system principal "unknown") for raw-SQL test inserts; `complete_on_merge_tx` routed through the applier as `MarkedDone(Merge)`; `RunEnded` carries `stop_requested_at`; title source `agent` derived from the creator; `Transition.touch` flag; migration backfills `first_*_at`; `when` step in `fake-acp-agent`; seeded project `DG`; TDD not strict in `df-core` transitions; some shell edits; mixed commit trailers.
- Reviewed range: BASE...be98758a
- Orchestrator check: `bazel test //...` with `--test_env=DOCKER_HOST=…` 50/50 pass.
- Security: 0 findings (no verify agents needed).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `crates/df-core/src/task_transition.rs:428` | `run_ended` with outcome `completed` emits `RecordFirst(Completion)`, `NotifyDependents`, `NotifyParent(Completed)` even when the task is already `cancelled` (releases completion dependents of a cancelled task) or `completed` (second parent message after mark done) | fix-now | fixed (0a9414ed) |
| SP2 | Spec | Blocking | `crates/df-db/src/webhooks.rs:95` | Scenario "child completes, later PR merged → parent gets two messages" has no test | fix-now | fixed (0a9414ed) |
| SP3 | Spec | Blocking | – | Scenario "PR closed without merge → `done` dependency stays unsatisfied" has no test | fix-now | fixed (0a9414ed) |
| SP7 | Spec | Blocking | `docs/content/architecture/agent-runs.md:99` | "Agent tools" says calls are scoped to the run's task, without the `create_task` / `send_message` exception | fix-now | fixed (0a9414ed) |
| S5 | Standards | P1 | `crates/df-db/src/task_events.rs:261,347`, `df-core` `is_satisfied` | Satisfaction rule duplicated: two SQL copies plus the Rust function | fix-now | fixed (0a9414ed) |
| S6/SP4 | Standards+Spec | P1 | `migrations/20261105000004_task_dependencies.sql:24` | `created_by` DEFAULT (system "unknown") weakens D1 for test convenience | fix-now | fixed (0a9414ed) |
| SP6 | Spec | P1 | task 6.4 | Phone details sheet not checked, no screenshots | fix-now | fixed (0a9414ed: checked at 390 px via DOM measurements, no discrepancy; no 390 px board exists; no screenshots) |
| S3 | Standards | P1 | `crates/df-db/src/tasks/dependency_tests.rs:475` | `wait_for_a_lock_wait` busy-loops without timeout | fix-now | fixed (0a9414ed) |
| S7 | Standards | P1 | `crates/df-db/src/tasks.rs` | `unsatisfied_dependencies` returns all targets; name misleads | fix-now | fixed (0a9414ed) |
| S8 | Standards | P1 | `crates/df-db/src/task_events.rs` `load_snapshot` | `active_run` string match, unknown falls through to `None` | fix-now | fixed (0a9414ed) |
| S9 | Standards | P1 | `crates/df-db/src/tasks.rs` `create_task_tx` | Followups of `execute_effects` dropped silently | fix-now | fixed (0a9414ed) |
| S14 | Standards | P1 | `crates/df-agent-channel/src/tools/delegation.rs` | Hand-built text block duplicates `df_db::tasks::text_block` | fix-now | fixed (0a9414ed) |
| SP8 | Spec | P1 | `specs/task-ui/spec.md:6` | Waiting group list disagrees with task-management (missing `blocked`, `in_review`) | fix-now | fixed (0a9414ed) |
| SP5 | Spec | P2 | `crates/df-e2e/tests/delegation.rs` | e2e scenario never run (needs `just e2e-local`); recorded here | record | fixed (a3da951e) |
| S10 | Standards | P2 | `df-db` `tasks.rs` ↔ `task_events.rs` | Circular helper coupling | tech-debt | tech-debt (TD-116) |
| S11 | Standards | P2 | `crates/df-db/src/tasks/{tests,dependency_tests}.rs` | 2,015 test lines as siblings; should be `tasks/tests/` dir | tech-debt | tech-debt (TD-117) |
| S13 | Standards | P2 | `df-web/src/testkit.rs`, `df-db` `set_status_for_test` | Duplicate test status helper | tech-debt | tech-debt (TD-118) |
| S1 | Standards | P3 | `task_transition.rs` `child_ended_text` | Public fn after private helpers | – | cosmetic |
| S2 | Standards | P3 | `dependency_tests.rs:302` | Second `impl Env` block | – | cosmetic |
| S12 | Standards | P3 | `df-core/src/domain.rs` | Test name reads as topic | – | cosmetic |
| S15 | Standards | P3 | `task_views.rs` | `Dependant` vs `dependent` spelling | – | cosmetic |
| S4 | Standards | Rejected | – | Bulk shell edits: process note, nothing to fix in code | | rejected (not actionable) |

User decision: Fix blocking + P1 (SP1, SP2, SP3, SP7, S5, S6/SP4, SP6, S3, S7, S8, S9, S14, SP8). S10, S11, S13 → tech debt; SP5 recorded here (e2e not run, needs `just e2e-local`); S1, S2, S12, S15 cosmetic.

## Round 2 (2026-11-05)

- Implementer: 0a9414ed, checks: fmt, clippy, machete, check-file-length, `just docs-build` ok; cargo tests df-core, df-db, df-agent-channel, df-web, df-gateway green; Bazel for those crates 14/14. Full `bazel test //...` not finished (disk full, ENOSPC); `dark-factory` and df-e2e not re-run with cargo. Shell one-liner edits again.
- Reviewed range: be98758a...0a9414ed
- Security: 0 findings. Spec: all round-1 Spec rows confirmed fixed. Standards: S3, S5–S9, S14 confirmed fixed.
- SP6 detail: the persona edit form was also measured at 390 px (no overflow, textarea 356 px, counter "42 / 500"), not compared with the `Config - Personas` phone edit sheet board; no 390 px board exists for an `awaiting_dependencies` task.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP9 | Spec | Blocking | `crates/df-core/src/task_transition.rs:429` vs design D2, specs task-dependencies / task-delegation, `docs/content/using/delegation.md` | SP1 fix added two rules no artifact states: a completed run end on a `cancelled` task is no Completion; a Completion of a Done child queues no parent message | | open |
| S16 | Standards | P1 | `crates/df-db/src/tasks/dependency_tests.rs:551` | Third `impl Env` block | | open |
| S17 | Standards | P1 | `crates/df-db/src/webhooks/tests.rs:480` | `add_child_dependent` creates no child | | open |
| S18 | Standards | P1 | `crates/df-db/src/webhooks/tests.rs:386` | `add_child` `depends_on` param always empty | | open |
| S19 | Standards | P1 | `webhooks/tests.rs`, `tasks/dependency_tests.rs` | `child_ended_messages` and completed `RunEnded` literal duplicated | | open |
| S20 | Standards | P1 | `crates/df-db/src/lifecycle_events/tests.rs:218` | `seed_with_creator(.., bool)` flag argument + `format!` SQL | | open |
| S21 | Standards | P1 | `crates/df-db/src/task_events.rs:324,366` | Two identical `is_satisfied` loops | | open |
| S22 | Standards | P1 | `crates/df-db/src/tasks.rs:155` | `pub fn text_block` without doc comment | | open |
| SP10 | Spec | Rejected | review log | Persona form at narrow viewport not stated: resolved by the SP6 detail line above | – | rejected (ledger updated) |

User decision: Fix blocking + P1 (SP9 by documenting the rules, code unchanged; S16–S22).

## Round 3 (2026-11-06)

- Implementer: b0a2b602 (check report pending at review time).
- Reviewed range: 0a9414ed...b0a2b602
- Security: 0 findings. SP9, S16–S22 confirmed fixed (b0a2b602).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP12 | Spec | P1 | `crates/df-core/src/task_transition/tests.rs:700` | Done-then-completed test only asserts the missing `NotifyParent`; nothing pins that `RecordFirst(Completion)` and `NotifyDependents` still run | | open |
| SP11 | Spec | P1 | `design.md:118` | "(no effects)" overstates: `TaskChanged` is still published | | open |
| S23 | Standards | P1 | `crates/df-db/src/tasks/tests.rs` (1,496 lines, now `pub(crate) mod tests`) | Crate-shared helpers make the whole test module `pub(crate)` and push the file to the limit | fix-now | open |

User decision: "Fix all P1 + S23" (SP11, SP12, S23 moved from P2 to fix-now).

## Round 4 (2026-11-06)

- Implementer: ccf5d5f9 (no check report received; orchestrator ran the checks).
- Orchestrator checks on ccf5d5f9: fmt, clippy `-D warnings`, check-file-length, check-test-names, machete, `openspec validate`, `just docs-build` ok; `bazel test //...` (with `--test_env=DOCKER_HOST`) 50/50; `cargo test -p dark-factory -p df-e2e` ok after `just plugins` (the wasm plugins were missing after `cargo clean`).
- Reviewed range: b0a2b602...ccf5d5f9. Security, Standards, Spec: 0 findings. SP11, SP12, S23 confirmed fixed (ccf5d5f9).
- Round 3 rows: SP11 fixed (ccf5d5f9), SP12 fixed (ccf5d5f9), S23 fixed (ccf5d5f9); remaining part tracked with S11 as TD-117.
- Round 2 rows: SP9, S16–S22 fixed (b0a2b602).

## Finish (2026-11-06)

- Tech debt: TD-116 (S10), TD-117 (S11, S23), TD-118 (S13). Deferred work already in the roadmap backlog ("Dependencies set by humans", "Start now").
- No `Cargo.toml` / `Cargo.lock` change: Bazel lockfile build not needed.
- SP5: user chose to run `just e2e-local` before finishing. Run on ccf5d5f9: demo 3/3, flow 9/9, lifecycle_events 1/1, webhooks 1/1 pass, but `delegation` was not run.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP13 | Spec | Blocking | `deploy/e2e/e2e-local.sh:242`, `docs/demo.md:36,90,99,159` | The e2e runner lists the test binaries by name and omits `--test delegation`: task 8.1's scenario never runs | fix-now | fixed (a3da951e) |

User decision: one extra fix round beyond the limit of 3 for SP13 (and SP5).

## Round 5 (2026-11-06)

- Implementer (fresh agent; the first stopped delivering reports): a3da951e. `--test delegation` added to the runner and `docs/demo.md`. The first run failed on a test bug: "Waits for" is inside the Dependencies disclosure, which is closed once the task left `awaiting_dependencies` (as specified); the test now opens it (`Session::open_dependencies`). Second full `just e2e-local`: delegation 1/1, demo 3/3, flow 9/9, lifecycle_events 1/1, webhooks 1/1; cluster deleted. fmt, clippy, check-file-length, machete, docs-build ok. Bazel not re-run: only the cargo-only `df-e2e` crate changed.
- Reviewed range: ccf5d5f9...a3da951e, by the orchestrator (test-only and runner/docs change, 3 files): no findings.
- SP5: fixed (a3da951e, scenario ran and passed).
- Status: done. No blocking finding open; all container-backed tests and the e2e suite ran.
