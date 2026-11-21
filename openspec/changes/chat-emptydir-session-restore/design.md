# Design

## Context

- **Pod volumes** (`agent_volumes`, `crates/df-k8s/src/render.rs`): one `task-data` volume at `/data` from PVC `task.key.volume_name()` (`task-<key lowercase>`, so `task-df-12-chat` for a chat). No subPaths: `/data/repo`, `/data/home` and `/data/runtime` are directories the runtime creates. The only `emptyDir` today is the memory volume at `/run/factory`. No container sets resource requests or limits.
- **Volume lifecycle**:
  - The scheduler calls `provider.ensure_task_volume` and `df_db::volumes::volume_ensured` for every run, chats included (`df-orchestrator/src/scheduler.rs`). Teardown deletes only the pod.
  - `df_orchestrator::Retention` scans hourly. `df_db::volumes::due_for_retention` treats `archived_at` like closing. Deletion emits `volume.destroyed` (`retention` or `reset`).
  - Columns `tasks.volume_provisioned_at` and `tasks.volume_deleted_at`. `POST /tasks/{key}/reset-workspace` calls `df_db::volumes::reset_workspace`.
- **Chat workspace**: `prepare_read_only` clones if needed, checks out `origin/<base>` detached, `reset --hard`, `clean -fdx`. `finalize_workspace` pushes nothing. The gateway rejects every ref update of a chat (`read_only: key.is_chat()`).
- **Resume** (`setup_session`, `crates/factory-agent/src/harness.rs`): with a non-empty `continuation.harness_session_id` the runtime sends `session/load`. On error it emits `harness.session_resume_failed`, creates a new session and prepends the summary (`context_compacted` notice). The session id goes back in `RunReport.harness_session_id`. The control plane stores it on `runs` and `tasks`; `Assign.continuation` takes the newest earlier run with a non-empty id.
- **Session files**: Claude Code writes `~/.claude/projects/<cwd slug>/<session>.jsonl`. Its auto-memory is also below `~/.claude/projects/<cwd slug>/memory/` (to verify, task 1.1). OpenCode keeps `opencode.db` (SQLite) below `$XDG_DATA_HOME/opencode`, default `~/.local/share/opencode`. `HOME` is `/data/home`, the cwd is always `/data/repo`.
- **Harness profiles** are constants in `crates/df-core/src/harness.rs` (`CLAUDE_CODE`, `OPENCODE`). `AcpHarness.files` (proto) already carries files the runtime writes below `HOME`.
- **gRPC**: the server limits messages to 16 MiB in both directions (`MAX_MESSAGE_BYTES`, `df-agent-channel/src/server.rs`). Artifacts travel as `ToolCallRequest.attachment` (max 8 MiB) and are stored as `BYTEA`. There is no object storage in the project.
- **Keep-alive**: after a turn the run waits `runs.keepAliveSeconds` (default 600) for a message, in the same pod. Idle time counts toward `runs.maxWallClockSeconds` (default 7200). Under slot pressure the longest idle chat run is released (`release_idle_chat_run`).
- **Measured**: one Claude Code planning chat of about 30 turns had 1.0 MB of session files, 285 KB gzip.

## Goals / Non-Goals

**Goals:**
- No PVC for chat runs: a new chat starts without waiting for storage.
- A chat run resumes the harness session of its previous run, for any harness that declares its session directories.
- The snapshot is opaque to the control plane and safe to unpack in the sandbox.

**Non-Goals:**
- Memory across runs and chats: agent memory (ADR-0040), iteration 6.
- Changing the summary fallback: separate change (backlog "Context compaction plugins", F3).
- `emptyDir` for tasks of kind `task`: rejected; they need their uncommitted work and the WIP flow.
- Resource requests for sandbox pods: backlog "Sandbox pod resources".
- A write block for chats: rejected; the gateway and the reset already keep chats read-only.

## Decisions

### D1 — Data volume per task kind

`RunSandboxSpec` gets `data_volume: DataVolume` with `Pvc { claim_name }` and `EmptyDir { size_limit }`. The scheduler builds `Pvc` for kind `task` (as today) and `EmptyDir { sandbox.chatDataSizeLimit }` for chats, and skips `ensure_task_volume` and `volume_ensured` for chats. `agent_volumes` renders the volume `task-data` from it; the mount stays `/data`.

`sandbox.chatDataSizeLimit` (Helm `sandboxNamespace.chatDataSizeLimit`) is a Kubernetes quantity, recommended default `10Gi` (open question 3). It is checked at startup like `pvcSize`. The `emptyDir` uses node disk (no `medium: Memory`). When a pod exceeds the limit, the kubelet evicts it; the run ends as a lost sandbox, as today.

