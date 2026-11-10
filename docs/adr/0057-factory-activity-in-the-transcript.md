# ADR-0057: Factory activity in the transcript

- **Status:** Accepted
- **Date:** 2026-11-10
- **Related:** [ADR-0044](0044-task-artifacts-and-cross-task-reading.md), [ADR-0046](0046-forge-activity-in-the-transcript.md), [ADR-0042](0042-live-views-resume-only-from-an-authoritative-sequence.md)

## Context

The transcript row of a written artifact and the live refresh of the Artifacts tab were derived in `df-web` from the harness's tool call (title `write_artifact`, `name` in the raw input). OpenCode wraps MCP tools in its own `execute` tool, so the detection missed them. The control plane is the one place that knows every stored artifact, whatever the harness.

## Decision

- **A family `FactoryActivity` next to `ForgeActivity`** in `darkfactory.transcript.v1` (`Event.kind` 20) for things the control plane did for a task that a board member should see. The first variant is `ArtifactWritten` (name, `ArtifactKind` enum text/image, MIME type, size; never the content). Later variants (an agent woken by another task, an incoming webhook event) are new fields of its oneof.
- **Written by the control plane only,** through `df_db::transcript::append_control_event` (same storage, locking, live delivery as forge activity). The runtime channel drops it, the runtime ignores it, the agent never receives it.
- **Written in the artifact's transaction, keyed per call:** source key `artifact:<call_id>`, run row locked before the task row (ADR-0042). A rejected write stores no event.
- **The web UI renders the event** as the artifact row and no longer inspects tool calls. The harness's tool row stays for every harness.

## Consequences

- Artifact rows and the live Artifacts tab work with every harness.
- With Claude Code a write shows the tool row and the artifact row.
- Transcripts from before this change have no artifact rows.
- Events outside a run need a run-less transcript row (as in ADR-0046); not covered.

## Alternatives considered

- **Parse OpenCode's `rawOutput.metadata.toolCalls[]`.** Rejected: undocumented, one parser per harness, raw output can be truncated.
- **One top-level `Event.kind` per event.** Rejected: every new event touches the same match arms in several crates.
- **Move `ForgeActivity` under `FactoryActivity`.** Rejected: changes stored JSON and the forge spec without behavior gain.
- **Key by name and content hash.** Rejected: two identical writes would merge into one row.
- **Hide or merge the tool row.** Rejected: it shows what happened, and for a failed write it is the only row.
