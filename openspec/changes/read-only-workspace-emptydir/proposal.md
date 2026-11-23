# Proposal

## Why

Every new chat run waits for a PVC: Longhorn takes seconds or more to create and attach one. A task with a read-only workspace keeps almost nothing of value on its PVC:

- `/data/repo` is reset to `origin/<base>` with `clean -fdx` on every run (`prepare_read_only`, `crates/factory-agent/src/workspace.rs`).
- The transcript is in PostgreSQL.

The only state lost without the PVC is the harness's own session files below `/data/home`, which `session/load` needs to resume the conversation. This change moves every task with a read-only workspace to an `emptyDir` and carries the session files across runs through the control plane.

Today only chats have a read-only workspace. The next user is a follow-up change that adds read-only review subtasks (`create_task` with `read_only: true` and a `base_task`); they reuse this mechanism unchanged.

## What Changes

- **One read-only property**: each task gets `read_only_workspace`. In this change it is true exactly for chats (set at creation, backfilled by the migration). The gateway's Git route, `Assign.workspace.read_only`, the volume choice and the session state all read this property instead of the chat key (`key.is_chat()` in `df-gateway-control/src/snapshot.rs`, `rc.is_chat()` for `read_only` in `df-agent-channel/src/assign.rs`). Chat-only behavior (tools, instructions, archiving, keep-alive) stays on the task kind.
- **`emptyDir` for read-only workspaces**:
  - The pod of a run with a read-only workspace mounts an `emptyDir` at `/data`, with a `sizeLimit` from the new option `sandbox.readOnlyDataSizeLimit`. Other tasks keep their PVC.
  - The scheduler doesn't create a PVC for such a task. Volume retention skips it, the "Reset workspace" action is not shown for it, and no `volume.*` lifecycle events are emitted for it.
  - **BREAKING**: existing chat PVCs (`task-<key>-chat`) are no longer used or deleted by the control plane. The upgrade notes give a `kubectl` command to delete them.
  - No new file-system write block. A read-only workspace stays read-only because the gateway rejects every ref update of the run and the workspace is reset. Landlock keeps `/data/repo` and `/data/home` writable.
- **Session state restore** (harness-agnostic):
  - A harness profile declares its session state directories below `HOME` as data: `.claude/projects` for Claude Code and `.local/share/opencode` for OpenCode. `Assign` carries the list for runs with a read-only workspace. A generic ACP harness (F12) only needs this setting, no code.
  - At run end, after the harness has stopped, the runtime packs these directories into one zstd-compressed tar archive and sends it in `RunReport`.
  - The control plane stores one snapshot per task in PostgreSQL (`task_session_states`, keyed by task id), as an opaque blob together with the run and the harness session id, and replaces it at every run end. It never reads the content. The row is deleted with its task.
  - At the next run, `Assign.continuation` carries the snapshot. The runtime unpacks it into the empty home before the harness starts, then calls `session/load` with the recorded `harness_session_id`, as today.
  - The snapshot is limited to 8 MiB compressed. For a larger one the run reports an empty archive, and the next run uses the summary.
  - Strict unpacking: regular files and directories only, no symlinks or links, relative paths without `..` below a declared directory, no-follow writes, sizes checked before and during unpacking.
  - Without declared directories, without a usable snapshot, or when unpacking fails, the run starts a new session with the existing summary fallback (`crates/df-agent-channel/src/summary.rs`), unchanged.
- **Chat keep-alive**: new option `runs.chatKeepAliveSeconds` (default 1800) replaces `runs.keepAliveSeconds` (default 600) for chat runs. It follows the kind, not the read-only property. A pod that stays alive between messages needs no provisioning and no restore. Idle chat runs still give up their slot under pressure.
- **Docs**: chats, architecture (agent runs, task volumes), OpenCode session resume, configuration reference (`sandbox.readOnlyDataSizeLimit`, `runs.chatKeepAliveSeconds`), Helm README, lifecycle events, backup, security and upgrade notes.

