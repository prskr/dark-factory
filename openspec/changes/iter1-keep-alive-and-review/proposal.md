## Why

Chatting with an agent is expensive and slow: every run ends with its turn, so each follow-up message provisions a new sandbox, re-prepares the workspace and loads the session again. Every run also ends as `waiting`, because `completed` is only reached when the agent calls `complete_task`, and a human cannot close a task or tell "the agent is done, review the PR" from "the agent waits for an answer".

## What Changes

- **Keep-alive after a turn.** When a turn ends normally with no queued prompts, the runtime reports `RUN_PHASE_AWAITING_INPUT` and keeps the run and sandbox alive until the control plane sends a `Prompt`, `Cancel` or the new `Release`. The control plane owns the idle deadline: last turn end plus `runs.keepAliveSeconds` (default 600, `0` restores today's behavior). A message before the deadline starts the next turn in the same run, with the same harness session. The run's state is `awaiting_input` and the task shows `waiting`. At the deadline the control plane sends `Release`; the run ends `END_TURN` as today. A wall-clock budget used up while idle also ends it `END_TURN`. Keeping the deadline in the control plane lets later triggers (monitors, webhooks, approvals) extend it without a contract change.
- **New task status `in_review`.** `complete_task` (outcome `completed`) sets the task to `in_review` instead of `completed`; the PR is marked ready as before. `in_review` belongs to the "Waiting" filter group.
- **Mark done.** A board member can mark a task done from the task page. The task becomes `completed`. An idle run of the task is cancelled; its run end does not change the status back.
- **BREAKING (pre-MVP, no data to keep):** `completed` now means "a human closed the task", not "the agent called `complete_task`". The status check constraint gains `in_review`.

Out of scope: closing tasks when the PR is merged (needs webhooks, ADR-0015), reverting a ready PR to draft when an `in_review` task gets a new message.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `acp-harness`: wait for `Prompt`, `Cancel` or `Release` after a normal turn end.
- `run-orchestration`: idle deadline and `Release`, `awaiting_input` phase, run end mapping for `completed` and for a task marked done, prompts to an idle run.
- `task-management`: status vocabulary (`in_review`), queued message on a task with an idle run, mark done.
- `pull-request-lifecycle`: ready for review when the agent completes (task `in_review`).
- `task-ui`: "Mark done" action, `in_review` badge and filter group.

## Impact

- Contract: `agent.proto` new control message `Release`. `RUN_PHASE_AWAITING_INPUT` exists already. No keep-alive field in `Assign`.
- `factory-agent`: idle wait in the turn loop (`harness::determine_run_outcome`, `runner`).
- `df-agent-channel`: idle deadline and `Release` in the session, phase handling for `AwaitingInput`, `runs::task_status_for`, mark-done cancel.
- `df-core`: `TaskStatus::InReview`, `RunsConfig::keep_alive_seconds`, status groups.
- `df-db`: migration for the status constraint, `queue_message`, mark-done query, `.sqlx` metadata.
- `df-web`: mark-done action and route, badge, filters.
- Docs and Helm: `crates/df-agent-channel/docs/configuration.md`, chart values.
- Design: PenPot "Status badge" variant "In review" and the "Mark done" action in the task header.
- Overlaps `iter2-agent-requests` (it planned `Assign.keep_alive` for `AwaitingInput`; it reuses the idle wait and sets its own deadline in the control plane instead) and `iter2-cancel` (status requirements): both rebase on this change.
