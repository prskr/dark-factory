# Design

## Context

See proposal.md and ADR-0061. Write sites of `runs.state` and the run timestamps on today's code (`db-write-ownership` moves the `df-agent-channel` ones into `df_db::runs` without changing them):

| Site | Transition (guard) | Also writes / raises |
|---|---|---|
| `runs.rs:220` `start_queued_runs` | insert `pending` | task `RunStarted` |
| `runs.rs:571` `begin_provisioning` | `pending → provisioning` | `provisioning_started_at`, `sandbox.provisioning` |
| `runs.rs:505` `mark_started` | `pending, provisioning → connecting` | `pod_ip`, `sandbox_started_at`, `sandbox.provisioned` |
| `runs.rs:536` `mark_connected` | `pending, provisioning, connecting → preparing`; other non-terminal: unchanged | `connected_at` (no transaction, no lock) |
| `runs.rs:261` `set_run_state_tx` | non-terminal → **any** | `pod_ip`; callers: `apply_phase` (`Preparing`, `Finalizing`) |
| `runs.rs:304` `set_idle_tx` | not ending → `awaiting_input` / `running` | task `TurnEnded` / `TurnStarted` |
| `session.rs:312` `start_finalizing(true)` | `awaiting_input → finalizing` if no undelivered message | idle `Release` follows |
| `session.rs:321` `start_finalizing(false)` | not ending → `finalizing` | budget `Cancel` follows |
| `tasks.rs:1154` `request_stop_tx` | sets `stop_requested_at`; `preparing, running, awaiting_input → finalizing` | board stop, `StopRun` effect (`MarkedDone` by merge, `Cancelled`) |
| `task_events.rs:453` `finalize_idle_run` | `awaiting_input → finalizing` | `FinalizeIdleRun` effect (`MarkedDone` by board) |
| `runs.rs:426` `end_run_tx` | non-terminal → `outcome.run_state()` | outcome fields, `ended_at`, `sandbox.*` event, task `RunEnded`, task fields |
| `scheduler.rs:414` `enforce_timeouts` | decides `Ended` for overdue runs on an unlocked read | |

Commands a session sends for a `finalizing` run, as `reconcile_finalizing` (`session.rs:369`) derives them today:

| Cause | Derived from | Command |
|---|---|---|
| board marked done | task `completed` | `Cancel("task marked done")` |
| stop (board, merge, cancel) | `stop_requested_at` set | `Cancel("run stopped by a board member")` |
| budget | budget deadline passed | `Cancel("wall-clock budget exceeded")` (sent by the budget timer) |
| idle deadline | none of the above, a turn ended | `Release` |

## Goals / Non-Goals

**Goals:**
- Every write of `runs.state`, `finalize_reason`, `stop_requested_at` and the lifecycle timestamps goes through `apply_run_event`.
- The rules of both tables above are unit tests on pure functions.
- Same states, commands, lifecycle events and task events as today.

**Non-Goals:**
- See proposal, Out of Scope.

## Decisions

### 1. Shape follows ADR-0055

```rust
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,
    /// A time, not a bool: `overdue` needs it for the stop timeout.
    pub stop_requested_at: Option<DateTime<Utc>>,
    pub connected_at: Option<DateTime<Utc>>, // budget start: connected_at, else started_at
    pub provisioning_began: bool,
    pub finalize_reason: Option<FinalizeReason>,
    pub undelivered_messages: bool,
    pub started_at: DateTime<Utc>,
}

pub enum RunEvent {
    ProvisioningStarted,
    SandboxStarted { pod_ip: String },
    Connected,
    TurnStarted,
    TurnEnded,
    Finalize(FinalizeReason),
    StopRequested(StopScope),
    Ended(RunEnd),
    /// Becomes `Ended` if `overdue` says so on the locked snapshot (decision 4).
    Overdue { now: DateTime<Utc>, limits: RunsConfig },
}

pub struct RunTransition {
    pub state: RunState,
    pub set: RunFields,          // timestamps (stamps), pod_ip, outcome fields, finalize_reason
    pub effects: Vec<RunEffect>, // Publish, Sandbox* (lifecycle events), Task(TaskCue)
}
```

