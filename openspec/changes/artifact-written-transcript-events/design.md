# Design

## Context

See proposal.md for the motivation; the requirements are in the delta specs (`factory-activity`, `task-ui`, `artifact-ui`).

Current state:
- **Detection in the web UI:** `written_artifact` (`crates/df-web/src/transcript/mod.rs`) turns a completed, untruncated tool call whose title contains `write_artifact` and whose raw input has a valid `name` into the artifact row (`artifacts::artifact_row`, attribute `data-artifact-written`). The row replaces the tool row. `scripts/artifactSplit.ts` watches the transcript for `data-artifact-written` and fires `artifacts-changed` (the Artifacts tab reloads, and the first artifact swaps in the tab bar out of band) and reloads an open artifact of that name.
- **OpenCode** reports MCP tools through its own `execute` tool: title `execute`, raw input `{"code": "..."}`, the inner calls only in `rawOutput.metadata.toolCalls[]`. The detection misses them.
- **Control-plane transcript events** exist for forge activity (ADR-0046). `df_db::transcript::append_control_event` stores an event under the run and task locks (`lock_run_and_task`), keyed by `(run_id, source_key)` with `ON CONFLICT DO NOTHING`, and publishes `transcript_event`. It refuses anything but `ForgeActivity`. `df-agent-channel` drops `ForgeActivity` sent by the runtime (`allowed_from_runtime`); `factory-agent`'s runner ignores the kind.
- **`write_artifact`:** `df-agent-channel/src/tools/artifacts.rs` validates and calls `df_db::artifacts::write_artifact`, which locks only the task row in its own transaction. The tool dispatcher stores every call in `tool_calls` by `call_id` and answers a repeated `call_id` with the stored result, so a handler runs once per call id.

## Goals / Non-Goals

**Goals:**
- One harness-independent source for "an artifact was written" in the transcript.
- An event family the next control-plane events (agent woken by another task, incoming webhook event) can join without another schema or storage change.

**Non-Goals:**
- The next variants themselves, and events outside a run (a run-less transcript row): backlog in `product/iterations/roadmap.md`, row to add when this change is archived.
- Parsing harness-specific tool output (OpenCode's `metadata.toolCalls`): rejected, see D1.
- Deduplicating the tool row and the artifact row: rejected (user decision; the tool row shows what happened).

## Decisions

### D1: The control plane records the write, the UI stops guessing

The control plane writes the event where the artifact is stored. The UI renders the event and no longer inspects tool calls for artifacts.

Alternative: extend `written_artifact` to read `rawOutput.metadata.toolCalls[]`. Rejected: it is an undocumented OpenCode format, every new harness needs another parser, and raw output can be truncated.

### D2: `FactoryActivity`, a family next to `ForgeActivity`

`darkfactory.transcript.v1` gets an additive `Event.kind` variant `FactoryActivity factory_activity = 20` with its own `oneof kind { ArtifactWritten artifact_written = 1; }`. `ArtifactWritten` carries `name`, `kind` (`text`/`image`), `mime_type` and `size` (bytes): what the row and a later reader need, never the content. Stored `transcript_events.kind` is `factory_activity`.

Later variants (agent woken by another task, webhook event received) are new fields of that oneof; storage, locking, live delivery, the runtime filter and the agent filter already cover them.

Alternatives:
- A top-level `Event.kind` variant per event (`ArtifactWritten = 20`, later 21, 22...). Rejected: every new event would touch the same five match arms across crates; the family keeps them in one place.
- Moving `ForgeActivity` under `FactoryActivity`. Rejected: changes the JSON of stored rows (`forgeActivity` → `factoryActivity.forge...`) and the forge spec, for no behavior gain. The two families share the storage path, not the schema.

### D3: One generalized `append_control_event`

`append_control_event` accepts `ForgeActivity` and `FactoryActivity` and derives the stored `kind` from the variant; anything else stays refused. A `factory_event(activity)` constructor mirrors `forge_event`. `allowed_from_runtime` in `df-agent-channel` refuses both kinds, and the runner of `factory-agent` ignores both.

### D4: Written in the artifact's transaction, keyed by the call id

`df_db::artifacts::write_artifact` takes the run id and the tool call id, locks with `lock_run_and_task` (run row first, then task row: the order ADR-0042 requires for transcript writers; today it locks only the task row), stores the artifact and appends the event with source key `artifact:<call_id>` before commit. A rejected write returns before the append, so it writes no event. The tool dispatcher passes `call_id` down to the handler.

Alternative keys: `artifact:<name>:<sha256>` would merge two writes of identical content in one run into one row although the agent wrote twice. Rejected.

### D5: Web UI renders the event; the tool row stays

`Folder::apply` maps `FactoryActivity::ArtifactWritten` to a new item kind rendered by `artifact_row` (label from the MIME type via `type_label`, not from the extension). `written_artifact` and its special case in `render_tool` are removed, so every `write_artifact` call renders as an ordinary tool row, followed by the artifact row from the event. `data-artifact-written` stays on the artifact row, so `artifactSplit.ts` and the out-of-band tab bar keep working unchanged.

## Risks / Trade-offs

- [Two rows per write with Claude Code] → accepted; the artifact row is compact and the tool row is collapsed by default.
- [`write_artifact` now holds the run row lock] → the transaction is short (two inserts); it follows the documented lock order, so it cannot deadlock with forge activity writers.
- [Transcripts written before this change have no artifact-written events] → their `write_artifact` calls show only as tool rows; the Artifacts tab still lists the artifacts. Acceptable pre-MVP, no backfill.

## Migration Plan

No database migration: `transcript_events.kind` is free text and `source_key` exists. Proto change is additive. Rollback: older readers fail to decode `factoryActivity` rows (generated decoder rejects unknown fields); acceptable pre-MVP.
