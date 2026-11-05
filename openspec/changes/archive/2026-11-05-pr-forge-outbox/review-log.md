# Review log: pr-forge-outbox

- BASE: 2ed023ad2781a1a18a6932bd4590719b43383f9b
- Store: none
- Status: done
- Implementer agent: a19b6ad21f7bbc28b (rounds 1–2), a582ac70b80b1ac16 (round 3)

## Round 1 (2026-11-05)

- Implementer: 75e98b2d, tasks 15/15, checks fmt/clippy/machete/file-length/docs-build ok, bazel test //... 50/50, container tests ran (podman); sqlx prepare --check not run (no new query! macros)
- Reviewed range: BASE...75e98b2d
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-agent-channel/src/pr.rs:58`, `crates/df-db/src/tasks.rs:934` | `ready` from app clock, `queued_at` = DB `now()` (tx start). A message tx started before run end, blocked on the task `FOR UPDATE`, commits with `queued_at < ready` → reopened task's PR marked ready. Plus replica clock skew. (verified) | fix-now | open |
| P2 | Spec | Blocking | `crates/df-agent-channel/src/pr/sync/dispatcher_tests.rs` | Scenarios "Forge down at first push" / "Restart before the PR is opened" have no dispatcher-level test (open path) | fix-now | open |
| P3 | Spec | P1 | `docs/content/architecture/agent-runs.md:85` | Status table says completed run always gets the summary | fix-now | open |
| S1 | Standards | P1 | `pr.rs:74`, `pr/sync.rs:320` | Comments cite TD-82/TD-13, deleted by this change | fix-now | open |
| S2 | Standards | P1 | `pr/sync.rs:35-100` | Second `impl PrSyncHandler` separated from type by private helpers | fix-now | open |
| S3 | Standards | P1 | `pr/sync.rs:101-141` | Step comments out of order (4 before 3) | fix-now | open |
| S4 | Standards | P1 | `pr.rs` `enqueue_run_end`, `runs.rs:629` | Two adjacent bool params | fix-now | open |
| S5 | Standards | P1 | `pr/sync.rs:183,345` | Recorded-PR query duplicated | fix-now | open |
| S9 | Standards | P1 | `crates/dark-factory/src/outbox.rs:201` | Test name no longer states what it asserts | fix-now | open |
| S6 | Standards | P3 | tests in runs/session/df-db | `pr.sync` outbox query repeated in tests, `(bool,bool,bool)` tuple | — | cosmetic |
| S7 | Standards | P3 | `pr/mark_draft.rs:302` | Test helper reached from sibling `tests` module | — | cosmetic |
| S8 | Standards | P3 | `crates/df-web/src/task_pages.rs:454` | Doc comment line >100 cols | — | cosmetic |

User decision: Fix blocking + P1 (P1, P2, P3, S1–S5, S9).

## Round 2 (2026-11-05)

- Implementer: e5bb084f, fixed P1 P2 P3 S1–S5 S9 (P1 via `clock_timestamp()` under task lock + migration `20261105000003` setting `task_messages.queued_at` default to `clock_timestamp()`). Checks fmt/clippy/machete/file-length/docs-build ok. Container tests: df-db, df-agent-channel, dark-factory outbox ran; df-web and `bazel test //...` NOT rerun (test PostgreSQL container `df-test-postgres-17-alpine-r1` broken after ENOSPC; removal denied to the subagent).
- Reviewed range: 75e98b2d...e5bb084f
- Security: 0 findings. Round-1 rows P1 P2 P3 S1–S5 S9: fixed (e5bb084f).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking (regression) | `crates/df-db/src/tasks.rs:999` | `stop_requested_at = now()` (tx start) vs `queued_at` now `clock_timestamp()`: a message committed while the stop tx waits on the lock counts as "after the stop" → wrong requeue. Widened by this change. (verified) | fix-now | open |
| P6 | Spec | Blocking | `crates/df-db/src/tasks.rs:847` | Mark-done clock read has no regression test (message waiting on mark-done's lock keeps PR draft) | fix-now | open |
| P5 | Spec | P1 | `design.md` D2/D3 | Design still says `now()`; clock_timestamp decision + migration unrecorded | fix-now | open |
| P7 | Spec | P1 | `pr/sync/dispatcher_tests.rs:135` | "exactly one PR" not asserted on forge create calls | fix-now | open |
| P8 | Spec | P1 | `docs/content/architecture/agent-runs.md:119` | Wording implies only lock-waiting messages count; Mark done not mentioned | fix-now | open |
| S10 | Standards | P1 | `pr.rs:61`, `df-db/src/tasks.rs:850` | `SELECT clock_timestamp()` + invariant comment duplicated | fix-now | open |
| S11 | Standards | P1 | `dispatcher_tests.rs:178` | `pushed_run` drops RunId, test re-queries it | fix-now | open |
| S13 | Standards | P1 | `configuration.md:212`, `agent-runs.md:119` | Unwrapped long sentences | fix-now | open |
| S12 | Standards | P3 | `dispatcher_tests.rs:190` | Hand-built text block; helpers exist | — | cosmetic |

User decision: Fix blocking + P1 (P4, P6, P5, P7, P8, S10, S11, S13).

## Round 3 (2026-11-05)

- Implementer: c1ed926f (commit message: fixed P4 P5 P6 P7 P8 S10 S11 S13). No report: the implementer hung waiting on the broken test container. Orchestrator ran fmt/clippy/machete/file-length/test-names. Container tests: none run for this round (user away; container `df-test-postgres-17-alpine-r1` exited, to be removed by the user at the end).
- Reviewed range: e5bb084f...c1ed926f
- Security: 0 findings. Round-2 rows P5 P7 P8 S10 S11 S13: fixed (c1ed926f). P4: code fixed, test missing (→ P9). P6: not fixed (→ P10).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P9 | Spec | Blocking | `crates/df-db/src/tasks/tests.rs:1382-1440` | `commit_a_message_while_blocked` inserts the message before `op`'s tx starts, so the stop-time tests pass with old `now()`; P4 regression not tested. (verified) | fix-now | open |
| P10 | Spec | Blocking | `crates/df-db/src/tasks/tests.rs:1455` | Mark-done test queues the message after commit; passes with `now()`/`Utc::now()` and with the old `queued_at` default | fix-now | open |
| S14 | Standards | Blocking | `crates/df-db/src/tasks/tests.rs:1385` | Lock-wait polling loop duplicated without timeout → test hangs instead of failing | fix-now | open |
| P11 | Spec | P1 | `crates/df-db/src/tasks.rs:44,1001` | `clock_under_task_lock` doc claims it provides `stop_requested_at`; `request_stop_tx` uses SQL `clock_timestamp()` directly | fix-now | open |

User decision: Fix blocking + P1 (P9, P10, S14, P11). Fresh implementer (previous one hung without report).

## Round 4 (2026-11-05)

- Implementer: ab01940f, fixed P9 P10 S14 P11. Checks fmt/clippy/machete/test-names ok, `cargo test -p df-db --no-run` ok. Container tests NOT run (broken container): df-db `a_message_committed_before_the_stop_is_queued_before_the_stop_time`, `a_message_queued_after_the_stop_is_queued_after_the_stop_time`, `a_message_after_mark_done_is_queued_after_the_ready_time`, `mark_done_takes_the_run_lock_before_the_task_lock`, `removing_a_message_while_another_is_queued_keeps_the_task_queued`; plus df-web and `bazel test //...` from round 2 on.
- Reviewed range: c1ed926f...ab01940f
- Security: 0 findings. Round-3 rows P9 P10 S14 P11: fixed (ab01940f).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P12 | Spec | P1 | `crates/df-db/src/tasks/tests.rs:1438` | No test for a message that commits while `mark_task_done` waits on the task lock; `ready = now()` regression would pass | fix-now | rejected (a message committed first sets the task `queued`; `can_mark_done` rejects it, so mark done conflicts and enqueues no job; the other order is covered by P10) |
| S15 | Standards | P3 | `tasks/tests.rs:1423` | Comment between attribute and fn instead of `///` | — | cosmetic |
| S16 | Standards | P3 | `tasks/tests.rs:1443` | `SELECT 1` purpose not stated | — | cosmetic |
| S17 | Standards | P3 | `tasks/tests.rs:1359` | Task-row lock query appears twice | — | cosmetic |

User decision: add P12 (one test), then finish. S15–S17 cosmetic.

## Finish (2026-11-05)

- Open blocking findings: none. Rejected: P12. Cosmetic: S6 S7 S8 S12 S15 S16 S17. Tech debt: none (no P2 findings).
- Bazel build: skipped, no `Cargo.toml`/`Cargo.lock` changes. `just docs-build`: ok.
- Docs changed: `docs/content/architecture/agent-runs.md`, `docs/content/observability/telemetry.md`, `docs/content/operations/troubleshooting.md`, `docs/content/using/tasks.md`, `crates/df-agent-channel/docs/configuration.md`.
- Roadmap: backlog row "Drop `tasks.completion_summary`" added.
- Status `untested`: container-backed tests not run after fix round 1 (test container `df-test-postgres-17-alpine-r1` broken, user away): `cargo test -p df-db`, `cargo test -p df-agent-channel`, `cargo test -p df-web`, `bazel --bazelrc=.bazelrc.agents test //crates/... //plugins/...`. Includes the race tests `a_message_committed_before_the_stop_is_queued_before_the_stop_time`, `a_message_after_mark_done_is_queued_after_the_ready_time`, `a_message_that_waited_for_the_run_end_keeps_the_pr_a_draft`.

## Container tests (2026-11-05)

- cargo (orchestrator): `df-db` 220 passed, `df-agent-channel` 287 + 27 + 1 passed, `df-web` 632 passed (a first df-web run failed only on ENOSPC in the test PostgreSQL container).
- Bazel `//crates/... //plugins/...` (user, after freeing disk space): all pass except `runs::tests::the_concurrency_limit_holds_further_tasks_and_the_oldest_resume_time_goes_first`, reported flaky. Not reproduced: 15/15 isolated runs and 3/3 full lib-suite runs passed; the failing runs here were test-container crashes (ENOSPC, recovery-mode restart). The test and its fixtures are not touched by this change (pre-existing, from `iter2-run-resilience`). Judged infrastructure; user decided to finish.
- Status set to `done` by user decision.
