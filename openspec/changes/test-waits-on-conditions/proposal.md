# Proposal

## Why

`CODING_STANDARDS.md` ("Tests") already says: "Avoid timing-based tests (`sleep` to wait for an effect). Wait on the condition, use a channel, or use `tokio::time::pause()`." Seven unit tests still sleep a fixed 50–100 ms in real time and hope that the effect has happened by then. One live-cluster test sleeps 3 s. On a slow CI runner such a test is flaky, or it passes while it tests a different path than the one it names. A similar problem exists in the database tests: seven test modules have their own copy of "wait until a session waits for a lock", although `df_test_postgres::wait_for_lock_wait` exists. One copy has no timeout and no database filter, so a lock wait of another test can end its wait.

The DF-46 retro sweep found these sites. Three of them are already entries in `docs/tech_debt.md`:
- TD-5: the shutdown-while-disconnected test sleeps 100 ms.
- TD-50: the proxy shutdown tests wait on the origin without a timeout.
- TD-101: the lock-wait loop is copied across tests.

The sites are fixed by hand and not by a lint, see design, Decision 3.

## What Changes

- **Fixed sleeps become waits on a condition.** The eight sites (table in design.md, "Context"):
  - `factory-agent`: `tests/connection.rs` (2), `tests/outbox.rs` (2), `tests/runner.rs` (1), `tests/runner_runtime.rs` (1).
  - `fake-acp-agent`: `src/tests.rs` (1).
  - `df-k8s`: `src/reconcile/tests.rs` (1, live cluster, `#[ignore]`).

  Each sleep is replaced by:
  - a signal the test can wait on: the mock control plane's `connection_count`, the end of a channel, a message the runner sends;
  - or `start_paused = true`, where the test only shows that something does *not* happen.

  No production behavior changes. Where a test needs a signal that the code does not expose, the test fixture (`factory-agent/src/test_harness.rs`) provides it, not the production type.
- **One lock-wait helper.** The private copies in `df-db` (`tasks/tests.rs`, `tasks/dependency_tests.rs`, `runs/tests.rs`, `git_pushes/tests.rs`, `transcript/tests.rs`), `df-web` (`task_pages/tests/reset_workspace.rs`) and `df-agent-channel` (`session/tests.rs`) call `df_test_postgres::wait_for_lock_wait`. Three copies (`runs/tests.rs`, `git_pushes/tests.rs`, `transcript/tests.rs`) have no timeout, and the one in `runs/tests.rs` also counts lock waits of every database. With the shared helper they get both.
- **Bounded origin waits in the proxy tests (TD-50).** The origin-side `listener.accept()` calls and the head read loop in `df-gateway/src/proxy/tests.rs` get the same 5 s `tokio::time::timeout` as the client reads. A regression then fails these tests instead of making them hang.
- **Tech-debt register.** TD-5, TD-50 and TD-101 are deleted from `docs/tech_debt.md`, both the entry and the summary row, as the register asks for a fixed entry.
- **Rule made concrete.** The bullet in `CODING_STANDARDS.md` ("Tests") gets two additions:
  - a sleep in a test is allowed only on a paused clock (`start_paused`, `time::pause`) or inside a bounded poll loop;
  - a database test waits for a lock with `df_test_postgres::wait_for_lock_wait`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. The change touches only test code and a coding standard (`skip_specs: true`).

## Impact

- Test code in `factory-agent`, `fake-acp-agent`, `df-k8s`, `df-db`, `df-web`, `df-agent-channel`. Test fixture `factory-agent/src/test_harness.rs` (a wait helper on `MockControlPlane`).
- `CODING_STANDARDS.md` ("Tests"). `docs/tech_debt.md` (TD-5, TD-50, TD-101 removed). Test code in `df-gateway/src/proxy/tests.rs`.
- No production code, no dependency, no database or protobuf change. No published docs.
- Domain model unchanged; no ADR.

## Out of Scope

- A lint against `tokio::time::sleep` / `std::thread::sleep`: rejected (design, Decision 3).
- The bounded poll loops in tests (`df-orchestrator/src/scheduler/tests.rs`, `df-db/src/outbox/dispatcher/tests.rs`, `df-k8s` live helpers): they wait for a condition with a deadline, as the standard asks. A generic `eventually` helper outside `df-e2e` is rejected: each loop is a few lines, and a shared crate for it costs more than it saves.
- The two advisory-lock waits (`df-db/src/network_access/tests.rs`, `advisory_lock_waiter`; `df-orchestrator/src/pr/sync/tests.rs`, `wait_for_queued_advisory_lock`): rejected. They read `pg_locks`, not `pg_stat_activity`, and both already have the timeout and the database filter. A shared helper for two callers saves little.
- TD-16 (`eventually` in `df-agent-channel/tests/agent_port.rs` polls with `sleep`): its fix needs a subscription to the `Notifier`, not a test-timing change. It stays in `docs/tech_debt.md`.
- TD-68 (keep-alive tests wait on real deadlines): its fix injects a clock into production code. It stays in `docs/tech_debt.md`.
- The four copies of the fixed 100 ms accept-error backoff in production code (`df-tls`, `df-gateway` ×2, `factory-agent`): a production refactoring, not test timing. Backlog: "Shared accept loop with backoff" (row added by this change).
