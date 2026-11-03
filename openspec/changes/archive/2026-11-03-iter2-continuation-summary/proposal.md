## Why

When a follow-up run cannot resume the harness session (`session/load` unsupported, session files lost with the PVC), the new session is seeded with `Continuation.summary`. Today that summary holds the task header and the last 20 text messages, rebuilt on every `Assign` from the last 500 transcript rows. Tool activity is missing, so the agent does not know which files it changed or which commands failed. On long tasks everything older than the row window silently disappears. Runs that start without a human (`iter2-run-resilience`) and chats (iteration 3, ADR-0026) rely on this fallback more often, so its shape should be right before they ship.

## What Changes

- **Tool activity in the summary.** One compact line per tool call (final title and status, merged from `tool_call` and `tool_call_update` events), interleaved with the text messages in transcript order. Consecutive successful calls collapse into one line (`Tool: 37 calls completed`); failed or unfinished calls stay individual.
- **Built from the whole transcript.** `Assign` builds the summary from all transcript events of the task except those of the run being assigned. No row window: what is dropped is decided by the size cap alone.
- **Priority at the cap.** The cap rises from 16 KiB to 64 KiB. When it is exceeded, tool lines and agent messages are dropped oldest first before any user message.
- **Built-in strategy only.** The summary is a pure function `(events, budget) → summary`, so a plugin can replace it later (F3). No model call, no plugin interface, no stored state.

Out of scope, waits for the F3 plugin: model-generated summaries, a stored summary with an incremental cursor (needed once a strategy is too expensive to rerun per `Assign`), token budgets, a WIT world, mid-run compaction (the harness owns its context inside a run, ACP has no hook), per-persona or per-project strategies.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `run-orchestration`: continuation summary content and how it is built.

## Impact

- `df-agent-channel`: `assign::transcript_summary` replaced by a pure fold in `summary.rs` and one query over the task's transcript in `Assign`.
- No migration, no contract change: `Continuation.summary` keeps its type and meaning. The runtime is unchanged.
- Docs: `design/agent-run.md` (continuity paragraph), F3 in `product/context.md` (input, output, trigger as described in this change's design).
- Overlaps `iter2-run-resilience` (resumed runs use the same summary, no conflict) and ADR-0026 chats (reuse this summary instead of a second mechanism).