Alternative: a generic ephemeral volume (`ephemeral.volumeClaimTemplate`). Rejected: it still goes through the storage class and Longhorn.

### D2 — Session state directories in the profile

`HarnessProfile` gets `session_state: &'static [&'static str]`, paths relative to `HOME`:
- `CLAUDE_CODE`: `[".claude/projects"]`
- `OPENCODE`: `[".local/share/opencode"]`

`AcpHarness` gets `repeated string session_state_dirs = 6`. `build_assign` fills it from the profile for chat runs only; task runs get an empty list (their PVC keeps the files). The runtime validates each entry like `HarnessFile.path` (relative, no `..`, no empty component) and fails the run before harness start otherwise. The F12 registry will store the same list as data per registered harness; no harness-specific code exists in the runtime.

`~` is not used in the data: all paths are relative to `HOME`, as for `files`.

### D3 — Archive format

A gzip-compressed tar (ustar) archive, built and read with the `tar` and `flate2` crates (both already in `Cargo.lock`, added to `[workspace.dependencies]`). The top-level entry names are the declared directories, so the archive is self-describing and easy to inspect.

Packing (runtime, outside Landlock, after the harness has stopped):
- Walk each declared directory without following symlinks (`symlink_metadata`). A missing directory is skipped.
- Include directories and regular files with one link only. Skip symlinks, hard-linked files, devices, FIFOs and sockets, with one `harness.session_state_skipped` notice per run that names the count.
- Open each file with `O_NOFOLLOW`. Store mode bits `0o600`/`0o700` only, no owner and no times.
- Stop and send no snapshot when the compressed size exceeds the limit (D5); emit `harness.session_state_too_large` with the size.

Unpacking (runtime, before the harness starts):
- Only when none of the declared directories exists below `HOME` (a fresh `emptyDir`). On a reconnect within the same pod the files are already there and the snapshot is ignored.
- Reject the archive when its size exceeds the limit.
- Per entry: type must be regular file or directory; path must be relative, without `..`, `.` or empty components, and must start with a declared directory. Anything else fails the whole restore.
- Create directories and files below `HOME` with `openat`-style no-follow writes (`rustix`, already a workspace dependency): every component is opened with `O_NOFOLLOW | O_DIRECTORY`, files with `O_CREAT | O_EXCL | O_NOFOLLOW`.
- Count decompressed bytes and entries while writing: at most 64 MiB and 10,000 entries. Exceeding either fails the restore.
- On failure, delete the declared directories that were written, emit `harness.session_restore_failed` with the reason, and start a new session with the summary (no `session/load`).

Alternative: a protobuf list of `{path, content}`. Simpler to validate (no entry types), but needs its own compression and tooling to inspect. Tar is rejected only if review prefers fewer parsing paths.

### D4 — Transport

- **Run end**: `RunReport` gets `optional bytes session_state = 6` (next free number). The runtime writes it in `finalize_run` after the harness has stopped, also for cancelled and failed runs where finalize runs. The report already goes through the outbox in `/data/runtime` and is resent until acknowledged; the pod lives until then, so the `emptyDir` lasts long enough.
- **Run start**: `Continuation` gets `bytes session_state = 3`.

With 8 MiB as limit, both messages stay below the 16 MiB server limit. The runtime's tonic client has no lower limit. A 285 KB snapshot transfers in well under a second.

### D5 — Storage in PostgreSQL

Table `chat_session_states`:

| Column | Type | Note |
|---|---|---|
| `task_id` | `UUID` PK, FK `tasks` `ON DELETE CASCADE` | one row per chat |
| `run_id` | `UUID NOT NULL`, FK `runs` | the run that reported it |
| `harness_session_id` | `TEXT NOT NULL` | the id the run reported |
| `data` | `BYTEA NOT NULL` | opaque archive |
| `size_bytes` | `INTEGER NOT NULL` | |
| `updated_at` | `TIMESTAMPTZ NOT NULL` | |

- `record_run_report` of a chat run upserts the row when the report has a snapshot and deletes it when the report has none (the harness declared nothing, the archive was too large, or packing failed). The control plane rejects a snapshot above the limit as if none was sent and logs a warning.
- The control plane never decodes `data`.
- Limit: 8 MiB compressed (`SESSION_STATE_MAX_BYTES` in `df-core`, a constant, not a setting).

PostgreSQL over object storage: the project has no object storage, artifacts already live in `BYTEA`, and one row per chat of a few hundred KB is small. Backups then contain the snapshots too. The existing content-addressed `blobs` table is not used: snapshots are overwritten every run and never shared, so content addressing only adds garbage collection.

