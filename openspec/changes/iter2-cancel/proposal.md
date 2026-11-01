# Proposal

## Why

A board member cannot stop an agent that went the wrong way or cancel a task that is no longer needed. Iteration 1 deferred the cancel button. With approvals and push-check exceptions, abandoned tasks would also leave pending requests in the inbox forever (ADR-0038 hygiene).

## What Changes

- **Stop run:** sends `Cancel` to a connected runtime, which stops the turn and finalizes (push, WIP snapshot, `RunReport` `CANCELLED`). A run that isn't connected yet is torn down directly. The task becomes `waiting` even if undelivered messages exist; they are delivered with the next message.
- **Stop timeout:** if no `RunReport` arrives within 2 minutes after `Cancel`, the sandbox is torn down, the run ends `cancelled` with detail `stop-timeout`, and the task volume is kept.
- **Cancel task:** new task status `cancelled`. Cancelling stops the active run as above, marks undelivered messages as discarded and closes pending requests (`task-cancelled`). The PR and the issue stay untouched. Queueing a message reopens a cancelled task (status `queued`).
- **Mark done marks the PR ready:** when a board member marks a task done, its PR (if any) is marked ready for review. No completion summary is posted, since no agent wrote one; this is an accepted trade-off. A task marked done without a PR doesn't get one later, even if its cancelled run still pushes.
- **UI:** "Stop" on an active run and "Cancel task" (with confirmation) on the task detail, both phone-ready. Both actions are audited (`run.stop`, `task.cancel`).
- **PenPot:** the task detail boards (Desktop, Phone) get both actions, the cancel confirmation gets its own board and `Status badge` a `cancelled` variant, before the code changes.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `task-management`: `cancelled` status, cancel and reopen behaviour, discarded messages, mark done marks the PR ready.
- `pull-request-lifecycle`: marking a task done by hand marks the PR ready; no PR is opened for a `completed` task.
- `run-orchestration`: stopping a run, stop timeout, and the stop exception in the run-end mapping.
- `agent-requests` (introduced by `iter2-agent-requests`): pending requests close on cancellation.
- `task-ui` (introduced by `iter1-control-plane-web`): stop and cancel actions.

## Impact

- Code: `df-db` migration (task status `cancelled`, `runs.stop_requested_at`, prompt state `discarded`), control plane run service (stop, timeout scan), `df-web` actions, the `df-web` mark-done handler calls the forge after the status change. No contract change: `Cancel` and `RUN_OUTCOME_CANCELLED` already exist.
- **Archive order:** after `iter2-agent-requests`, because both modify `Task status vocabulary` and `Run end maps to task status`; the deltas here include `blocked`.
- PenPot file "App Design": task detail boards (Desktop, Phone), cancel confirmation board, `Status badge` component; mapping rows in `docs/design-workflow.md`.
