# Review log: iter1-keep-alive-and-review

- BASE: b7eae06c73f03df766b5397f9991db1290a66515
- Store: none
- Status: done
- Implementer agent: af2d4a68bee4afbb4
- Design commit: 6c2b5a3 (task 1.2 done; PenPot board "Desktop · in review", tokens `color.status.review.*`)
- Paused after design: artifacts must be updated (`/opsx:update`), then resume `/implement-change iter1-keep-alive-and-review`.

## Round 1 (2026-10-27)

- Implementer: ce8056c, tasks 16/16, checks fmt/clippy/machete/tests ok, container tests ran (podman socket), none skipped
- Reviewed range: b7eae06...ce8056c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| A1 | Spec | Blocking | `session.rs:280-291` | Budget `Cancel` leaves run not `finalizing`; prompt can follow | fix-now | fixed (ce693fd) |
| A3+S11 | Spec+Standards | Blocking | `idle_tests.rs:237` | "Stream lost while idle" untested; wall-clock negative check | fix-now | fixed (ce693fd) |
| S1 | Standards | P1 | `df-db/tasks.rs:693`, `df-web/view.rs:496` | mark-done rule duplicated | fix-now | fixed (ce693fd) |
| S5 | Standards | P1 | `session.rs:153` | `cancel_sent` name clash | fix-now | fixed (ce693fd) |
| A2 | Spec | P1 | `session.rs:1170` | prompt forwarding not asserted | fix-now | fixed (ce693fd) |
| A4 | Spec | P1 | `df-web/task_pages/tests.rs:2170` | mark-done POST test weak | fix-now | fixed (ce693fd) |
| A5 | Spec | P1 | `idle_tests.rs:188` | same-session follow-up not asserted | fix-now | fixed (ce693fd) |
| S3 | Standards | P2->fix | `session.rs:305`, `runs.rs:395` | lock+pending query repeated | fix-now (user) | fixed (ce693fd) |
| S4 | Standards | P3->fix | `session.rs:345,407` | send Release duplicated | fix-now (user) | fixed (ce693fd) |
| S6 | Standards | P3->fix | `session.rs:390`, `runs.rs:407` | string compare of status | fix-now (user) | fixed (ce693fd) |
| S7 | Standards | P3->fix | `session.rs:~873` | `last_turn_end` repeats query | fix-now (user) | fixed (ce693fd) |
| S10 | Standards | P3->fix | `runner.rs:533` | `wait_for_input` param clump | fix-now (user) | fixed (ce693fd) |
| S12 | Standards | P3->fix | `session.rs:1267` | panic guard in test | fix-now (user) | fixed (ce693fd) |
| S2 | Standards | P2 | `runs.rs:241`, `tasks.rs:~253` | SQL state list duplicated | tech-debt | tech-debt (TD-65) |
| S13 | Standards | P2 | `idle_tests.rs:463` | fixture duplicated | tech-debt | tech-debt (TD-67) |
| S8 | Standards | P3 | `runner.rs:541,1046` | magic duration twice | cosmetic | cosmetic |
| S9 | Standards | P3 | `runner.rs:1224` | PhaseChanged send duplicated | cosmetic | cosmetic |
| S14 | Standards | P3 | `runs.rs:228` | bool flag arg | cosmetic | cosmetic |

User decision: Blocking + P1 + S3, S4, S6, S7, S10, S12.

## Round 2 (2026-10-27)

- Implementer: ce693fd, tasks 16/16, checks ok, container tests ran, none skipped
- Reviewed range: ce8056c...ce693fd

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| A3 (reopened) | Spec | Blocking | `idle_tests.rs:277-372` | stream-lost test cannot tell a surviving wait from an ended one | fix-now | fixed (de443d9) |
| A6 | Spec | Blocking | `session.rs:286`, `:384-416` | `Release` follows budget `Cancel` via `reconcile_finalizing` | fix-now | fixed (de443d9) |
| A7 | Spec | P1 | `df-web/task_pages/tests.rs:2200` | done-filter assertion has no baseline | fix-now | fixed (de443d9) |
| S17 | Standards | P1 | `session.rs:654`, `start_finalizing` | `lock_task_and_check_pending` used for lock only | fix-now | fixed (de443d9) |
| S18 | Standards | P3->fix | `runs.rs:420`, `session.rs:401`, `tasks.rs:695` | `ParseEnumError` closure repeated | fix-now (user) | fixed (de443d9) |
| S19 | Standards | P3->fix | `idle_tests.rs:692` | test coupled to log text | fix-now (user) | fixed (de443d9) |
| S20 | Standards | P3->fix | `view.rs:568` | `TaskDetail.status` still `String` | fix-now (user) | fixed (de443d9) |
| S15 | Standards | P2 | `session.rs:322` | bool flag in `start_finalizing` | tech-debt | tech-debt (TD-66) |
| S16 | Standards | P2 | `runs.rs:241`, `session.rs:339`, `tasks.rs:256` | state list in three places (merged with S2) | tech-debt | tech-debt (TD-65) |

User decision (round 2): Blocking + P1 + S18, S19, S20.

## Round 3 (2026-10-27)

- Implementer: de443d9, tasks 16/16, fmt/clippy/machete ok, container socket went away mid-round: df-db, df-web, factory-agent (final state), dark-factory serve_* not re-run; passed earlier: idle_tests 5/5, df-agent-channel, df-core
- Reviewed range: ce693fd...de443d9

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21 | Standards | P1 | `idle_tests.rs:371-373` | chunk matched via Debug output | fix-now | fixed (6a3f806) |
| S22 | Standards | P1 | `df-agent-channel/src/error.rs:25` | stale `InvalidData` doc | fix-now | fixed (6a3f806) |
| S23 | Standards | P3->fix | `runs.rs:367` | `has_pending` name unclear | fix-now (user) | fixed (6a3f806) |

User decision (round 3): Blocking + P1 + S23.

Round 3 fixed (6a3f806): S21, S22, S23. Tech debt created: TD-65 (S2, S16), TD-66 (S15), TD-67 (S13), TD-68 (S11 remainder). Cosmetic: S8, S9, S14.
Container tests not run in the final state (socket went away during fix round 2): df-db, df-web, factory-agent, df-agent-channel container-backed tests, dark-factory `serve_*`.
