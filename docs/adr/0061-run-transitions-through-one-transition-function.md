# ADR-0061: Run transitions through one transition function

- **Status:** Accepted (planned, `run-state-machine`)
- **Date:** 2026-11-17
- **Related:** [ADR-0055](0055-task-transitions-through-one-transition-function.md) (amends "Run state stays where it is"), [ADR-0016](0016-runs-and-triggers.md), [ADR-0049](0049-lifecycle-events-outbox-and-ed25519-signatures.md)

## Context

ADR-0055 put every change of `tasks.status` behind one pure function and one applier, and left `runs.state` where it was. The run state is still written by 12 statements in `df-db` and `df-agent-channel`:

- the scheduler (insert, provisioning, sandbox started);
- the connect;
- the runtime's phases;
- the session's `finalizing` moves for idle release and budget cancel;
- the board's stop request;
- the task applier's `FinalizeIdleRun` and `StopRun` effects;
- the run end.

Each statement carries its own guard in its `WHERE` clause. `set_run_state_tx` accepts any target state. `enforce_timeouts` decides on a row read without a lock.

Why a run is `finalizing` isn't stored. The session works it out again after every reconnect from the task status, `stop_requested_at`, the budget deadline and the last turn end, and remembers what it sent in two flags. The runtime has four `AtomicBool`s for the control commands it received, with their precedence spread across the runner.

## Decision

- **Every change of a run's state goes through one pure function** in `df-core`: `run_transition(&RunSnapshot, RunEvent) -> Result<RunTransition, Rejected>`.
  - `RunSnapshot` holds the run's state, `stop_requested_at`, `connected_at`, `finalize_reason`, whether provisioning began, and whether the task has an undelivered message.
  - `RunEvent` names what happened:
    - `ProvisioningStarted`, `SandboxStarted`, `Connected`, `TurnStarted`, `TurnEnded`;
    - `Finalize(FinalizeReason)`, `StopRequested(StopScope)`;
    - `Ended(RunEnd)`;
    - `Overdue`, which ends the run when the stop timeout, the connect timeout or the hard stop has passed.
  - `RunTransition` is the new state, the run fields to write, and **effects**: publish `RunChanged`, enqueue a sandbox lifecycle event, raise a task event (`TurnStarted`, `TurnEnded`, `RunEnded`).
- **One applier in `df-db`** (`apply_run_event`) locks the run row, loads the snapshot, calls `run_transition` and writes the result in the caller's transaction. A task event goes to `apply_task_event`, which takes the task row lock after the run row, as ADR-0055 requires.
- **The task applier raises run events.** `FinalizeIdleRun` becomes `Finalize(Done)` and `StopRun` becomes `StopRequested`; both go through `apply_run_event`. Neither raises a task event, so a cascade is at most task → run or run → task.
- **`runs.finalize_reason`** (`idle`, `budget`, `stop`, `done`) is written with `finalizing`. The command a session sends follows from it alone: `idle` → `Release`, the others → `Cancel` with their reason. A session remembers only whether it has sent the command on its stream.
- **The overdue decision is pure**: `overdue(&RunSnapshot, now, &RunLimits) -> Option<RunEvent>`. The applier repeats it under the run lock.
- **The runtime keeps its control commands in one value** (`RunControl`) with one function for the outcome at the end of a turn, instead of four flags.
- The scheduler's slot accounting stays outside, as in ADR-0055.

## Consequences

- Run rules are unit tests on a pure function. The PostgreSQL tests check locking and writing.
- `set_run_state_tx` and the guards in `WHERE` clauses are gone. A transition the state does not allow returns `Rejected` (a command) or an empty transition (an event that changes nothing).
- A reconnecting session no longer reads the task status or computes the budget deadline to pick its command.
- One more column, set and cleared only by the applier.

## Alternatives considered

- **Run state as part of the task state machine** (one snapshot, one function). Rejected: a task has many runs over days, and a run has its own timeouts and a connection. One function would mix two lifecycles that only meet at run start, turns and run end.
- **Keep one function per transition in `df_db::runs`** (the state after `db-write-ownership`). Rejected: the rules stay SQL predicates, testable only against PostgreSQL. `set_run_state_tx` would still allow any target state.
- **Derive the session's command as today instead of storing `finalize_reason`.** Rejected: the derivation reads the task status and wall-clock deadlines and needs two session flags to avoid sending twice. The reason is known when the run moves to `finalizing`.
