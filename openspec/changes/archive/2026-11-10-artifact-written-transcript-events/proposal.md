# Proposal

## Why

The transcript row of a written artifact and the live refresh of the Artifacts tab depend on how the harness reports the call. `df-web` recognises `write_artifact` only by the tool title and the `name` in its raw input. OpenCode wraps MCP tools in its own `execute` tool (`{"code":"await tools.factory.write_artifact({...})"}`), so with OpenCode no artifact row appears, the Artifacts tab does not reload, and an open artifact is not refreshed. The control plane is the one place that knows every successful write, whatever the harness.

## What Changes

- **New transcript event family `FactoryActivity`** in `darkfactory.transcript.v1` (additive), next to `ForgeActivity`: things the control plane did for a task that a board member should see in the transcript. The first variant is **`ArtifactWritten`** (name, type, MIME type, size). The family is meant to take later control-plane events, such as one agent waking another or an incoming webhook event.
- **The control plane writes `ArtifactWritten`** into the run's transcript when `write_artifact` stores an artifact, in the same transaction as the artifact row and once per tool call. A rejected write produces no event. The runtime cannot send factory activity, and it is never sent to the agent.
- **Artifact rows come from the event.** The transcript shows the artifact row ("findings.md · Markdown", opens the current version) for each `ArtifactWritten` event. The harness's own tool row stays visible above it, for every harness: it shows what happened, and for a failed write it is the only row.
- **BREAKING (UI):** with Claude Code, a successful write used to replace the tool row with the artifact row. Now both rows show. The detection by tool title in `df-web` is removed.
- The Artifacts tab, its first appearance and the reload of an open artifact follow the event, so they work live with every harness.

## Capabilities

### New Capabilities

- `factory-activity`: control-plane events in a task's transcript that are not forge activity. Who may write them, when, how often, and that the agent never receives them. It starts with artifact writes.

### Modified Capabilities

- `task-ui`: "Artifact rows in the transcript" and "Artifacts tab appears with the first artifact" are triggered by the artifact-written event instead of the `write_artifact` tool call; the tool row stays.
- `artifact-ui`: "Artifacts update live" is triggered by the artifact-written event.

## Out of scope

- Other `FactoryActivity` variants (an agent woken by another task, an incoming webhook event): backlog in `product/iterations/roadmap.md`, row to add when this change is archived.
- Factory activity outside a run (for example a webhook event while no run is active): needs a run-less transcript row, as ADR-0046 already notes for forge activity. Backlog, same row.
- Hiding or merging the harness's tool row with the artifact row: rejected.
- Moving `ForgeActivity` into `FactoryActivity`: rejected, see design.

## Impact

- `contract/proto/darkfactory/transcript/v1/transcript.proto`: new messages, new `Event.kind` variant.
- `df-db`: `append_control_event` accepts factory activity; `write_artifact` locks run and task and appends the event in its transaction.
- `df-agent-channel`: passes the tool call id to `write_artifact`; drops factory activity sent by the runtime; event kind name.
- `factory-agent`: ignores the new kind like forge activity.
- `df-web`: new transcript item for the event, removal of the tool-title detection.
- Docs: `docs/content/using/artifacts.md` (transcript and live sections); an ADR for the event family.