`limits` is the existing `RunsConfig`, not a new type. The applier takes a `RunEnv` (the lifecycle sinks and the resume policy of the task event for a run end) next to the event; `RunEnv::new(events, max_auto_resumes, resume_backoff)` builds it, `RunEnv::events_only(events)` serves the events that enqueue lifecycle events but end no run, and `RunEnv::none()` the events that need neither. It returns `Option<AppliedRun>` (`None` for a missing run).

`RunEnd` moves from `df_db::runs` to `df-core` with it. The overdue causes (stop timeout, connect timeout, hard stop) produce `Ended` with the outcome and detail used today. Insertion stays in `start_queued_runs`: a new run has no snapshot. The insert writes `pending` and raises task `RunStarted` as today. `run_transition` starts at `pending`.

An event that changes nothing returns an empty transition, like a repeated `Connected` or `TurnEnded` on a `finalizing` run. `Rejected` is reserved for commands, of which runs have one: `StopRequested` on a terminal run. Callers that return whether something changed (`begin_provisioning`, `mark_started`, `start_finalizing`) keep their `bool` from "the transition changed the run" (it has effects), which also covers a change of the reason alone.

### 2. Applier and lock order

`apply_run_event(tx, run_id, event, env)` does the following:
- `SELECT … FOR UPDATE` on the run;
- reads the undelivered flag (the task lock is taken first where the rule needs it, see below);
- calls `run_transition`;
- writes the row;
- runs the effects in list order.

A `Task(event)` effect calls `apply_task_event`, which locks the task's run rows (already held) and then the task row. That's the order of ADR-0055 and `lock_run_and_task`.

`Finalize(Idle)` reads undelivered messages. Today `start_finalizing` takes `lock_run_and_task` before it checks, so a concurrent `queue_message` serializes on the task row. The caller keeps doing that: it takes `lock_run_and_task`, then calls the applier. The applier's run lock is then a no-op, because the transaction already holds it.

In the other direction, the task applier's effects `FinalizeIdleRun` and `StopRun` call `apply_run_event` for the task's active run. The run rows are already locked by `apply_task_event`, so nothing new is locked. `Finalize` and `StopRequested` raise no task event, so a cascade stops after one hop (ADR-0061).

### 3. `finalize_reason`

`runs.finalize_reason text NULL CHECK (finalize_reason IN ('idle','budget','stop','done'))`. The column is written with `finalizing`, by:
- `Finalize(reason)`;
- `StopRequested`, which writes `stop` only if the run moves to `finalizing`.

On a run that is already `finalizing`, `Finalize(reason)` replaces `idle` with a `Cancel` reason (`budget`, `done`) and changes nothing else (state unchanged, publish), so the stream that sent the `Release` sends that `Cancel` once. A `Cancel` reason is never replaced. That keeps today's precedence: the budget timer sends its `Cancel` after an idle `Release` (`session.rs:271`), and a task marked done gets its `Cancel` on an idle-finalizing run (`session.rs:358`). A session that starts after the budget deadline applies `Finalize(Budget)` before its first reconcile, so it never sends an idle `Release` then.

A `StopRequested` with `IncludingFinalizing` on a run that is already `finalizing` sets `stop_requested_at` and changes the reason to `stop`. That's today's effect: `stop_requested_at` makes `reconcile_finalizing` send the stop `Cancel`.

`FinalizeReason::command()` returns `Release` for `idle` and `Cancel(reason text)` for the others, and alone decides between them. The session records the last command it sent on its stream (`command_sent: Option<RunCommand>`), not a "sent" flag. `reconcile_finalizing` becomes:
1. read the run;
2. return if it's not `finalizing`, or this stream already sent that same command;
3. otherwise send `command()` and record it.