Out of scope:
- **Read-only review subtasks** (`create_task` with `read_only: true` and `base_task`, review checkout of the reviewed branch): the follow-up change that reuses this mechanism. Roadmap: the backlog row added by this change names it as next user. This change doesn't touch `create_task`.
- **Deleting snapshots** with their tasks and chats (retention): the planned task retention and deletion change; the roadmap row added by this change names it. This change deletes no snapshot by retention; the foreign key only cascades.
- **Memory files in `/data/home`** outside the declared directories are lost at the end of each run. Memory shared across runs and tasks comes with agent memory (ADR-0040), iteration 6 in `product/iterations/roadmap.md`.
- **Better summary fallback**: a separate change (backlog row "Context compaction plugins", F3). This change doesn't touch `summary.rs`.
- **Session state for tasks with a writable workspace**: rejected. Their PVC keeps the session.
- **Snapshots after every turn** (protects a pod lost in the keep-alive window): rejected; snapshots are taken only at run end.
- **Sandbox pod resources** (CPU, memory, ephemeral storage): instance-wide `sandbox.resources` in the proposed change `sandbox-scheduling-resources`; values per run kind in the backlog row "Sandbox pod resources".
- **Harness registry entries with `session_state`** for Generic ACP: iteration 4, "Harness registry and Generic ACP (F12)". This change only adds the profile field and the `Assign` field it will use.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `task-management`: the read-only workspace property; volume retention skips read-only workspaces and the Reset workspace action is not shown for them.
- `sandbox-provisioning`: a PVC for a writable workspace, an `emptyDir` with a size limit for a read-only one.
- `acp-harness`: session state directories, packing at run end, restore at run start, strict archive format.
- `run-orchestration`: the control plane stores the session state per task and delivers it in `Assign`; `Assign.workspace.read_only` from the property; the chat keep-alive window.
- `git-proxy`: the push rejection follows the read-only property.
- `chats`: a chat has a read-only workspace and keeps only its session state between runs.
- `opencode-harness`: OpenCode session state of a read-only workspace comes from the snapshot.
- `workspace-lifecycle`: a read-only workspace always starts on an empty data volume.
- `lifecycle-events`: no volume events for tasks with a read-only workspace.

`sandbox-filesystem-isolation` is unchanged.

## Impact

- `contract/proto` (`agent.proto`): `AcpHarness.session_state_dirs`, `Continuation.session_state`, `RunReport.session_state`. Field numbers are set at implementation (next free number then).
- `crates/df-core`: `HarnessProfile.session_state` for both profiles; `sandbox.readOnlyDataSizeLimit`, `runs.chatKeepAliveSeconds`; a data volume choice on `RunSandboxSpec`; `read_only_workspace` on the task.
- `crates/df-db`: migration (`tasks.read_only_workspace`, table `task_session_states`); task creation sets the property; `ActiveRun.read_only`; store, load and delete queries; `due_for_retention` limited to writable workspaces; `.sqlx/`.
- `crates/df-web`: the task page hides "Reset workspace" for read-only workspaces.
- `crates/df-gateway-control`: `GitRoute.read_only` from `ActiveRun.read_only`.
- `crates/df-k8s`: `agent_volumes` renders an `emptyDir` from the data volume choice.
- `crates/df-orchestrator`: `RunContext::read_only_workspace()`; the scheduler skips `ensure_task_volume` for read-only workspaces.
- `crates/df-agent-channel`: `Assign` carries `read_only`, the dirs and the snapshot; `record_run_report` stores the snapshot; the chat keep-alive deadline.
- `crates/factory-agent`: pack at finalize, unpack before harness start (new module `session_state.rs`); uses `tar` (already in `Cargo.lock`) and `ruzstd` (new, pure Rust: memory-safe decoding of agent-controlled data).
- `deploy/helm`: `sandboxNamespace.readOnlyDataSizeLimit`, `runs.chatKeepAliveSeconds`.
- `crates/fake-acp-agent`, `crates/df-e2e`: a fake session state directory and a chat that resumes after its pod was released.
- `product/iterations/roadmap.md`: one backlog row for this change.
