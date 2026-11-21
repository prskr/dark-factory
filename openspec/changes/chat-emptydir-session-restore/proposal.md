# Proposal

## Why

Every new chat run waits for a PVC: Longhorn takes seconds or more to create and attach one. A chat's PVC keeps almost nothing of value:

- `/data/repo` is reset to `origin/<base>` with `clean -fdx` on every run (`prepare_read_only`, `crates/factory-agent/src/workspace.rs`).
- The transcript is in PostgreSQL.

The only state lost without the PVC is the harness's own session files below `/data/home`, which `session/load` needs to resume the conversation. This change moves chats to an `emptyDir` and carries the session files across runs through the control plane.

## What Changes

- **`emptyDir` for chat runs**:
  - A chat run's pod mounts an `emptyDir` at `/data`, with a `sizeLimit` from the new option `sandbox.chatDataSizeLimit`. Tasks of kind `task` keep their PVC.
  - The scheduler doesn't create a PVC for a chat. Retention, "Reset workspace" and the `volume.*` lifecycle events no longer apply to chats.
  - **BREAKING**: existing chat PVCs (`task-<key>-chat`) are no longer used or deleted by the control plane. The upgrade notes give a `kubectl` command to delete them.
  - No new file-system write block. A chat stays read-only because the gateway rejects every ref update of a chat (`read_only` in `df-gateway-control/src/snapshot.rs`) and the workspace is reset. Landlock keeps `/data/repo` and `/data/home` writable.
- **Session state restore** (harness-agnostic):
  - A harness profile declares its session state directories below `HOME` as data: `.claude/projects` for Claude Code and `.local/share/opencode` for OpenCode. `Assign` carries the list. A generic ACP harness (F12) only needs this setting, no code.
  - At run end, after the harness has stopped, the runtime packs these directories into one gzip tar archive and sends it in `RunReport`.
  - The control plane stores one snapshot per chat in PostgreSQL as an opaque blob, together with the run and the harness session id, and replaces it at every run end. It never reads the content.
  - At the next run, `Assign.continuation` carries the snapshot. The runtime unpacks it into the empty home before the harness starts, then calls `session/load` with the recorded `harness_session_id`, as today.
  - The snapshot is limited to 8 MiB compressed (open question 2). A larger one is not sent; the next run uses the summary.
  - Strict unpacking: regular files and directories only, no symlinks or links, relative paths without `..` below a declared directory, no-follow writes, sizes checked before and during unpacking.
  - Without declared directories, without a usable snapshot, or when unpacking fails, the run starts a new session with the existing summary fallback (`crates/df-agent-channel/src/summary.rs`), unchanged.
- **Chat keep-alive**: new option `runs.chatKeepAliveSeconds` (recommended default 1800) replaces `runs.keepAliveSeconds` (default 600) for chat runs. A pod that stays alive between messages needs no provisioning and no restore. Idle chat runs still give up their slot under pressure.
- **Docs**: chats, architecture (agent runs, task volumes), OpenCode session resume, configuration reference (`sandbox.chatDataSizeLimit`, `runs.chatKeepAliveSeconds`), Helm README, lifecycle events, backup and upgrade notes.

Out of scope:
- **Memory files in `/data/home`** outside the declared directories are lost at the end of each chat run. Memory shared across runs and chats comes with agent memory (ADR-0040), iteration 6 in `product/iterations/roadmap.md`.
- **Better summary fallback**: a separate change (backlog row "Context compaction plugins", F3). This change doesn't touch `summary.rs`.
- **Session state for tasks of kind `task`**: rejected. Their PVC keeps the session.
- **Snapshots after every turn** (protects a pod lost in the keep-alive window): rejected for now, see design open question 5.
- **Sandbox pod resources** (CPU, memory, ephemeral-storage requests): existing backlog row "Sandbox pod resources".
- **Harness registry entries with `session_state`** for Generic ACP: iteration 4, "Harness registry and Generic ACP (F12)". This change only adds the profile field and the `Assign` field it will use.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `sandbox-provisioning`: tasks of kind `task` get a PVC, chats an `emptyDir` with a size limit.
- `task-management`: volume retention and Reset workspace apply to tasks of kind `task`; chat snapshots follow the same retention times.
- `acp-harness`: session state directories, packing at run end, restore at run start, strict archive format.
- `run-orchestration`: the control plane stores the chat snapshot and delivers it in `Assign`; the chat keep-alive window.
- `chats`: a chat keeps only its session state between runs.
- `opencode-harness`: OpenCode session state for chats comes from the snapshot.
- `workspace-lifecycle`: a chat run always prepares on an empty data volume.
- `lifecycle-events`: chats emit no volume events.

`sandbox-filesystem-isolation` is unchanged.

## Impact

- `contract/proto` (`agent.proto`): `AcpHarness.session_state_dirs`, `Continuation.session_state`, `RunReport.session_state`.
- `crates/df-core`: `HarnessProfile.session_state` for both profiles; `sandbox.chatDataSizeLimit`, `runs.chatKeepAliveSeconds`; a data volume choice on `RunSandboxSpec`.
- `crates/df-k8s`: `agent_volumes` renders an `emptyDir` for chats.
- `crates/df-orchestrator`: the scheduler skips `ensure_task_volume` for chats; retention deletes chat snapshots instead of PVCs.
- `crates/df-db`: migration for table `chat_session_states`; store, load, delete and retention queries; `due_for_retention` limited to `kind = 'task'`; `reset_workspace` rejects chats; `.sqlx/`.
- `crates/df-agent-channel`: `Assign` carries the dirs and the snapshot; `record_run_report` stores the snapshot; the chat keep-alive deadline.
- `crates/factory-agent`: pack at finalize, unpack before harness start (new module `session_state.rs`); uses `tar` and `flate2`, already in `Cargo.lock`.
- `deploy/helm`: `sandboxNamespace.chatDataSizeLimit`, `runs.chatKeepAliveSeconds`.
- `crates/fake-acp-agent`, `crates/df-e2e`: a fake session state directory and a chat that resumes after its pod was released.
- `product/iterations/roadmap.md`: one backlog row for this change.
