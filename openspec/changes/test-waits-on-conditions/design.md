# Design

## Context

See proposal.md, "Why". The rule is in `CODING_STANDARDS.md` ("Tests"). The sites:

| Site | Sleep | What the test waits for | Fix (Decision) |
|---|---|---|---|
| `factory-agent/src/tests/connection.rs:786` (`test_scenario_short_outage_events_queue_in_outbox`) | 50 ms | the connection loop sees the dropped stream, so the next messages are sent during the outage | outage switch on the mock (1) |
| `factory-agent/src/tests/connection.rs:1248` (`test_final_report_is_delivered_after_shutdown_while_disconnected`, TD-5) | 100 ms | the runtime is disconnected when the final report is sent. The mock accepts the reconnect at once, so after 100 ms the runtime may already be connected again: the test may not test what its name says | outage switch on the mock (1) |
| `factory-agent/src/tests/outbox.rs:210` (`test_outbox_bounded_backpressure`) | 50 ms | the spawned `append_with_backpressure` waits for space before the test frees it | paused clock (2) |
| `factory-agent/src/tests/outbox.rs:419` (`test_append_or_stop_appends_when_stopped_while_full`) | 100 ms | negative: `append_or_stop` is still waiting | paused clock (2) |
| `factory-agent/src/tests/runner.rs:494` (`test_scenario_outcome_cancelled_by_control_plane`) | 100 ms | the turn runs when the cancel arrives | signal: send the cancel after the first outbound message of the turn (1) |
| `factory-agent/src/tests/runner_runtime.rs:644` (`test_scenario_full_runtime_two_runs_on_single_data_dir`) | 50 ms, then `abort` | the collector task `cp_task2` has read every outbound message of run 2 | await the collector after `run_session` returns and drops its sender (1) |
| `fake-acp-agent/src/tests.rs:1100` (`test_scenario_hang_until_cancelled`) | 100 ms | the prompt hangs when the cancel arrives | signal: the script says "Starting long task..." before `end: hang`; send the cancel after that session update (1) |
| `df-k8s/src/reconcile/tests.rs:308` (`policy_loop_live`, `#[ignore]`, live cluster) | 3 s | the first pass's patch has landed | make the first pass visible: clear the egress rules before the loop starts, then wait until they equal `initial` again (1) |

Facts the fixes build on:

- `MockControlPlane` (`factory-agent/src/test_harness.rs`) counts stream openings in `connection_count` (an `AtomicUsize`). `drop_active_stream` clears the active sender. Nothing stops the runtime from reconnecting at once.
- `Outbox` uses `std::fs` (sync) and a `tokio::sync::Notify` (`space_available`). It has no `tokio::fs` and no `spawn_blocking`. On a paused clock, `tokio::time::sleep` therefore returns only when every task has run until it blocks.
- Seven private lock-wait loops, all with the same `pg_stat_activity` query:
  - `df-db/src/tasks/tests.rs:1414`, `tasks/dependency_tests.rs:483`, `runs/tests.rs:751`, `git_pushes/tests.rs:430`, `transcript/tests.rs:217`;
  - `df-web/src/task_pages/tests/reset_workspace.rs:40`;
  - `df-agent-channel/src/session/tests.rs:141`.

  This is TD-101. Its proposed fix, the helper `df_test_postgres::wait_for_lock_wait(&PgPool)` (`df-test-postgres/src/lib.rs:131`), already exists, with a 10 s timeout and the filter `datname = current_database()`. Differences of the copies:
  - `runs/tests.rs:751` has neither the timeout nor the filter.
  - `git_pushes/tests.rs:430` and `transcript/tests.rs:217` have no timeout. Inside the loop they assert `!writer.is_finished()`, so a writer that never waits fails at once.
  - `runs`, `git_pushes`, `transcript` and `dependency_tests` spin with `yield_now`; the others sleep 10–20 ms per poll.

  The three crates already have `df-test-postgres` as a dev-dependency.
