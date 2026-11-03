## Context

`assign::transcript_summary` reads the last 500 rows of kinds `user_message` and `agent_message_chunk`, merges chunks, keeps the last 20 messages and fits them into 16 KiB (`fit_summary`). It runs on every `Assign`, including when `session/load` will succeed. The runtime prepends the result only when the session was not resumed (`runner.rs`, `is_resumed`). Which events exist: `contract/proto/darkfactory/transcript/v1/transcript.proto` (`ToolCall`, `ToolCallUpdate` with `tool_call_id`, `title`, `status`).

This change ships the part of F3 that does not need a model, in the shape a plugin can later replace.

## Goals / Non-Goals

**Goals:**
- The fallback tells the agent what it did, not only what was said.
- Old history is not lost to a row window; only the size cap drops entries, and it drops the least useful ones first.
- The computation is a pure function over the events, testable without a database.
- No stored state: nothing to keep consistent across reconnects, late batches or runs that end without a report.

**Non-Goals (wait for the F3 plugin):**
- Model-generated or semantic summaries.
- A stored summary with a cursor and incremental updates (D6).
- Token-based budgets (the cap stays in bytes).
- A WIT world, host inference access, plugin selection per persona or project.
- Mid-run compaction: ACP offers no hook, the harness manages its context inside a run.
- Exporting or restoring harness sessions (agent-specific, see the discussion of PVC loss: the fallback is the safety net).

## Decisions

### D1 — Rebuilt on every `Assign`, without the assigned run
`Assign` of run N ≥ 2 reads all transcript events of the task with kinds `user_message`, `agent_message_chunk`, `tool_call`, `tool_call_update` and `run_id <> <run being assigned>`, ordered by `id`, and folds them. A reconnect mid-run therefore never sees its own half-finished run, and late batches of an ended run are simply included. Runs that ended without a report (lost sandbox, hard timeout) need no special path. The query selects only the payload fields the fold uses (text blocks with their `hidden` flag, title, status, turn and tool call ids), not raw tool input or output. *ponytail:* reads the whole transcript once per `Assign`; store the result with a cursor (D6) when a strategy is too expensive to rerun or a task's transcript makes the query measurably slow.

### D2 — The summary is a list of entries, rendered at the end
The fold produces entries (`"User: …"`, `"Agent: …"`, `"Tool: cargo test (failed)"`) in transcript order; the control plane renders the task header (title and description, cut to 4 KiB) followed by the kept entries, joined by blank lines. User entries contain only the visible blocks of a message (`hidden` blocks, see `hide-system-prompt-blocks`). Consecutive chunks of one agent message are merged into one entry.

### D3 — Tool calls: one line per call, successful streaks collapsed
`tool_call` and `tool_call_update` are merged by `(run_id, turn_id, tool_call_id)` (the id is unique within a turn only, and a run can have several turns). The line is `Tool: <title> (<status>)` with the last known title and status. Two or more consecutive calls with status `completed` (no other entry between them) become `Tool: <n> calls completed`; a failed, pending or in-progress call stays its own line and breaks the streak. Raw input and output are not included: they are large and the workspace on the PVC or the pushed branch carries the result. Tool calls with origin `FACTORY` are included like any other.

### D4 — Cap and drop order
The cap is 64 KiB (the header's 4 KiB included). When the entries exceed what is left after the header, tool lines and agent messages are dropped oldest first; user messages are dropped (oldest first) only when no other entry is left. The newest entry is always kept, shortened if it alone exceeds the budget. Entries keep their transcript order after dropping.

### D5 — A run that is still active is not covered
The summary covers the events of all runs except the one being assigned. A follow-up run only starts after the previous run ended, so its summary is complete at that point. Keep-alive (`iter1-keep-alive-and-review`) continues in the same run and the same harness session, where no summary is used.

### D6 — F3 interface, recorded here, built later
A plugin strategy takes: previous summary text, transcript events after a cursor (including tool calls), and a budget. It returns the new entries text (no header, the control plane adds it) and the new cursor. Because a model-based strategy is too expensive to rerun per `Assign`, F3 adds the stored state this change does without: a table `task_summaries(task_id, covers_through_event_id, text)` updated at run end or idle, never in the `Assign` path. The built-in fold of this change stays the default strategy and the fallback when no stored summary exists.

## Risks / Trade-offs

- [Reading the whole transcript per `Assign` gets slow on very long tasks] → `Assign` happens once per run connect, the query uses `transcript_events_task_id_idx` and skips raw tool payloads. Store the result (D6) if it shows up in measurements.
- [User messages can fill the cap on their own] → unlikely at 64 KiB; the oldest user messages go first, the header keeps the task's goal.
- [Collapsed streaks hide which files were read or edited] → the workspace or the pushed branch shows the result; failed calls stay visible.
- [The summary can contain secrets typed into messages] → same exposure as the transcript, which the harness already saw; not a new boundary.
