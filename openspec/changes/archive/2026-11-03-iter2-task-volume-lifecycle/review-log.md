# Review log: iter2-task-volume-lifecycle

- BASE: 715e2e8
- Store: none
- Status: done (2 rounds; no new tech debt; Bazel build `--lockfile_mode=error` ok)
- Implementer agent: stateless `implementer` subagent per run (VS Code); tasks in batches
- DOCKER_HOST: unix:///var/run/docker.sock
- Design tasks: 3.1 (PenPot, orchestrator), approved
  - Boards: Tasks / "Desktop · reset workspace", "… confirmation", "… while running"; Tasks - Phone / "Phone · reset workspace", "… confirmation", "… while running"
  - User decisions: item in the Status menu (✓ status · Mark done · Reset workspace… · Cancel task…), neutral colour, danger only on the confirm button; confirmation in two paragraphs ("Lost for good: …" separate); while a run is active the item is disabled with the hint "Stop the run first"

## Implementation runs

| Batch | Tasks | Commits | Notes |
|---|---|---|---|
| 1 | 1.1, 1.2 | 05330d0, 3ae0370 | df-k8s DELETE with 5 s API timeout, 404 succeeds; marker cleared in `Scheduler::provision` |
| 2 | 2.1 | 82719f0 | Scan runs on the Kubernetes Lease leader (scheduler has no election); keys `volumes.retainAfterCloseSeconds` / `volumes.idleTtlSeconds` (spec says `retainAfterClose` / `idleTtl`); tasks without runs never qualify; one Helm unit test removed to stay under 1,500 lines |
| 3 | 3.2, 4.1 | 103d913, d5b7649 | Shared `df_db::volumes`; `WebBackends` struct; Status menu now also on `completed` tasks; plain (unchecked) queries; user guide in `crates/df-web/README.md` |

### Visual comparison (orchestrator, 390 and 1440 px)

V1 (major): phone details sheet is ~422 px tall, so the Status menu starts at y=757 and "Reset workspace…" / "Cancel task…" are below the viewport. V2 (minor): second confirmation paragraph muted (#57534e) instead of `color.text`, paragraphs 500 instead of 400, phone 14 px instead of 15 px. V3 (minor): a provider failure on reset shows the full "Error" page instead of feedback in place (seen locally without a cluster, 500). Minor/cosmetic: desktop dialog 448 vs 480 px with border, menu row heights/gaps/insets, phone menu 192 vs 220 px, backdrop 50 vs 40 %, danger button border.

## Round 1 (2026-11-02)

- Implementer: d5b7649, tasks 7/7, checks fmt/clippy/machete ok, container tests ran
- Reviewed range: 715e2e8...d5b7649
- Security: no findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `df-agent-channel/src/retention.rs:62` | `select!` drops a scan mid `delete_task_volume` on shutdown (not cancel-safe); doc claims a rollback | | open |
| P2+V1 | Spec/Visual | Blocking | `df-web/src/task_pages/view.rs:582` | Phone: Status menu opens below the viewport, reset and cancel items not visible | | open |
| S2 | Standards | P1 | `deploy/helm/dark-factory/tests/chart_test.yaml` | File at 1,494/1,500 lines; `idleTtlSeconds` override not asserted; split by topic | | open |
| P4+S9+V3 | Spec/Standards | P1 | `df-web/src/task_pages.rs:572` | Provider failure or vanished task renders the full error page; 404 for `NotFound`, inline error for provider failure | | open |
| P3 | Spec | P1 | `df-web/src/task_pages/tests/reset_workspace.rs:29` | Reset → next run recreates the volume untested end to end | | open |
| S3 | Standards | P1 | `df-db/src/volumes.rs:36,63` | Static queries unchecked; use `query!`/`query_scalar!` | | open |
| S5 | Standards | P1 | `df-agent-channel/src/retention/tests.rs:241` | Gated provider fake copied from scheduler tests | | open |
| S7, S8, S10 | Standards | P1 | `reset_workspace.rs` tests, `retention.rs:42` | 404 check inside the preparing test; audit count query repeated; unused `Clone` derive | | open |
| V2 | Visual | P1 | `reset_dialog` | Second paragraph muted and 500 weight; design: `color.text`, 400, phone 15 px | | open |
| P1 | Spec | P1 | spec/proposal/design/tasks | Config keys are `…Seconds` in code; artifacts say `retainAfterClose`/`idleTtl` (orchestrator) | | open |
| P5 | Spec | P1 | `design.md` D1 | Scan runs on the Lease holder, marker cleared after `ensure_task_volume` in `Scheduler::provision` (orchestrator) | | open |
| S4 | Standards | P3 | `retention.rs:74` | `cutoffs` tuple | | cosmetic |
| S6 | Standards | P3 | `df-db/src/volumes.rs` | Provider orchestration in the storage crate | | cosmetic |
| P6 | Spec | P3 | `df-db/src/volumes.rs:71` | Reset lock test only on the retention path; holds by construction | | cosmetic |
| V4 | Visual | P3 | dialog, menus | Dialog width/border, menu row metrics, backdrop, button border (shared with the cancel UI) | | cosmetic |

User decision: Fix blocking + P1 (P1 and P5 by the orchestrator, fixed in the artifacts).

Status after the fix round: S1, P2+V1, S2, P4+S9+V3, P3, S3, S5, S7, S8, S10, V2 fixed (9b2e87d); P1, P5 fixed (dcc006b). Round 2 reviewers and a browser re-check at 390/1440 px confirmed each.

## Round 2 (2026-11-02)

- Implementer: 9b2e87d, container tests ran
- Reviewed range: dcc006b...9b2e87d
- Security: no findings
- Browser re-check: phone menu fully visible (y 428–604), disabled item with hint, confirmation paragraphs #1c1917/400/15 px, provider failure → 503 with inline error, desktop menu unchanged

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P1 | `df-web/src/task_pages/tests/reset_workspace.rs:115` | Wait-for-lock loop in a test has no upper bound; helper sits between tests | fix-now | fixed (4e08e47) |
| S12 | Standards | P3 | `df-agent-channel/src/scheduler/tests.rs:133` | `set_status` duplicated between test modules | | cosmetic |
| V5 | Visual | P3 | Status menu | 14 vs 15 px items on phone, current-status styling and casing, 44 vs 62 px disabled row, paragraph wraps to 4 lines (shared with the cancel UI) | | cosmetic |

User decision: fix S11 (done by the orchestrator, test-only), then finish.
