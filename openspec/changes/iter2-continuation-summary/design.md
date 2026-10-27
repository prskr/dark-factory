## Context

`assign::transcript_summary` reads the last 500 rows of kinds `user_message` and `agent_message_chunk`, merges chunks, keeps the last 20 messages and fits them into 16 KiB (`fit_summary`). It runs on every `Assign`, including when `session/load` will succeed. The runtime prepends the result only when the session was not resumed (`runner.rs`, `is_resumed`). Which events exist: `contract/proto/darkfactory/transcript/v1/transcript.proto` (`ToolCall`, `ToolCallUpdate` with `tool_call_id`, `title`, `status`).

This change ships the part of F3 that does not need a model, in the shape a plugin can later replace.

## Goals / Non-Goals

**Goals:**
- The fallback tells the agent what it did, not only what was said.
- Old history is not lost because a run produced many events.
- The summary is normally computed once per run end; `Assign` only folds what is still uncovered.
- The computation is a pure function over `(previous summary, events)`, testable without a database.

**Non-Goals (wait for the F3 plugin):**
- Model-generated or semantic summaries.
- Token-based budgets (the cap stays in bytes).
- A WIT world, host inference access, plugin selection per persona or project.
- Compacting the stored summary itself; the size cap drops the oldest entries.
- Mid-run compaction: ACP offers no hook, the harness manages its context inside a run.
- Exporting or restoring harness sessions (agent-specific, see the discussion of PVC loss: the fallback is the safety net).

## Decisions

### D1 — Stored summary with a cursor
`task_summaries(task_id PK, covers_through_event_id BIGINT, text TEXT, updated_at)`. `covers_through_event_id` refers to `transcript_events.id` (the task-wide ordering already used by `transcript_events_task_id_idx`). Updating reads `WHERE task_id = $1 AND id > $cursor AND kind IN (...) ORDER BY id`, folds, upserts. The upsert only writes when the new cursor is ahead of the stored one (`ON CONFLICT … DO UPDATE … WHERE task_summaries.covers_through_event_id < EXCLUDED.covers_through_event_id`), so a delayed update cannot replace a newer summary. *ponytail:* one row per task, no history of summaries; add versions when a plugin needs to compare strategies.

### D2 — The summary is a list of entries, rendered at the end
User entries contain only the visible blocks of a message (`hidden` blocks, see `hide-system-prompt-blocks`), in incremental updates and rebuilds alike. Folding works on rendered entries (`"User: …"`, `"Agent: …"`, `"Tool: cargo test (failed)"`), so the stored text is the entries joined by blank lines under the header. To append, the stored text is split back into entries on the separator and the header is rebuilt from the task. Entries never contain the separator (it is replaced by a single newline when an entry is created). *Alternative:* a JSON column with entries. Rejected for now: the text is what `Assign` sends, and a plugin returns text.

### D3 — Tool calls: one line per `tool_call_id`
Events `tool_call` and `tool_call_update` are merged by `(run_id, turn_id, tool_call_id)` (the id is unique within a turn only, and a run can have several turns). The line is `Tool: <title> (<status>)` with the last known title and status. Raw input and output are not included: they are large and the workspace on the PVC or the pushed branch carries the result. Tool calls with origin `FACTORY` are included like any other.

### D4 — Update at run end, on the finalize path
The update runs where the run report is persisted (`session.rs`, next to the `harness_session_id` update), after the final transcript events are stored. A failure is logged as a warning and does not fail the run end: the catch-up in `Assign` (D5) folds the missing events, because the cursor only advances on success.

### D5 — `Assign` catches up before it reads
`Assign` runs the same update as the run end (`update_task_summary`) before reading the row: it folds the events after the cursor, or the whole transcript if no row exists (tasks before this change), and stores the result. Normally there is nothing to fold. This covers every way a run can end (report, lost sandbox, hard timeout in `scheduler.rs`/`finish_run`, a failed or skipped update) with one code path and needs no data migration. The built-in fold is cheap and incremental, so it is fine in the `Assign` path; D7 forbids it for a model-based strategy.

### D6 — A run that is still active is not covered
The summary covers events up to the last run end. A follow-up run only starts after the previous run ended, so the summary is current at that point. Keep-alive (`iter1-keep-alive-and-review`) continues in the same run and the same harness session, where no summary is used.

### D7 — F3 interface, recorded here, built later
A plugin strategy takes: previous summary text, transcript events after the cursor (including tool calls), and a budget. It returns the new summary text and the new cursor. The control plane persists the result and runs it at run end or idle, never in the `Assign` path (the built-in catch-up of D5 stays). The built-in fold of this change is the default strategy.

## Risks / Trade-offs

- [Splitting the stored text back into entries is fragile] → entries are created without the separator; a test round-trips a summary through split and join. If it proves brittle, switch to a JSON column (D2).
- [Tool lines can crowd out messages within 16 KiB] → the oldest entries drop first, and a tool line is short. A noisy run (hundreds of calls) still keeps the recent part. A per-run cap on tool lines is the first knob if it matters.
- [A crash between report persistence and summary update] → the cursor did not advance, `Assign` folds the missing events (D4, D5).
- [The summary can contain secrets typed into messages] → same exposure as the transcript, which the harness already saw; not a new boundary.
