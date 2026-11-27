# Tasks

Write each test change so that it runs green, then remove the sleep. The red state here is a flaky or wrong path, not a compile error. Prove the fix with a loop: `cargo nextest run -p <crate> <test> --retries 0` 50 times, stopping at the first failure (`for i in $(seq 50); do … || { echo "run $i failed"; exit 1; }; done` in a subshell; a plain loop reports only the last run's status), with a parallel CPU load (`stress-ng --cpu 0` or a parallel `cargo build`), when the tool is there. Tests that need PostgreSQL run only with a container socket (skill `rust`). Name the ones you could not run.

## 1. `factory-agent`: connection tests (design, Decision 1)

- [ ] 1.1 In `factory-agent/src/test_harness.rs`, add `refuse_streams()`, `accept_streams()` and `wait_for_connection_attempts(n, timeout)` to `MockControlPlane`. A refused stream answers `Status::unavailable` and still increments `connection_count`. Add a unit test in the same file: refuse, the runtime's reconnect attempt is counted and gets no stream, accept, the next attempt gets a stream. Verify: the test passes.
- [ ] 1.2 `tests/connection.rs:786`: refuse new streams, then drop the active one, then wait until `connection_count` has grown by one (design, Decision 1: this order), send the two messages, accept again. Remove the sleep and its comment. Verify: the test passes 50 times in the loop.
- [ ] 1.3 `tests/connection.rs:1248`: the same order: refuse, drop, wait for the refused attempt, send the report, `shutdown()`, accept. If the test now fails, the report path after shutdown while disconnected is broken: report it to the user with the output, do not bring the sleep back. Delete TD-5 (entry and summary row) from `docs/tech_debt.md`. Verify: the test passes 50 times in the loop, and `grep -n "TD-5 " docs/tech_debt.md` finds nothing.

## 2. `factory-agent` and `fake-acp-agent`: outbox, runner, cancel

- [ ] 2.1 `tests/outbox.rs` `test_outbox_bounded_backpressure` and `test_append_or_stop_appends_when_stopped_while_full`: switch them to `#[tokio::test(start_paused = true)]` (design, Decision 2). Keep the sleep, which now advances the paused clock only, and add `assert!(!handle.is_finished())` before the space is freed in the first test. Verify: both pass 50 times in the loop. As a check, remove the ack in the first test: the `is_finished` assertion still holds and the test fails at the timeout.
- [ ] 2.2 `tests/runner.rs:494`: the control-plane task reads from `outbound_rx` until the first message of the turn (the first `PhaseChanged` with phase `Running`, or the first transcript event, whichever the runner sends first; check in `runner.rs`). Only then does it send `Cancel`. Remove the sleep. Verify: the test passes 50 times in the loop and still ends with outcome `Cancelled`.
- [ ] 2.3 `tests/runner_runtime.rs:644`: replace sleep and `abort` with `tokio::time::timeout(Duration::from_secs(5), cp_task2).await`, and fail the test on both errors (`.expect("collector ended")` for the timeout, `.expect("collector did not panic")` for the join) before reading `run2_messages`. That works when `run_session` drops the last `outbound_tx2`. If the collector does not end because another clone of the sender lives on, drop that clone in the test, or drain `outbound_rx2` until it is empty and then abort. Verify: the test passes 50 times in the loop, and the assertions on `run2_messages` still see every message.
- [ ] 2.4 `fake-acp-agent/src/tests.rs:1100`: send the `CancelNotification` after the client has received the session update "Starting long task..." of the script, not after 100 ms. Remove the sleep. Verify: the test passes 50 times in the loop.

## 3. `df-k8s` live test

- [ ] 3.1 `df-k8s/src/reconcile/tests.rs:308`: replace the 3 s sleep with a wait for the first pass's patch. The policy already has the rules `initial`, so that patch changes nothing visible (design, Context). Before the loop starts, patch the egress rules to `[]`. After the first `get_targets` call, wait with `wait_for_egress_rules(&netpols, &initial, …)` until the first pass has restored them. Remove the sleep. Verify: `cargo test -p df-k8s --lib policy_loop_live -- --ignored` passes against a dev cluster (`DF_LIVE_*`, crate README). Without a cluster, state that it did not run.

## 4. One lock-wait helper (design, Decision 4)

- [ ] 4.1 Replace the private loops with `df_test_postgres::wait_for_lock_wait(&pool)`:
  - `df-db/src/tasks/tests.rs` (`wait_for_lock_waiter`), `tasks/dependency_tests.rs` (`wait_for_a_lock_wait`), `runs/tests.rs:751` (inline), `git_pushes/tests.rs:430`, `transcript/tests.rs:217`;
  - `df-web/src/task_pages/tests/reset_workspace.rs` (`wait_for_a_lock_wait`);
  - `df-agent-channel/src/session/tests.rs:141` (inline).

  In `git_pushes` and `transcript`, the in-loop `assert!(!writer.is_finished())` goes away with the loop (design, Decision 4). Leave the advisory-lock waits of `network_access/tests.rs` and `pr/sync/tests.rs` alone. Remove the private functions, and delete TD-101 (entry and summary row) from `docs/tech_debt.md`. Verify: `grep -rn "wait_event_type = 'Lock'" crates --include=*.rs` lists only `df-test-postgres/src/lib.rs`, and `cargo nextest run -p df-db -p df-web -p df-agent-channel` passes (container socket needed; otherwise CI).

- [ ] 4.2 TD-50: in `df-gateway/src/proxy/tests.rs`, wrap every origin-side `listener.accept()` and the request-head read loop that has no deadline in `tokio::time::timeout(Duration::from_secs(5), …)`, like the client reads. Delete TD-50 (entry and summary row) from `docs/tech_debt.md`. Verify: `cargo nextest run -p df-gateway proxy` passes. As a check, drop the forwarding in one test: it fails within 5 s instead of hanging.

## 5. Rule and checks

- [ ] 5.1 In `CODING_STANDARDS.md`, section "Tests": extend the sleep bullet. A sleep in a test is allowed only on a paused clock (`start_paused`, `time::pause`) or inside a poll loop with a deadline. To fake an outage, refuse in the mock instead of waiting. A database test waits for a row or table lock with `df_test_postgres::wait_for_lock_wait`; a wait for an advisory lock keeps its own `pg_locks` helper. Verify by reading it against design, Decisions 1, 2 and 4.
- [ ] 5.2 Verify that no real-time fixed sleep is left: `grep -rnE "sleep\(" crates --include=*.rs` lists, in test code, only sleeps on a paused clock or in bounded poll loops. Check each remaining test hit by hand and list them in the PR description.
- [ ] 5.3 Run the checks: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, and `cargo nextest run -p factory-agent -p fake-acp-agent`. Verify: all pass.
- [ ] 5.4 Published docs: none. The change touches only test code and `CODING_STANDARDS.md`.