### D6 — Which snapshot a run gets

`Assign.continuation` of a chat run carries the stored snapshot only when its `run_id` is the newest earlier run of the chat whose sandbox connected. Then `harness_session_id` is the snapshot's id. Otherwise (a later run was lost without a report, so the snapshot misses its turns) the run gets no snapshot, `session/load` fails on the empty home, and the summary covers every run. This avoids a silent resume of a stale session.

Mid-run reconnects in the same pod get the snapshot again; the runtime ignores it (D3).

### D7 — Retention and reset for chats

- `due_for_retention` and the PVC deletion apply only to `kind = 'task'`.
- The retention scan deletes the `chat_session_states` row of a chat with the same rules a chat PVC had: archived for `volumes.retainAfterCloseSeconds`, or idle for `volumes.idleTtlSeconds`, and no non-terminal run. No lifecycle event. A later run then starts with the summary.
- `reset_workspace` rejects a chat with a conflict ("chats have no workspace volume"). The chat page has no such action today.
- `volume.provisioned` and `volume.destroyed` are never emitted for chats.

### D8 — Chat keep-alive

`runs.chatKeepAliveSeconds` (optional, recommended default 1800, `0` releases right after the turn) sets the idle deadline of chat runs; `runs.keepAliveSeconds` stays for tasks. The session computes `idle_at` from the run's kind. The window stays bounded by `runs.maxWallClockSeconds` (default 7200), which includes idle time. Slot pressure still releases the longest idle chat run, so a longer window costs node resources, not chat slots.

### D9 — Existing chat PVCs

No automatic cleanup. A migration sets `volume_deleted_at` for chats so the columns read as "no volume". The upgrade notes give a command that deletes PVCs whose name ends in `-chat` in the sandbox namespace. Pre-MVP, no installation needs a migration path.

## Risks / Trade-offs

- **Eviction**: `emptyDir` on node disk counts toward the node's ephemeral storage. A pod above `sizeLimit` or on a node under disk pressure is evicted; the chat loses the session of that run and the next run uses the summary. Mitigation: size limit, and the backlog item for resource requests.
- **Clone on every new pod**: a chat run that is not in its keep-alive window clones afresh (blobless). For large repositories this can take longer than a PVC attach. Mitigation: the longer chat keep-alive (D8). Measure with a large repository before archiving.
- **Lost pod in the keep-alive window**: the session files of that run are lost; the next run uses the summary. Accepted (open question 5).
- **Harness format changes**: a harness update can make an old snapshot unreadable. `session/load` then fails and the summary takes over. No version field is needed.
- **Database size**: at most 8 MiB per chat, typically a few hundred KB; retention removes snapshots of archived and idle chats.
- **Untrusted content**: the agent controls the files it packs. The control plane never reads them; the runtime unpacks them only into the same chat's fresh home with the checks of D3. Packing never follows links, so the archive can't pull in files outside the declared directories (for example `/data/runtime` or `/run/factory/tls`).

## Migration Plan

1. Deploy the version: new chat runs use `emptyDir`. The first run of an existing chat has no snapshot and starts with the summary (one `context_compacted` notice).
2. Delete the old chat PVCs with the command from the upgrade notes.

Rollback: an older version recreates chat PVCs on the next run; chats then start once with the summary.

## Open Questions

1. **Storage**: PostgreSQL table (recommended, D5) or object storage. Object storage would add a new dependency and operator setup for a few hundred KB per chat.
2. **Size limit**: 8 MiB compressed (recommended). The suggested 20 MB is above the 16 MiB gRPC message limit; it would need a higher limit or a chunked transfer. 8 MiB is about 28 times the measured 30-turn chat.
3. **`sandbox.chatDataSizeLimit` default**: `10Gi` (recommended), half of `sandbox.pvcSize`. A chat holds a blobless clone, the session state and harness caches.
4. **Chat keep-alive**: separate `runs.chatKeepAliveSeconds` with default 1800 (recommended). Alternatives: keep one setting; or exclude idle time from the wall-clock budget for chats so a chat pod can live longer than 2 hours.
5. **Snapshot timing**: only at run end (recommended, as decided). A snapshot after every turn would protect against a lost pod in the keep-alive window, at the cost of an upload per turn.
6. **Snapshot retention**: reuse `volumes.retainAfterCloseSeconds` and `volumes.idleTtlSeconds` (recommended, D7), or keep snapshots as long as the chat exists.
7. **Old chat PVCs**: manual `kubectl` deletion (recommended, D9) or a one-time deletion by the retention scan.
8. **Reset for chats**: reject (recommended, D7), or offer "Reset session" that deletes the snapshot.
