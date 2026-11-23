# Design

## Context

- **Read-only today = chat**: a chat run gets a read-only checkout of the base branch. The decision is spread over the key format:
  - `GitRoute.read_only: key.is_chat()` (`df-gateway-control/src/snapshot.rs`), from `ActiveRun.task_key` (`df-db/src/egress.rs`);
  - `Workspace.read_only: rc.is_chat()` (`df-agent-channel/src/assign.rs`), with `RunContext::is_chat()` → `task.key.is_chat()` (`df-orchestrator/src/run_context.rs`);
  - chat-only behavior also uses `is_chat()`: tool subset (`tools.rs`), chat instructions (`assign.rs`), task kind on insert (`df-db/src/tasks.rs`), base-task check (`tasks/tree.rs`).
- **Pod volumes** (`agent_volumes`, `crates/df-k8s/src/render.rs`): one `task-data` volume at `/data` from PVC `task.key.volume_name()` (`task-<key lowercase>`, so `task-df-12-chat` for a chat). No subPaths: `/data/repo`, `/data/home` and `/data/runtime` are directories the runtime creates. The only `emptyDir` today is the memory volume at `/run/factory`. No container sets resource requests or limits.
- **Volume lifecycle**:
  - The scheduler calls `provider.ensure_task_volume` and `df_db::volumes::volume_ensured` for every run, chats included (`df-orchestrator/src/scheduler.rs`). Teardown deletes only the pod.
  - `df_orchestrator::Retention` scans hourly. `df_db::volumes::due_for_retention` treats `archived_at` like closing. Deletion emits `volume.destroyed` (`retention` or `reset`).
  - Columns `tasks.volume_provisioned_at` and `tasks.volume_deleted_at`. `POST /tasks/{key}/reset-workspace` calls `df_db::volumes::reset_workspace`.
- **Read-only workspace in the runtime**: `prepare_read_only` clones if needed, checks out `origin/<base>` detached, `reset --hard`, `clean -fdx`. `finalize_workspace` pushes nothing. The runtime only looks at `Assign.workspace.read_only`, never at the key.
- **Resume** (`setup_session`, `crates/factory-agent/src/harness.rs`): with a non-empty `continuation.harness_session_id` the runtime sends `session/load`. On error it emits `harness.session_resume_failed`, creates a new session and prepends the summary (`context_compacted` notice). The session id goes back in `RunReport.harness_session_id`. The control plane stores it on `runs` and `tasks`; `Assign.continuation` takes the newest earlier run with a non-empty id.
- **Session files**: Claude Code writes `~/.claude/projects/<cwd slug>/<session>.jsonl`. Its auto-memory is also below `~/.claude/projects/<cwd slug>/memory/` (to verify, task 1.1). OpenCode keeps `opencode.db` (SQLite) below `$XDG_DATA_HOME/opencode`, default `~/.local/share/opencode`. `HOME` is `/data/home`, the cwd is always `/data/repo`.
- **Harness profiles** are constants in `crates/df-core/src/harness.rs` (`CLAUDE_CODE`, `OPENCODE`). `AcpHarness.files` (proto) already carries files the runtime writes below `HOME`.
- **gRPC**: the server limits messages to 16 MiB in both directions (`MAX_MESSAGE_BYTES`, `df-agent-channel/src/server.rs`). Artifacts travel as `ToolCallRequest.attachment` (max 8 MiB) and are stored as `BYTEA`. There is no object storage in the project.
- **Keep-alive**: after a turn the run waits `runs.keepAliveSeconds` (default 600) for a message, in the same pod. Idle time counts toward `runs.maxWallClockSeconds` (default 7200). Under slot pressure the longest idle chat run is released (`release_idle_chat_run`).
- **Measured**: one Claude Code planning chat of about 30 turns had 1.0 MB of session files, 285 KB gzip.
- **Next user**: a follow-up change adds read-only review subtasks (`create_task` with `read_only: true` and a `base_task`; the gateway rejects their pushes; the reviewed branch is checked out fresh on every run). It must be able to reuse everything here by setting the read-only property.
- **Planned**: a separate change adds task retention and deletion.

## Goals / Non-Goals

**Goals:**
- One place decides whether a task's workspace is read-only; volume, gateway, `Assign` and session state follow it.
- No PVC for a read-only workspace: such a run starts without waiting for storage.
- A run with a read-only workspace resumes the harness session of its previous run, for any harness that declares its session directories.
- The snapshot is opaque to the control plane, keyed by task, deleted with its task, and safe to unpack in the sandbox.

