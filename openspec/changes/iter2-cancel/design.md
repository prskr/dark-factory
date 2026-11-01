# Design

## Context

`Cancel` (stop the turn, then finalize) and `RUN_OUTCOME_CANCELLED` exist in the agent protocol. The runtime handles them (`acp-harness`), and the control plane already sends `Cancel` when the wall-clock budget is exceeded (`run-orchestration`). This change adds the human trigger, a stop timeout and the task-level `cancelled` status.

## Goals / Non-Goals

**Goals:**
- Reuse the existing `Cancel` path: the runtime still pushes and snapshots, so no work is lost.

**Non-Goals:**
- Closing the PR or commenting on the issue on cancellation (humans do that in the forge).
- Cancelling subtrees (comes with the task tree, ADR-0034).

## Decisions

### D1 — Stop marker
`runs.stop_requested_at` (nullable) is set in the stop transaction. It drives:
- the run-end mapping exception: only prompts with `queued_at > stop_requested_at` make the task `queued`; older undelivered prompts leave it `waiting`;
- the timeout scan: the existing `enforce_timeouts` loop also selects runs with `stop_requested_at < now() - 2 min` that are not terminal.

Sending `Cancel` follows the existing path for commands (idempotent, re-sent after reconnects by the replica holding the stream). The stop handler publishes `run_changed`, and the holding replica sends `Cancel` on that notification. A run without a connection (`pending`, `provisioning`, `connecting`) is torn down by the handler directly through `SandboxProvider`, outside the database transaction.

### D2 — Cancel task
One transaction: set `cancelled`, mark undelivered prompts `discarded` (new prompt state, excluded from delivery queries), close pending requests through the `iter2-agent-requests` close function, audit. Then the run is stopped as in D1.

The `RunReport` handler and message queueing both lock the task row (`FOR UPDATE`), so they are serialized. The handler reads the task status under that lock and only writes a new one in these cases:
- status still `cancelled` → stays `cancelled`;
- status `queued` (reopened by a message after the cancel) → stays `queued`. That message was queued after `stop_requested_at`, so the mapping in D1 also yields `queued`;
- any other status → mapped as usual.

The new run starts through the normal scheduler, which only starts runs for tasks without a non-terminal run. It therefore waits for the stopped run to end.

### D3 — UI
Two htmx `POST` actions on the task detail. "Cancel task" uses a native `<dialog>` for confirmation (no extra JS beyond Alpine). Live updates come through the existing `run_changed`/`task_changed` SSE events.

### D4 — Design first in PenPot
The boards of task 3.1 are drawn and approved before the UI code changes and are the reference for placement, wording and states. Alternative: implement first and draw afterwards. Rejected: PenPot is the master of the UI design (`docs/design-workflow.md`).

### D5 — Mark done marks the PR ready
After the `mark_task_done` transaction commits, the mark-done handler calls `update_pull_request(draft: false)` through the forge routes. Best effort: a failure is logged as a warning and not retried, the same as at run end. A PR that is already ready (task was `in_review`) stays unchanged. The run that mark done cancels ends `cancelled`, so the run-end path does not mark the PR ready a second time.

Without a PR, nothing is sent to the forge. The cancelled run still finalizes and may push, so its push report or the run-end retry would open a draft PR for the done task. `try_ensure_draft` therefore skips a task that is `completed`, which covers both triggers; it reads the status together with `pr_number` under its advisory lock, so a mark done that commits just before is seen. Alternative: open the PR and mark it ready right away. Rejected: the human decided the task is done without one.

No completion summary is posted: no agent wrote one. Later, the factory minion can write one from the transcript (`product/iterations/roadmap.md`). Alternative: post a generic "marked done by <actor>" comment. Rejected: noise for the reviewer.

To keep mark done the exception, the `complete_task` description in `factory-tools.v2.json` gets: "Also call it when the instructions leave nothing to do (already done, nothing to change, question answered), with a summary that says why: if you end your turn without it, the task waits for a human." Agents only see the tool descriptions and the persona's system prompt; `product/agent-playbook.md` has the same rule but reaches agents only as a skill (`iter4-skills`). Alternative: a hidden instruction block in every first prompt, like the `set_task_title` request. Rejected: the tool description is where the agent looks when it decides to call the tool, and it costs no extra message.

## Risks / Trade-offs

- [Stop during a push leaves a half-pushed state] → the runtime finalizes normally; Git pushes are atomic per ref.
- [Teardown after the stop timeout loses the WIP snapshot] → the task volume is kept, and the next run's prepare pushes unpushed commits.
- [Mark done gives the reviewer no summary] → accepted; the `complete_task` description asks agents to call it themselves when nothing is left to do (D5).