So a stop that changes the reason of a released run (`idle` → `stop`) sends the `Cancel` after the `Release`, once.

The budget timer goes through `Finalize(Budget)` and then sends its `Cancel` the same way. It stops calling `start_finalizing(false)` and `send_cancel` separately.

`Release` today also requires a turn that ended (`last_turn_end`). `Finalize(Idle)` is only raised by the idle timer, which is armed from a turn end, so this condition holds by construction. Task 3.2 pins it with the existing reconnect tests.

### 4. Timeouts

`overdue(&RunSnapshot, now, &RunsConfig) -> Option<RunEvent>` is the `match` of `enforce_timeouts` (`scheduler.rs:420`) moved verbatim, so the four branches and their order don't change. `enforce_timeouts` lists candidates with `list_overdue_runs` (from `db-write-ownership`, decision 9). For each, it calls `apply_run_event(Overdue { now, limits }, env)`. The applier evaluates `overdue` on the locked snapshot and applies the resulting `Ended`, or nothing if the run moved meanwhile. `finish_run` (teardown) follows as today when a run was ended.

### 5. Runtime: `RunControl`

`factory-agent/src/runner.rs` replaces the four `AtomicBool`s with one `std::sync::Mutex<RunControl>`:

```rust
struct RunControl { cancelled: bool, released: bool, end_run_after_turn: bool, await_input: bool }
```

It gets methods `on_cancel`, `on_release`, `on_end_run_requested`, `on_await_input` and `turn_outcome(&self) -> Option<ControlOutcome>`. `turn_outcome` holds the precedence that is spread over `runner.rs:1059-1140` today: cancel before end-run before release. `await_input` only picks `Blocked` over `Released` within a release; `release_outcome()` returns `Blocked` whenever `await_input` is set, whatever else is set, as before. The guard is never held across `.await`. The cancel-safe `select!` branch at `runner.rs:473` keeps its single idempotent side effect, now `on_cancel`.

### 6. Diagram checked by a test

`crates/df-core/docs/run-transitions.md` has a Mermaid state diagram and an event table, written by hand from the arms of `run_transition`. A test checks that every `RunState`, `RunEvent` and `FinalizeReason` appears in it (as `the_status_diagram_names_every_status_and_event` does for tasks).

## Intended differences

The states, commands, lifecycle events and task events stay the same, with these exceptions that follow from storing the reason:

- A PR merge (`StopRun(ActiveOnly)`) now stores `stop` and the session sends `Cancel("run stopped by a board member")` instead of `Cancel("task marked done")`. The runtime ignores the reason text.
- A run that is already `finalizing` for `idle` or `budget` gets no extra `Cancel` on a merge: `ActiveOnly` leaves a `finalizing` run alone.
- A run that is `finalizing` for `stop` or `done` gets no second `Cancel` with the budget text when its budget deadline passes; it keeps its reason.
- A run `finalizing` for `idle` still gets the budget `Cancel` (the budget takes over the reason, see decision 3), and a stream that starts after the budget deadline sends that `Cancel`, never the idle `Release`.
- A reconnect of an unchanged run publishes no `RunChanged`. Every consumer of `RunChanged` only re-reads state.
- A run stopped before it connected, whose sandbox connects before the scheduler ends it, moves to `finalizing` with reason `stop` on `Connected`, so the session sends the stop `Cancel`. Before, it moved to `preparing` and was only torn down at the stop timeout.

## Risks / Trade-offs

- [Behavior drift while moving 12 guards into one function.] → Group 1 writes the unit tests from the guards in the table above before any caller switches. The existing PostgreSQL tests of `runs`, `session`, `scheduler` and `task_events` must pass unchanged.
- [`finalize_reason` replaces a derivation that was correct after reconnects.] → The reconnect tests in `session/tests.rs` (cancel and release resent after a lost stream) are the acceptance check.
- [One more lock acquisition per event.] → All callers already lock the run or the task row; the applier's lock is the same row.
