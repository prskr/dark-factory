# Design

## Context

See proposal.md for the motivation. Current behavior:

- **Stop** (`df_db::stop_run` → `request_stop_tx`):
  - Sets `runs.stop_requested_at` and moves the run to `finalizing`.
  - The session's `reconcile_finalizing` sends `Cancel`.
  - The runtime's `await_prompt` (`crates/factory-agent/src/runner.rs`) sets `cancelled`, sends ACP `session/cancel`, and waits up to `CANCEL_RESPONSE_TIMEOUT` for the prompt response.
  - `determine_run_outcome` (`harness.rs`) then finalizes with `CANCELLED`.
- **Session cancel:** ACP `session/cancel` is only used on that path and for the wall-clock deadline.
- **Turn end:** the runtime reports `TurnEnded{stop_reason, detail}`. After a normal turn end it continues with queued prompts or waits for input (`idle_until_next_prompt`).
- **Busy flag:** the control plane tracks whether a turn is open per stream (`busy`, set by persisted `TurnStarted`/`TurnEnded`). The DB run state is `running` during a turn and `awaiting_input` while idle.
- **Dark Factory tool calls** wait in the runtime's `pending_tool_calls` for a `ToolCallResult` from the control plane.

This change builds on `conversation-flow` (one `Prompts` control message per release).

## Goals / Non-Goals

**Goals:**
- End the current turn and keep the run, the harness session and the workspace as they are.
- Messages waiting at the interrupt start the next turn without extra steps.

**Non-Goals:**
- Interrupting a run that is provisioning or finalizing: rejected; Stop run covers it.
- Rolling back what an interrupted turn did (commits, created tasks, raised questions): rejected; the agent continues from the current state and is told it was interrupted.
- A "send and interrupt" action on a held-back message: backlog (`product/iterations/roadmap.md`).

## Decisions

### D1: Protocol

- `ControlMessage.body` gets `Interrupt { string by = 1; }`, where `by` is the board member's display name.
- `TurnEnded` gets `string interrupted_by = 5`. The web UI renders the row from this field, not from the free-text `detail`, and the runtime also sets `detail` to "Interrupted by <name>" for logs.

### D2: Control plane path without persistence

`POST /tasks/{key}/interrupt` calls `df_db::interrupt_turn(task, actor)`, which in one transaction:
1. Locks the task's active run.
2. If its state is not `running`, returns `NoTurnRunning` (the handler shows the notice).
3. Writes the audit entry `run.interrupt`.
4. Publishes `Notification::RunInterrupt { run_id, by }` (PostgreSQL `NOTIFY`, as other notifications).

Every replica receives the notification. The one holding the run's stream sends `Interrupt` if its `busy` flag is set, and otherwise drops it. No row stores the interrupt, so a reconnect or a later turn never sees it (spec "Runtime not connected").

Alternative: a `runs.interrupt_requested_at` column reconciled like `stop_requested_at`. Rejected: a stored interrupt could hit a later turn, and the decision was best effort.

### D3: Runtime

**Receiving.** `handle_control_message` puts `by` into `shared.interrupt` (a `Mutex<Option<String>>`) and sends on a new broadcast `interrupt_tx`. Outside a turn the receiver isn't polled, so the value is cleared when the next turn starts.

**Cancelling the turn.** `await_prompt` gets a select branch for `interrupt_rx`. It does not set `cancelled`. It sends `session/cancel` and waits for the prompt response with the same `CANCEL_RESPONSE_TIMEOUT`.

**Open tool calls.** On interrupt, every entry in `pending_tool_calls` is completed with a cancelled `ToolCallResult` ("Interrupted by <name>"), so the harness's MCP call returns. A late result from the control plane finds no entry and is dropped, as today.

**Outcome.** `TurnOutcomeInput` gets `interrupted: bool`. `determine_run_outcome` checks `cancelled` and the budgets first, unchanged. Then an interrupted turn gives:
- `ContinueNextTurn` with queued prompts;
- `AwaitInput` otherwise, regardless of the stop reason the harness reports.

**Budgets.** The turn counter already counted the turn at `begin_turn`, so `max_turns` applies as usual.

**Next input.** `begin_turn` takes `interrupted_by` from the previous turn and puts the text block "Your previous turn was interrupted by <name>." before the prompts' blocks.

### D4: Web UI

- **Interrupt button:** `composer_action` shows **Interrupt** (`hx-post /tasks/{key}/interrupt`) while the active run is `running`. That is the existing `stoppable` check narrowed to the run state, updated live through the run-changed SSE events. The notice "No turn is running" uses the existing toast.
- **Stop run:** the status switch gets **Stop run** (`/tasks/{key}/stop`, unchanged), and the run row's button is relabelled **Stop run**.
- **Transcript:** the Folder renders a `TurnEnded` with `interrupted_by` as a notice-style row "Interrupted by <name>".

### D5: Task status and resumes

An interrupt changes no task state directly. The run goes from `running` to `awaiting_input` through the usual phase report, and the task follows as after a normal turn end. Automatic resumes are only used when a run ends, so an interrupt can't consume one.

## Risks / Trade-offs

- [A harness ignores `session/cancel` or answers late.] → After `CANCEL_RESPONSE_TIMEOUT` the turn is treated as ended with an error response. If the harness then misbehaves, the board member falls back to Stop run.
- [The race between a turn ending and the interrupt arriving.] → The runtime ignores an `Interrupt` outside a turn. The control plane's `busy` check drops most of them earlier.
- [Two board members interrupt at once.] → The first ends the turn and the second is ignored by the runtime; both are audited.

## Migration Plan

No data migration. Deploy the control plane and the runtime image of the same release, after or together with `conversation-flow`.