**Non-Goals:**
- Read-only review subtasks and any change to `create_task`: the follow-up change.
- Task retention and deletion: the planned separate change; this change only cascades the snapshot.
- Memory across runs and tasks: agent memory (ADR-0040), iteration 6.
- Changing the summary fallback: separate change (backlog "Context compaction plugins", F3).
- `emptyDir` for writable workspaces: rejected; they need their uncommitted work and the WIP flow.
- Resource requests for sandbox pods: the proposed change `sandbox-scheduling-resources` (instance-wide `sandbox.resources`); values per run kind in the backlog row "Sandbox pod resources".
- A file-system write block for read-only workspaces: rejected; the gateway and the reset already keep them read-only.

## Decisions

### D1 — One read-only property

Migration: `tasks.read_only_workspace BOOLEAN NOT NULL DEFAULT false`, backfilled to `kind = 'chat'`, with `CHECK (read_only_workspace = (kind = 'chat'))` in this change. Chat creation sets it. The follow-up relaxes the check to `kind <> 'chat' OR read_only_workspace` and lets `create_task` set it.

Readers:
- `df_db::egress::ActiveRun` gets `read_only` from the column; `git_route` uses it for `GitRoute.read_only`. The key is still parsed for the WIP branch; a read-only run gets no WIP branch and no task branch (empty), as a chat has today.
- The task row in `RunContext` gets `read_only_workspace`; `RunContext::read_only_workspace()` replaces `is_chat()` where the meaning is "read-only": `Workspace.read_only`, the empty task and WIP branch in `Assign`, the data volume (D2), `session_state_dirs` (D3) and the session state (D6, D7).
- Chat-only behavior keeps `is_chat()`: tools, chat instructions, archiving, keep-alive (D9), the chat slots.

Alternative: a function of the kind in `df-core` (`TaskKind::read_only_workspace()`), no column. Rejected: the follow-up then needs the column anyway and the gateway still reads the key.

### D2 — Data volume from the read-only property

`RunSandboxSpec` gets `data_volume: DataVolume` with `Pvc { claim_name }` and `EmptyDir { size_limit }` and drops the unused `volume_name`. The scheduler builds `Pvc` for a writable workspace (as today) and `EmptyDir { sandbox.readOnlyDataSizeLimit }` for a read-only one, and skips `ensure_task_volume` and `volume_ensured` for the latter. `agent_volumes` renders the volume `task-data` from it; the mount stays `/data`. `df-k8s` doesn't know about kinds.

