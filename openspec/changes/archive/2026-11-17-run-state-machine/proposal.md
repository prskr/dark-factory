# Proposal

## Why

`tasks.status` has one transition function (ADR-0055); `runs.state` has none. Its rules sit in the `WHERE` clauses of 12 statements in two crates, `set_run_state_tx` accepts any target state, and `enforce_timeouts` decides on an unlocked read. Why a run is `finalizing` isn't stored, so after a reconnect the session works it out again from the task status, `stop_requested_at`, the budget deadline and the last turn end, and keeps two flags so it doesn't send twice. The runtime holds the commands it received in four `AtomicBool`s. ADR-0061 decides to treat runs the way ADR-0055 treats tasks.

## What Changes

- `df_core::run_transition`: `run_transition(&RunSnapshot, RunEvent) -> Result<RunTransition, Rejected>` and `overdue(&RunSnapshot, now, &RunsConfig)`, both pure, with unit tests for every rule.
- `df_db::apply_run_event`: locks the run row, loads the snapshot, applies the transition and runs its effects (publish, sandbox lifecycle event, task event through `apply_task_event`) in the caller's transaction.
- These become `RunEvent`s:
  - the transition functions of `df_db::runs` (from `db-write-ownership`): `start_queued_runs` (insert), `begin_provisioning`, `mark_started`, `mark_connected`, `set_idle`, `start_finalizing`, `end_run`;
  - `request_stop_tx`;
  - the task applier's effects `FinalizeIdleRun` and `StopRun`;
  - `enforce_timeouts`.
  
  `set_run_state_tx` is deleted.
- New column `runs.finalize_reason` (`idle`, `budget`, `stop`, `done`). The session picks `Release` or `Cancel` from it alone. The session flags `human_cancel_sent`, `release_sent` and the loop's `cancel_sent` become one record of the last command sent on this stream (`command_sent: Option<RunCommand>`). `ended` comes from the applier's result.
- `factory-agent`: `RunControl` replaces the four flags `released`, `cancelled`, `end_run_requested` and `await_input_requested`, with one function deciding the outcome at the end of a turn.
- `apply_phase`: the runtime never sends `Preparing` or `Finalizing` (`factory-agent` sends only `Running` and `AwaitingInput`; task 1.1 confirms this). Both phases map to no event and are logged at `debug`. `Connected` already moves the run to `preparing`.
- Docs:
  - `crates/df-core/docs/run-transitions.md`: state diagram and event table, checked by a test as for tasks;
  - ADR-0061, plus ADR-0055 amended to point to it.
- No change in observable behavior, configuration or wire format, except the intended differences in design.md.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. `run-orchestration` describes the run states and commands; they stay the same. `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- Removing `RUN_PHASE_PREPARING` and `RUN_PHASE_FINALIZING` from `agent.proto` (wire change; the runtime does not send them): backlog row in `product/iterations/roadmap.md`, proposed at archive.
- The scheduler's slot accounting and the order of due tasks. Rejected (ADR-0055, ADR-0061).
- Merging run and task state into one machine. Rejected (ADR-0061, Alternatives).
- Stream-local session state (`busy`, `idle_at`, `sent_prompts`). Rejected: it describes prompts in flight on one stream, not the run.
- The remaining state predicates: change `state-predicates`.

## Impact

- Code:
  - `df-core`: new `run_transition.rs`, `RunState`, `RunOutcome`
  - `df-db`: new `run_events.rs`; `runs.rs`, `tasks.rs` (`request_stop_tx`), `task_events.rs` (the two effects), a migration
  - `df-agent-channel`: `session.rs`, `scheduler.rs`, `tools.rs`, `server.rs`
  - `factory-agent`: `runner.rs`
- Schema: `runs.finalize_reason text NULL` with a `CHECK`. Runs that are `finalizing` when the migration runs get `stop` if `stop_requested_at` is set, else `idle`. No installations exist yet, so the only effect on such a run is a possibly wrong command; the hard stop still ends it.
- Docs: ADR-0061 (written with this proposal), ADR-0055 status line, `crates/df-core/docs/run-transitions.md`. `df-core/docs` is not mounted into the published site. The mounted `crates/df-agent-channel/docs/configuration.md` changes: a "Run transitions" section (events, `finalize_reason` and its commands) and a fix of its state diagram (the runtime never sends `FINALIZING`).
- Order:
  - after `db-write-ownership`: the run SQL is in `df_db::runs`, and `list_overdue_runs` exists;
  - best before `orchestrator-crate`, which moves the shrunk `scheduler` and `runs`;
  - independent of `state-predicates`;
  - `iter3-task-tree` (subtree cancel) raises `StopRequested` through the task applier, so whichever lands second adapts.