- Two other tests wait for an advisory lock through `pg_locks` (`df-db/src/network_access/tests.rs:559`, `df-orchestrator/src/pr/sync/tests.rs:598`). Both have a 10 s timeout and the database filter (proposal, "Out of Scope").
- `policy_loop_live` provisions the `NetworkPolicy` with the rules `initial` already (`drive_policy_loop` checks this). So the first pass's patch changes nothing, and neither the rules nor the `resourceVersion` show that it landed.
- TD-50: in `df-gateway/src/proxy/tests.rs`, some origin-side `listener.accept()` calls (for example near lines 973 and 1000) still have no timeout. The client side already uses `tokio::time::timeout(Duration::from_secs(5), …)` (line 912).
- TD-5's own text says that the test fails when the manager is still in the active session at the moment it sends. Decision 1 rules this state out.

## Goals / Non-Goals

**Goals:** no test sleeps in real time to wait for an effect. Each test from the table tests the path its name says, independent of how fast the runner is.

**Non-Goals:** the bounded poll loops and the accept-loop backoff (proposal, "Out of Scope").

## Decisions

### Decision 1: wait on a signal the test can see

Prefer a signal that the test fixture or the protocol already gives:
- a message on a channel;
- the end of a channel;
- a counter of the mock.

Where none exists, the fixture adds one. The production type does not. For the two connection tests, `MockControlPlane` gets an outage switch:
- `refuse_streams()`: new streams are answered with `Status::unavailable`, and each refused attempt is still counted. The handler decides on refusal before it increments `connection_count`, so a counted attempt is already refused when the test sees the count.
- `accept_streams()`: new streams are accepted again.
- `wait_for_connection_attempts(n)`: waits, with a timeout, until `connection_count` reaches `n`.

The order is fixed: the test first refuses new streams, then reads `connection_count` as the baseline, then drops the active one (`drop_active_stream`), then waits until the count reaches baseline + 1. A baseline read after the drop could already include the refused attempt, and the wait would then target a second one. The refused attempt shows that the runtime has seen the drop. Refusing first matters: if the test dropped the stream first, the runtime could reconnect, and be accepted, before refusing starts. Then it sends the messages, which are now sent during the outage by construction. At the end it accepts again.

- Rejected: a connection-state `watch` on the production `ConnectionHandle`. It is test-only API in production code.
- Rejected: a longer sleep. It is still a guess.

### Decision 2: a paused clock for "does not happen (yet)"

A test that shows that a task still waits uses `#[tokio::test(start_paused = true)]`. On a paused clock, `sleep` advances only when every task has run until it blocks. So `!handle.is_finished()` after the sleep is certain, and the sleep costs no time. This works only without real I/O through the runtime (sockets, `tokio::fs`). The outbox tests qualify (see Context).

### Decision 3: no lint

- Rejected: `disallowed-methods` for `tokio::time::sleep` / `std::thread::sleep`. They have about 40 legitimate production callers (backoff, retry, ticks) and about 25 test callers on a paused clock or in bounded poll loops. Each would need an `#[expect]`, which buries the eight real hits. Unlike `Locator::first` (change `e2e-locator-lint`), a sleep is not wrong in itself.
- Rejected: an xtask grep. It cannot see whether the clock is paused.

The rule stays a written standard, made concrete (proposal, "What Changes"). The standards reviewer checks it.

### Decision 4: one lock-wait helper

The copies call `df_test_postgres::wait_for_lock_wait`. Its 10 s timeout replaces the copies' 5 s and 10 s timeouts. This only matters when a test fails. `runs/tests.rs` gets the database filter and the timeout with it.

The in-loop `!writer.is_finished()` assertion of `git_pushes` and `transcript` goes away on purpose. A writer that ends without waiting then fails the test after the helper's 10 s timeout, with "no lock wait", instead of at once. That is slower, but only when the test fails, and the two tests keep a short loop for one assertion less. The advisory-lock waits stay as they are: their query is a different one (Context).

## Risks / Trade-offs

- [A fixed test now fails, because it never tested its path. For example, the report test was connected again before it sent] → That is a finding. Fix the test or report it as a bug in the code under test; do not bring the sleep back.
- [`start_paused` and real I/O: a test with a socket never advances, or advances too early] → Use Decision 2 only for the outbox tests. Each conversion runs 50 times in a loop (task 1.x).
- [The `df-k8s` live test runs only by hand] → The fix is checked against a dev cluster if one is available. Otherwise the task says that it was not run.

## Migration Plan

Test code only. Rollback: revert the commit.
