# Tasks

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`. The tasks follow the resolved questions of `design.md`.

## 1. Verify the harness session files

- [ ] 1.1 With the current Claude Code harness image: run a chat with two turns, copy only `~/.claude/projects` into an empty `HOME`, start the adapter and call `session/load` with the session id. Record whether it succeeds, which other files below `~/.claude` it reads, and where auto-memory is written. Do the same for OpenCode with `~/.local/share/opencode`. Update `HarnessProfile.session_state` (design D3) and the design if other directories are needed. Also record what each adapter does on `session/load` for an id whose files are missing (error, empty session, or failure only at the first prompt); design D7 doesn't rely on it, but the docs should say it. Measure the session state size (raw tar, and with `ruzstd` `Fastest`) of a real chat and of a review-style session that reads many files; record the numbers in design D4. If one exceeds about 25 % of the 8 MiB limit, switch the compressor to the `zstd` crate at a higher level (19 or lower, or `window_log(23)` or lower, no long mode) and keep `ruzstd` for decompression (design D4).

## 2. Contract and core (`contract/proto`, `df-core`)

- [ ] 2.1 Add `AcpHarness.session_state_dirs` (`repeated string`), `Continuation.session_state` (`bytes`), the message `SessionState { bytes archive }` and `RunReport.session_state` (`SessionState`), with comments that state the meaning of unset, empty and non-empty (design D5). Assign the final numbers: the next free number of each message at implementation time, checked against its `reserved` entries. Replace the placeholders `<n1>`–`<n3>` in `design.md` D5 with them. Verify with `cargo build -p df-proto`.
- [ ] 2.1a Write a failing test (an `Assign` of 5 MiB through the real runtime client), then set `max_decoding_message_size(16 MiB)` on the client built by `connect_channel` (`factory-agent/src/connection.rs`), matching `MAX_MESSAGE_BYTES` of the server (design D5). Verify with `cargo test -p factory-agent connection`.
- [ ] 2.2 Write failing tests, then add `HarnessProfile.session_state` (`.claude/projects`, `.local/share/opencode`) and `SESSION_STATE_MAX_BYTES = 8 MiB`. Verify with `cargo test -p df-core harness`.
- [ ] 2.3 Write failing config tests, then add `sandbox.readOnlyDataSizeLimit` (default `10Gi`) and `runs.chatKeepAliveSeconds` (default 1800). Add a quantity check in `df-core` that implements the full Kubernetes quantity grammar and requires a value above zero (design D2), and call it at startup for `sandbox.readOnlyDataSizeLimit` and `sandbox.pvcSize`, with an error naming the option. Tests: `10Gi`, `500Mi`, `1.5G`, `10e9`, `1E3`, `20000m`, `+5Gi` valid; `lots`, `0`, `-1Gi`, `10GB`, `1.5.0G`, empty invalid. Verify with `cargo test -p df-core config`.
- [ ] 2.4 Add `DataVolume { Pvc { claim_name }, EmptyDir { size_limit } }` to `RunSandboxSpec` and drop the unused `volume_name` field (design D2).

## 3. Read-only property (`df-db`, `df-orchestrator`, `df-gateway-control`, `df-agent-channel`)

- [ ] 3.1 Add the migration (design D1, D6, D10): `tasks.read_only_workspace` backfilled from `kind = 'chat'` with `CHECK (read_only_workspace = (kind = 'chat'))`; table `task_session_states`; for tasks with a read-only workspace `volume_deleted_at` set and `volume_provisioned_at` cleared, as `delete_volume` does. Regenerate `.sqlx/`. Verify with a migration test (backfill, check rejects a mismatch, cascade on task delete) and `cargo build --workspace` offline.
- [ ] 3.2 Write failing tests, then set `read_only_workspace` on chat creation and load it into the task row of `RunContext`; add `RunContext::read_only_workspace()`. Verify with `cargo test -p df-db tasks` and `cargo test -p df-orchestrator run_context`.
- [ ] 3.3 Write failing tests, then add `ActiveRun.read_only` from the column and use it in `git_route` instead of `key.is_chat()`. Test: a route built from an `ActiveRun` with `read_only: true` and a non-chat key is read-only with no task and no WIP branch. Verify with `cargo test -p df-gateway-control snapshot`.
- [ ] 3.4 Write failing tests, then build `Workspace.read_only` and the empty task and WIP branch in `build_assign` from `read_only_workspace()` instead of `is_chat()`. Chat tools and instructions keep `is_chat()`. Verify with `cargo test -p df-agent-channel assign`.

## 4. Pod rendering and scheduling (`df-k8s`, `df-orchestrator`)

- [ ] 4.1 Write failing render tests (`EmptyDir` gives an `emptyDir` with `sizeLimit` and no PVC; `Pvc` unchanged), then render `task-data` from `DataVolume` in `agent_volumes`. Verify with `cargo test -p df-k8s render`.
- [ ] 4.2 Write failing scheduler tests, then skip `ensure_task_volume` and `volume_ensured` for read-only workspaces and build `DataVolume` from `read_only_workspace()`. Tests: a chat run creates no PVC and no `volume.provisioned` event; a task run still does. Verify with `cargo test -p df-orchestrator scheduler`.

## 5. Session state storage, retention and reset (`df-db`, `df-web`)

- [ ] 5.1 Write failing tests, then implement `df_db::task_session_states::{store, delete, for_assign}` keyed by task id. `for_assign` returns the state unless a run of the task newer than the row's `run_id` has a `turn_started` transcript event (design D7). Tests: overwrite at run end; deletion on an empty archive; a run without a `turn_started` event in between still gives the state; a run with a `turn_started` event that was lost in between gives `None`; the row is deleted with its task. The queries take a task id and never read the key.
- [ ] 5.2 Write a failing test, then add `NOT t.read_only_workspace` to `due_for_retention`. Test: an archived chat past `retainAfterCloseSeconds` with a stored session state is not due, and its session state stays. Verify with `cargo test -p df-db volumes`.
- [ ] 5.3 Write a failing test, then hide "Reset workspace" in the task view for a task with a read-only workspace. Verify with `cargo test -p df-web reset_workspace`.

## 6. Control plane channel (`df-agent-channel`)

- [ ] 6.1 Write failing tests, then fill `session_state_dirs` (read-only workspaces only) and `continuation.session_state` with its session id from `for_assign` in `build_assign`. For a read-only workspace without a delivered snapshot, `continuation.harness_session_id` is empty: remove the fallback to the newest earlier id and `tasks.harness_session_id` for these runs (design D7). Tests: snapshot delivered with its id; no snapshot gives an empty id even when `tasks.harness_session_id` is set; writable workspaces unchanged. Verify with `cargo test -p df-agent-channel assign`.
- [ ] 6.2 Write failing tests, then handle `RunReport.session_state` in `record_run_report` for read-only workspaces: non-empty archive stores, empty archive or one above the limit deletes (warning above the limit), unset keeps the row (design D6). Verify with `cargo test -p df-agent-channel run_report`.
- [ ] 6.3 Write failing tests with `tokio::time::pause()`, then compute the idle deadline from `runs.chatKeepAliveSeconds` for chats (by kind). Tests: a chat run is not released at 10 minutes and is released at 30; a task run at 10. Verify with `cargo test -p df-agent-channel session`.

## 7. Runtime (`factory-agent`)

- [ ] 7.1 Add `tar` and `ruzstd` to `[workspace.dependencies]` and `factory-agent` (latest releases, minimal features; skill `rust`), update `Cargo.lock` and the Bazel lock files. Compress with `ruzstd`'s best implemented level (`Fastest` today, design D4). Verify with `cargo deny check` and a static musl build of `factory-agent` with Bazel.
- [ ] 7.2 Write failing tests, then start the harness as the leader of a new process group and kill the group in `stop_harness` before packing (design D4, best effort). Verify with `cargo test -p factory-agent harness`: a background child of the harness is gone after stop.
- [ ] 7.3 Write failing tests, then implement `session_state::pack` (design D4) on file descriptors only: `openat` from the `HOME` fd, directories with `O_NOFOLLOW | O_DIRECTORY | O_CLOEXEC`, files with `O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY | O_CLOEXEC`, type and `nlink == 1` from `fstat` on the open fd, reads bounded by `take(st_size)` (a short file gives an empty archive), limits of 64 MiB of uncompressed tar stream (a counting writer between tar builder and compressor) and 10,000 entries, skipped-entry count, the compressed size limit, the `SessionState` presence rules. Tests with `tempfile`: a symlink to a file outside, an intermediate directory replaced by a symlink after listing (inject the swap through a test hook between list and open), a hard link, a FIFO (no block), a missing directory, an archive above the limit, a sparse large file, more than 10,000 files, 4,096 files of 16,383 bytes (content below 64 MiB, tar stream above), a directory with several links (included), a file that shrinks during the read.
- [ ] 7.4 Write failing tests, then implement `session_state::restore`: zstd window check before decoding, the whole decompressed stream capped with `Read::take(65 MiB)` before the tar reader, at most 10,000 entries, entry type, raw path and prefix checks, `openat` creation from the `HOME` fd with `O_NOFOLLOW`, exclusive file creation, cleanup on failure by fd (`unlinkat` from the `HOME` fd, no follow), skip when a declared directory exists. Tests: `..` entry, absolute path, symlink entry, hard-link entry, entry outside the declared directories, decompression bomb, oversized PAX header, oversized zstd window (above 8 MiB rejected, exactly 8 MiB accepted), a pre-existing symlink at a declared path, and a round trip: an archive from `pack` passes `restore`, which also pins the window `ruzstd` `Fastest` declares.
- [ ] 7.5 Write failing tests, then validate `session_state_dirs` on the raw string split at `/` (not with `Path::components()`, design D3), call `restore` once before harness start, skip `session/load` after a failed restore (notice `harness.session_restore_failed`), call `pack` in `finalize_run` after the harness stopped, and leave `RunReport.session_state` unset unless at least one turn started in the run (covers the early-exit paths `end_run_early` and `collected.fail`, and a session driver error before the first turn; design D5). Test: harness started, session setup fails before the first turn → field unset. The runtime decides only on the non-empty list. Verify with `cargo test -p factory-agent`.

## 8. Fake agent and end-to-end

- [ ] 8.1 Let `fake-acp-agent` write a session file below a declared directory and fail `session/load` when it is missing. Point the fake profile's `session_state_dirs` at it.
- [ ] 8.2 Add an e2e test (skill `e2e`): a chat answers, its run is released (`runs.chatKeepAliveSeconds: 0`), the next message starts a new pod without a PVC, the session loads and no `context_compacted` notice appears. Verify with `just e2e-local`.

## 9. Deployment and docs

- [ ] 9.1 Add `sandboxNamespace.readOnlyDataSizeLimit` and `runs.chatKeepAliveSeconds` to the Helm values, ConfigMap, schema and `deploy/helm/dark-factory/README.md`. Verify with the Helm tests.
- [ ] 9.2 Update the published docs and verify with `just docs-build`:
  - `docs/content/using/chats.md`: no volume, session restore, what is lost between runs, chat keep-alive; the volume retention text for archived chats removed;
  - `docs/content/architecture/agent-runs.md`: read-only workspaces, data volume per workspace mode, session state flow (sequence diagram), task volumes section;
  - `docs/content/using/opencode.md`: "Session resume" for read-only workspaces;
  - `docs/content/integrations/lifecycle-events.md`: no volume events for read-only workspaces;
  - `docs/content/operations/backup.md`: session states are in PostgreSQL;
  - `docs/content/operations/upgrades.md`: deleting old chat PVCs. Select them by label, not by name: list first with `kubectl get pvc -n <ns> -l app.kubernetes.io/managed-by=dark-factory,dark-factory.io/instance=<instance> -L dark-factory.io/task-key`, check that the rows whose task key ends in `-CHAT` are old chat volumes, then delete exactly those names;
  - `docs/content/security/sandbox.md` and `docs/content/security/git-proxy.md`: session state packing and unpacking rules; push rejection for read-only workspaces;
  - `crates/df-agent-channel/docs/configuration.md`: reference entries for `runs.chatKeepAliveSeconds` and `sandbox.readOnlyDataSizeLimit` (variable, optional, default, effect);
  - `crates/factory-agent/docs/claude-acp.md`: resume for read-only workspaces (snapshot restore, no `session/load` without a snapshot);
  - `crates/factory-agent/docs/configuration.md`: keep-alive for chats, the new `Assign` and `RunReport` fields, and reference entries for the notices `harness.session_state_skipped`, `harness.session_state_too_large` and `harness.session_restore_failed`;
  - `docs/content/architecture/_index.md`: `/data` is the task volume only for writable workspaces;
  - `docs/content/operations/sandbox-nodes-and-resources.md` (created by `sandbox-scheduling-resources`; if that change isn't merged yet, wait for it or add the section there when it lands): a section "Ephemeral storage for read-only workspaces" (design D2):
    - the `emptyDir` at `/data` counts toward the pod's ephemeral storage, together with the container's writable layer (including `/tmp`) and its logs;
    - `sandbox.readOnlyDataSizeLimit` alone already evicts a pod whose `/data` grows above it;
    - `sandbox.resources.requests.ephemeral-storage` lets the scheduler place sandboxes only on nodes with enough free disk. It applies to every sandbox pod, also to tasks with a PVC, so size it for the typical use of a read-only run (clone plus session), not for the size limit;
    - a `sandbox.resources.limits.ephemeral-storage` must be at least `sandbox.readOnlyDataSizeLimit` plus room for `/tmp` and the writable layer, otherwise the container limit evicts the pod before the `emptyDir` limit applies; without a strong reason, set no limit;
    - an example values snippet, and that values per run kind are a backlog item ("Sandbox pod resources");
  - `docs/content/operations/troubleshooting.md`: the `Evicted` row also names an `emptyDir` above `sandbox.readOnlyDataSizeLimit` (`Usage of EmptyDir volume "task-data" exceeds the limit`) and an exceeded ephemeral-storage limit, with the fix for each.
- [ ] 9.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `openspec validate read-only-workspace-emptydir --strict`.