`sandbox.readOnlyDataSizeLimit` (Helm `sandboxNamespace.readOnlyDataSizeLimit`) is a Kubernetes quantity, default `10Gi`. Today no quantity option is validated (`pvcSize` isn't either) and `k8s-openapi`'s `Quantity` is a plain string. A small check in `df-core` implements the full Kubernetes quantity grammar (a signed decimal number, then a binary suffix `Ki`…`Ei`, a decimal suffix `m`, `k`, `M`…`E`, an exponent `e<n>`/`E<n>`, or no suffix) and additionally requires a value above zero, because a size limit of zero or below makes no sense. Anything else is rejected at startup with an error naming the option. Because it accepts every value Kubernetes accepts (apart from zero and negative sizes, which Kubernetes rejects for a PVC as well), the same check also applies to `pvcSize` without breaking a working configuration. The `emptyDir` uses node disk (no `medium: Memory`). When a pod exceeds the limit, the kubelet evicts it; the run ends as a lost sandbox, as today.

Alternative: a generic ephemeral volume (`ephemeral.volumeClaimTemplate`). Rejected: it still goes through the storage class and Longhorn.

### D3 — Session state directories in the profile

`HarnessProfile` gets `session_state: &'static [&'static str]`, paths relative to `HOME`:
- `CLAUDE_CODE`: `[".claude/projects"]`
- `OPENCODE`: `[".local/share/opencode"]`

`AcpHarness` gets `repeated string session_state_dirs` (field number: see D5). `build_assign` fills it from the profile for runs with a read-only workspace; other runs get an empty list (their PVC keeps the files). The runtime validates each entry on the raw string, split at `/`: no leading `/`, and no component that is empty, `.` or `..`. Otherwise it fails the run before harness start. (`write_harness_files` uses `Path::components()`, which silently drops `.` and empty components, so it can't be reused as is.) The F12 registry will store the same list as data per registered harness; no harness-specific code exists in the runtime. The runtime packs and restores whenever the list is non-empty; it doesn't check the read-only flag itself.

`~` is not used in the data: all paths are relative to `HOME`, as for `files`.

### D4 — Archive format

A zstd-compressed tar (ustar) archive (one zstd frame), built and read with the `tar` crate (already in `Cargo.lock`) and `ruzstd` (new, pure Rust). Both go into `[workspace.dependencies]`. The top-level entry names are the declared directories, so the archive is self-describing and easy to inspect (`zstd -dc | tar -t`). Decompression goes through `ruzstd`'s streaming decoder, so the limits below are counted while reading.

Why `ruzstd`: the runtime decompresses and parses an archive whose content the agent controls, in a process outside Landlock. A pure Rust decoder keeps that parsing memory-safe. A C dependency as such is no obstacle: `ring` already brings C and assembly into every static musl binary through `toolchains_musl` (ADR-0058), and `zstd-sys` would build the same way.

Compression level: `ruzstd` implements only its `Fastest` level today (about zstd level 1). That is enough. Measured on the planning session of this change (several runs over three days, many design rounds; `~/.claude/projects` as tar, 2026-11-23): 4.17 MB raw, 839 KB with gzip -6 (128 ms), 1.06 MB with gzip -1. zstd level 1 is typically about the size of gzip -6, so about 10–12 % of the 8 MiB limit. Higher levels typically save another 10–30 % on JSON text (estimate, not measured); a session would have to grow 8–10 times before that matters.

Switching the compressor later needs no migration: the level is not part of the format, and every zstd decoder reads frames of every level. If measurements (task 1.1) or later runs show sessions close to the limit (above about 25 % of it), compress with the `zstd` crate (C) at a higher level and keep `ruzstd` for decompression, so parsing of agent-controlled data stays memory-safe. That is a code change only. The window must stay within the restore check (at most 8 MiB): use level 19 or lower, or set `window_log(23)` or lower explicitly, and never long mode. zstd's defaults for inputs over 256 KB use windowLog 19 at level 1, 21–22 at levels 3–16, exactly 23 (8 MiB) at levels 17–19, and 25–27 at levels 20–22 and in long mode, which restore would always reject.

There is one format from the start and no format marker: a later codec change (not a level change) would need a content check to tell old snapshots from new ones.

Codecs considered (gzip measured as above; the others are typical values for JSON text, not measured):
- **zstd** (chosen): at low levels as small as gzip -6 and faster, at high levels about 10–30 % smaller. Encoder `ruzstd` (`Fastest`) now; the `zstd` crate later if needed (see above).
- **gzip** (`flate2`, already in the graph): rejected; slower than zstd at the same ratio, and switching later means a format check.
- **brotli** (already in the graph through `tower-http`): about as small as zstd at high quality, but much slower to compress at that quality.
- **snappy**: rejected; built for speed, its ratio is worse than gzip, which counts against the 8 MiB limit.

Packing (runtime, outside Landlock, after the harness has stopped). The agent may still have a background process running at this point (`stop_harness` kills only the harness process), so every path can change between two calls. All checks therefore act on open file descriptors, never on paths (`rustix`, already a workspace dependency):
- Before packing, kill the harness's process group. The harness is started as the leader of a new process group for this. A process that left the group itself can still race, so this is best effort and not the protection.
- Open `HOME` as a directory fd. Walk each declared directory with `openat` relative to its parent fd. Open every directory with `O_NOFOLLOW | O_DIRECTORY | O_CLOEXEC` and every file with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY | O_CLOEXEC`, so a symlink fails to open and a FIFO doesn't block. A missing declared directory is skipped.
- Apply the restore limits while packing, on the same quantity restore counts: the uncompressed tar stream (headers, padded data blocks and end-of-archive blocks), measured by a counting writer between the tar builder and the compressor, at most 64 MiB; and at most 10,000 entries. Restore's cap of 65 MiB leaves 1 MiB of slack above that. Read each file through `take(st_size)` from its `fstat`; a limit reached ends packing with an empty archive and a `harness.session_state_too_large` notice; a file that comes up short (it shrank during the read) ends packing with an empty archive and a `harness.session_state_skipped` notice that names the file. So packing never produces an archive that restore rejects, and a sparse huge file can't stall `finalize_run`.
- `fstat` the open fd and include it only if it is a directory, or a regular file with `nlink == 1`. Skip everything else (symlinks, hard-linked files, devices, FIFOs, sockets) with one `harness.session_state_skipped` notice per run that names the count.
- Store mode bits `0o600`/`0o700` only, no owner and no times.
- Stop and send an empty archive when the compressed size exceeds the limit (D6); emit `harness.session_state_too_large` with the size.

Unpacking (runtime, once, before the harness starts):
- Only when none of the declared directories exists below `HOME` (a fresh `emptyDir`).
- Reject the archive when its size exceeds the limit.
- Read the zstd frame header first and reject a frame whose window size is above 8 MiB (strictly greater; exactly 8 MiB, as levels 17–19 produce, is accepted), so the decoder's buffer stays bounded.
- Count the whole decompressed stream, not only file contents: wrap the decoder output in `Read::take(64 MiB + 1 MiB)` before the tar reader, so PAX and GNU long-name data, which the `tar` crate reads into memory, count too. Reaching the cap fails the restore. At most 10,000 entries.
- Per entry: type must be regular file or directory; path must be relative, without `..`, `.` or empty components, and must start with a declared directory. Anything else fails the whole restore.
- Create directories and files below `HOME` with `openat` from the `HOME` fd: every component opened with `O_NOFOLLOW | O_DIRECTORY` (created first if missing), files with `O_CREAT | O_EXCL | O_NOFOLLOW`.
- On failure, delete what was written by fd as well (`unlinkat` from the `HOME` fd, without following symlinks), emit `harness.session_restore_failed` with the reason, and start a new session with the summary (no `session/load`).

The runtime drops a repeated `Assign` for the same run (`CommandFilter`, `connection.rs`), so a reconnect never restores again. The control plane still sends the snapshot in every `Assign` of the run, also on a reconnect; at most 8 MiB on a rare event, accepted.

Alternative: a protobuf list of `{path, content}`. Simpler to validate (no entry types), but needs its own compression and tooling to inspect. Rejected; tar it is.

### D5 — Transport

New fields of existing messages carry placeholder numbers (`= <n1>`, `= <n2>`, `= <n3>`). The final numbers are assigned when the change is implemented: the next free numbers of each message at that time, checked against its `reserved` entries, so parallel changes don't claim the same numbers. The field of the new message `SessionState` is numbered.

- **Run end**: `RunReport` gets `SessionState session_state = <n1>`, a message `SessionState { bytes archive = 1; }`. Presence carries the meaning:
  - field unset: the session wasn't touched, because no turn started in this run (Landlock check, prepare, spawn or cancel before `Assign` end through `end_run_early` or `collected.fail`; a session driver error before the first turn reaches `finalize_run` but still has no turn), or the list of directories is empty. The runtime packs only after at least one `TurnStarted`;
  - field set, `archive` empty: packing was tried and produced nothing usable (too large, nothing found, failed);
  - field set, `archive` non-empty: the new snapshot.

  The runtime fills it in `finalize_run` after the harness has stopped, also for cancelled and failed runs that reach finalize. The report already goes through the outbox in `/data/runtime` and is resent until acknowledged; the pod lives until then, so the `emptyDir` lasts long enough.
- **Run start**: `Continuation` gets `bytes session_state = <n2>`. Plain `bytes`: an empty archive is never valid, so empty means "none".
- **Harness**: `AcpHarness` gets `repeated string session_state_dirs = <n3>`.

Message limits: the server allows 16 MiB both ways (`MAX_MESSAGE_BYTES`). The runtime's client (`connect_channel`, `factory-agent/src/connection.rs`) sets no limit, so tonic's default of 4 MiB for received messages applies; an `Assign` with a 4–8 MiB snapshot would fail on every connect. The runtime client therefore sets `max_decoding_message_size(16 MiB)` (the test client already sets 32 MiB). A 285 KB snapshot transfers in well under a second.

### D6 — Storage in PostgreSQL, keyed by task

Table `task_session_states` (module `df_db::task_session_states`):

| Column | Type | Note |
|---|---|---|
| `task_id` | `UUID` PK, FK `tasks` `ON DELETE CASCADE` | one row per task |
| `run_id` | `UUID NOT NULL`, FK `runs` `ON DELETE CASCADE` | the run that reported it |
| `harness_session_id` | `TEXT NOT NULL` | the id the run reported |
| `data` | `BYTEA NOT NULL` | opaque archive |
| `size_bytes` | `INTEGER NOT NULL` | |
| `updated_at` | `TIMESTAMPTZ NOT NULL` | |

- Keyed by task id only; nothing reads the key or its `-CHAT` suffix. The foreign key cascades, so the row can't outlive its task. Deleting snapshots together with tasks and chats is the subject of the planned task retention and deletion change; this change deletes no snapshot except through `record_run_report`.
- `record_run_report` of a run with a read-only workspace (D5 for the field):
  - non-empty archive within the limit: upsert the row;
  - empty archive, or one above the limit: delete the row; above the limit also log a warning;
  - field unset: keep the row, so a transient early failure (forge down during prepare) doesn't lose the session.

  Reports of other runs never touch the table.
- The control plane never decodes `data`.
- Limit: 8 MiB compressed (`SESSION_STATE_MAX_BYTES` in `df-core`, a constant, not a setting).

PostgreSQL over object storage: the project has no object storage, artifacts already live in `BYTEA`, and one row per task of a few hundred KB is small. Backups then contain the snapshots too. The existing content-addressed `blobs` table is not used: snapshots are overwritten every run and never shared, so content addressing only adds garbage collection.

### D7 — Which snapshot a run gets

`Assign.continuation` of a run with a read-only workspace carries the stored snapshot unless a run of the task newer than the row's `run_id` has a persisted `turn_started` transcript event (`transcript_events.kind = 'turn_started'`, `KIND_TURN_STARTED` in `df-db/src/transcript.rs`). Then `harness_session_id` is the snapshot's id.

Control plane and runtime use the same signal: the runtime sets `RunReport.session_state` only after a turn started (D5), and the report follows that `TurnStarted` in the outbox sequence, so a received report with the field set implies a persisted `turn_started`. `runs.state` can't serve: it holds only the current state, and a run lost after its first turn ends `failed`. Runs without a turn (early failure, session driver error before the first turn) don't count and keep the row (D6).

A run lost after its first turn whose `TurnStarted` never reached the control plane doesn't count either: the older snapshot is delivered. That matches what the control plane knows; the summary lacks that turn too, and its prompt was never confirmed and is sent again.

Otherwise (a later run was lost after its first turn, a persisted `turn_started`, so the snapshot misses its turns, or there is no snapshot) the run gets no snapshot **and an empty `harness_session_id`**. Today `continuation()` (`df-agent-channel/src/assign.rs`) falls back to the newest earlier id or `tasks.harness_session_id`; for read-only workspaces that fallback is removed. With an empty id the runtime doesn't call `session/load`; it starts a new session with the summary and writes the `context_compacted` notice. A run therefore never depends on an adapter failing `session/load` for a session it doesn't have, and never silently resumes nothing or a stale session.

### D8 — Retention and reset skip read-only workspaces

- `due_for_retention` keeps its rules and adds `NOT t.read_only_workspace`: the scan never touches a task with a read-only workspace, and doesn't delete session states. Their deletion comes with the planned task retention and deletion change.
- The "Reset workspace" action is not shown for a task with a read-only workspace. The chat page has no such action today; the task page hides it by the property. The endpoint gets no extra check: chat keys never reach it (`Section::serves` in `task_pages.rs` rejects them). The follow-up's read-only review subtasks are served by the task pages, so that change adds a 409 for read-only workspaces to the endpoint.
- `volume.provisioned` and `volume.destroyed` are never emitted for read-only workspaces.

### D9 — Chat keep-alive

`runs.chatKeepAliveSeconds` (optional, default 1800, `0` releases right after the turn) sets the idle deadline of chat runs; `runs.keepAliveSeconds` stays for all other tasks, read-only or not. A conversation waits for a human; a review subtask doesn't. The session computes `idle_at` from the run's kind. The window stays bounded by `runs.maxWallClockSeconds` (default 7200), which includes idle time. Slot pressure still releases the longest idle chat run, so a longer window costs node resources, not chat slots.

### D10 — Existing chat PVCs

No automatic cleanup. For tasks with a read-only workspace the migration sets `volume_deleted_at` and clears `volume_provisioned_at`, as `delete_volume` does, so the columns read as "no volume". The upgrade notes tell operators to list the instance's PVCs by label (`app.kubernetes.io/managed-by=dark-factory`, `dark-factory.io/instance`, with the `dark-factory.io/task-key` column), check the rows whose task key ends in `-CHAT`, and delete exactly those. Selecting by name suffix alone could hit an unrelated PVC in a shared namespace. Pre-MVP, no installation needs a migration path.

## Risks / Trade-offs

- **Eviction**: `emptyDir` on node disk counts toward the node's ephemeral storage. A pod above `sizeLimit` or on a node under disk pressure is evicted; the task loses the session of that run and the next run uses the summary. Mitigation: the size limit, and an `ephemeral-storage` request through `sandbox.resources` (proposed change `sandbox-scheduling-resources`, which passes any resource through and sets no defaults). This change sets no default either; the operations docs explain how to size the request and why a limit must stay above `sandbox.readOnlyDataSizeLimit` plus the container's own use (task 9.2).
- **Clone on every new pod**: a run with a read-only workspace that is not in its keep-alive window clones afresh (blobless). For large repositories this can take longer than a PVC attach. Mitigation: the longer chat keep-alive (D9). Measure with a large repository before archiving.
- **Lost pod in the keep-alive window**: the session files of that run are lost; the next run uses the summary. Accepted: snapshots are taken only at run end.
- **Harness format changes**: a harness update can make an old snapshot unreadable. `session/load` then fails and the summary takes over. No version field is needed.
- **Database size**: at most 8 MiB per task, typically a few hundred KB. Until the task retention and deletion change, snapshots of archived chats stay in the database.
- **Untrusted content**: the agent controls the files it packs. The control plane never reads them; the runtime unpacks them only into the same task's fresh home with the checks of D4. The runtime runs outside Landlock, so packing is a way around it if it can be tricked into opening a path outside the declared directories (for example `/data/runtime` or `/run/factory/tls`): a process the agent left running can swap a directory for a symlink between two path-based calls. Packing therefore works only on file descriptors opened with `O_NOFOLLOW` relative to their parent fd and checks type and link count with `fstat` (D4).
- **Two sources of truth during this change**: the key suffix and the column. The `CHECK` constraint keeps them equal until the follow-up relaxes it.

## Migration Plan

1. Deploy the version: the migration adds and backfills `read_only_workspace` and creates `task_session_states`. New chat runs use `emptyDir`. The first run of an existing chat has no snapshot and starts with the summary (one `context_compacted` notice).
2. Delete the old chat PVCs with the command from the upgrade notes.

Rollback: an older version recreates chat PVCs on the next run; chats then start once with the summary.

## Resolved Questions

Decided by Peter Kurfer on 2026-11-22:

1. **Storage**: PostgreSQL (D6).
2. **Size limit**: 8 MiB compressed. The suggested 20 MB is above the 16 MiB gRPC message limit.
3. **`sandbox.readOnlyDataSizeLimit` default**: `10Gi`.
4. **Chat keep-alive**: separate `runs.chatKeepAliveSeconds`, default 1800 (D9).
5. **Snapshot timing**: only at run end.
6. **Snapshot retention**: not in this change. Deleting snapshots with their tasks and chats is the subject of the planned task retention and deletion change (D6, D8).
7. **Old chat PVCs**: deleted by hand with `kubectl` (D10).
8. **Reset for read-only workspaces**: the action is not shown (D8).
9. **Read-only property**: a task column with a `CHECK` tied to the kind; the follow-up relaxes the check (D1).
10. **Protobuf field numbers**: placeholders in the proposal; the final numbers are set at implementation (D5).
11. **Archive compression** (2026-11-23): zstd from the start, so no later format migration is needed. `ruzstd` at `Fastest` for memory-safe decoding; the compressor may move to the `zstd` crate later without migration if measured sessions get close to the limit (D4).

Review DF-21 round 2 (2026-11-23): one signal for "the session was touched" on both sides, a persisted `turn_started` event (D5, D7); restore limits applied while packing, reads bounded by `st_size` (D4); cleanup by fd, `O_NOCTTY | O_CLOEXEC`.

Review DF-21 (Grace Hopper, 2026-11-23) found no open product question. Its findings are applied: runtime client receive limit (D5), fd-based packing (D4), no session id without a snapshot (D7), snapshot kept on early failure (D5, D6), run provisioning and volume event scoped (spec deltas), whole-stream decompression limit and window check (D4), quantity check (D2), restore once (D4), directory check on the raw string (D3), docs and migration details (tasks).

## Open Questions

None.
