# Tasks

Requires `db-write-ownership`. Before starting, re-measure the tables in design.md on the current code (`rg "state = |stop_requested_at = " crates/df-db/src crates/df-agent-channel/src`); a new write site joins group 2. Database tests need PostgreSQL; report the ones that could not run. No user- or operator-visible behavior changes (see design.md, "Intended differences"); the published crate reference `crates/df-agent-channel/docs/configuration.md` still changes (Run transitions section, state diagram fix), `docs/content/` does not. Contributor docs are tasks 1.4 and 5.1.

## 1. Pure transition function (`df-core`)

- [x] 1.1 Confirm that `factory-agent` never sends `RunPhase::Preparing` or `Finalizing` (`rg "RunPhase::" crates/factory-agent/src` outside tests). If it does, stop and add the phase as a `RunEvent` to design decision 1.
- [x] 1.2 Write failing unit tests in `crates/df-core/src/run_transition/tests.rs`, one per guard in the first table of design.md and one per row of the command table. Also cover: empty transitions for repeated or late events, `Rejected` for `StopRequested` on a terminal run, and the four branches of `overdue` in today's order. Then add `run_transition.rs` (`RunSnapshot`, `RunEvent`, `RunTransition`, `RunEffect`, `FinalizeReason` with `command()`, `overdue`, `RunEnd` moved from `df_db::runs`) until they pass. Verify `cargo nextest run -p df-core run_transition`.
- [x] 1.3 Move the `match` of `enforce_timeouts` into `overdue` verbatim and keep `scheduler`'s existing timeout tests green against it. Verify `cargo nextest run -p df-core run_transition -p df-agent-channel scheduler`.
- [x] 1.4 Write `crates/df-core/docs/run-transitions.md` (diagram, event table, command table) and the test of design decision 6. Verify `cargo nextest run -p df-core run_transition`.

## 2. Applier (`df-db`)

- [x] 2.1 Add the migration for `runs.finalize_reason` (design decision 3, with the backfill of the proposal's Impact). Regenerate `.sqlx/`. Verify `cargo nextest run -p df-db migrate`.
- [x] 2.2 Write failing `df-db` tests for `apply_run_event`:
  - it locks the run row (a concurrent `queue_message` serializes, as in the existing `start_finalizing` test);
  - it writes state, fields and `finalize_reason`;
  - it raises the task event in the same transaction;
  - it publishes `RunChanged` only on a change;
  - it enqueues the sandbox lifecycle events of today.
  
  Implement `crates/df-db/src/run_events.rs`. Verify `cargo nextest run -p df-db run_events`.
- [x] 2.3 Switch the `df_db::runs` transitions (`begin_provisioning`, `mark_started`, `mark_connected`, `set_idle`, `start_finalizing`, `end_run`) and `request_stop_tx` to `apply_run_event`, keeping their signatures where callers use the return value. Delete `set_run_state`/`set_run_state_tx`. Verify `cargo nextest run -p df-db` and that `rg "UPDATE runs SET state" crates/*/src` finds only `run_events.rs` outside test code.
- [x] 2.4 Switch the task applier's effects `FinalizeIdleRun` and `StopRun` (`task_events.rs`) to `apply_run_event` (design decision 2). Add a test that a `MarkedDone` by the board stores `finalize_reason = done` and raises no further task event. Verify `cargo nextest run -p df-db task_events tasks`.

## 3. Session and scheduler (`df-agent-channel`)

- [x] 3.1 `apply_phase`: `Running`/`AwaitingInput` → `set_idle`, `Preparing`/`Finalizing` → debug log, no event (proposal). Verify `cargo nextest run -p df-agent-channel session`.
- [x] 3.2 Replace `reconcile_finalizing`, `human_cancel_sent`, `release_sent` and the loop's `cancel_sent` with `finalize_reason.command()` and the last command sent on the stream (design decision 3). Route the budget timer through `Finalize(Budget)`. Verify that the existing reconnect, release, budget-cancel and stop tests in `session/tests.rs` pass unchanged with `cargo nextest run -p df-agent-channel session`.
- [x] 3.3 Switch `enforce_timeouts` to `list_overdue_runs` + `apply_run_event(Overdue)` (design decision 4). Verify that the `scheduler` tests pass unchanged and that the new test "a run that connected after being listed is not ended" passes with `cargo nextest run -p df-agent-channel scheduler`.

## 4. Runtime (`factory-agent`)

- [x] 4.1 Write failing unit tests for `RunControl::turn_outcome` (each command alone, each pair in precedence order), then replace the four `AtomicBool`s in `runner.rs` with `Mutex<RunControl>` (design decision 5). Verify `cargo nextest run -p factory-agent` and that `rg "AtomicBool" crates/factory-agent/src/runner.rs` finds none of the four flags.

## 5. Documentation and integration

- [x] 5.1 Set ADR-0055's "Run state stays where it is" bullet to point to ADR-0061 and update ADR-0061's status line to "Accepted". In `docs/architecture.md`, drop the "target" mark from `run_transition` in the `df-core` row.
- [x] 5.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `SQLX_OFFLINE=true cargo build --workspace`, `bazel --bazelrc=.bazelrc.agents test //crates/df-core/... //crates/df-db/... //crates/df-agent-channel/... //crates/factory-agent/...` and `just e2e-local` (or report that it could not run); all pass.

## Workflow follow-up

- At archive: propose the backlog row "Remove `RUN_PHASE_PREPARING` and `RUN_PHASE_FINALIZING` from `agent.proto`" for `product/iterations/roadmap.md`.
