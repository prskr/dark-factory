## Why

When a follow-up run cannot resume the harness session (`session/load` unsupported, session files lost with the PVC), the new session is seeded with `Continuation.summary`. Today that summary holds the task header and the last 20 text messages, rebuilt from scratch on every `Assign` from the last 500 transcript rows. Tool activity is missing, so the agent does not know which files it changed or which commands failed. On long tasks everything older than the row window silently disappears. Runs that start without a human (`iter2-run-resilience`) and chats (iteration 3, ADR-0026) rely on this fallback more often, so its shape should be right before they ship.

## What Changes

- **Tool activity in the summary.** The built-in summary gets one compact line per tool call (final title and status, merged from `tool_call` and `tool_call_update` events by `tool_call_id`), interleaved with the text messages in transcript order.
- **Stored summary with a cursor.** The control plane stores the summary per task together with the id of the last transcript event it covers (`task_summaries`). `Assign` reads the stored text, after folding any events the cursor does not cover yet.
- **Incremental update at run end.** When a run ends, the control plane folds the transcript events after the cursor into the stored summary, within the existing size cap. The update runs once per run end, not per `Assign`.
- **Built-in strategy only.** The update is a pure function `(previous summary, new events, budget) → summary`, so a plugin can replace it later (F3). No model call, no plugin interface.

Out of scope, waits for the F3 plugin: model-generated summaries, token budgets, a WIT world, compaction of the summary itself, mid-run compaction (the harness owns its context inside a run, ACP has no hook), per-persona or per-project strategies.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `run-orchestration`: continuation summary content, storage and update point.

## Impact

- `df-db`: migration `task_summaries (task_id, covers_through_event_id, text, updated_at)`, `.sqlx` metadata.
- `df-agent-channel`: `assign::transcript_summary` split into a pure `fold_summary` and a run-end hook in `session`/`runs`; `Assign` reads the stored row.
- No contract change: `Continuation.summary` keeps its type and meaning. The runtime is unchanged.
- Docs: `design/agent-run.md` (continuity paragraph), F3 in `product/context.md` (input, output, trigger as described in this change's design).
- Overlaps `iter2-run-resilience` (resumed runs use the same summary, no conflict) and ADR-0026 chats (reuse the stored summary instead of a second mechanism).
