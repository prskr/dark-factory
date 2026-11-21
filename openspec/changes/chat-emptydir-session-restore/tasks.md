# Tasks

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`. Resolve the open questions of `design.md` before task 2; the tasks use the recommended answers.

## 1. Verify the harness session files

- [ ] 1.1 With the current Claude Code harness image: run a chat with two turns, copy only `~/.claude/projects` into an empty `HOME`, start the adapter and call `session/load` with the session id. Record whether it succeeds, which other files below `~/.claude` it reads, and where auto-memory is written. Do the same for OpenCode with `~/.local/share/opencode`. Update `HarnessProfile.session_state` (D2) and the design if other directories are needed.

## 2. Contract and core (`contract/proto`, `df-core`)

- [ ] 2.1 Add `AcpHarness.session_state_dirs = 6`, `Continuation.session_state = 3` and `RunReport.session_state = 6` (`optional bytes`) with comments. Verify with `cargo build -p df-proto`.
- [ ] 2.2 Write failing tests, then add `HarnessProfile.session_state` (`.claude/projects`, `.local/share/opencode`) and `SESSION_STATE_MAX_BYTES = 8 MiB`. Verify with `cargo test -p df-core harness`.
- [ ] 2.3 Write failing config tests, then add `sandbox.chatDataSizeLimit` (default `10Gi`, startup check of the quantity) and `runs.chatKeepAliveSeconds` (default 1800). Verify with `cargo test -p df-core config`.
- [ ] 2.4 Add `DataVolume { Pvc { claim_name }, EmptyDir { size_limit } }` to `RunSandboxSpec` and drop the unused `volume_name` field (design D1).

## 3. Pod rendering and scheduling (`df-k8s`, `df-orchestrator`)

- [ ] 3.1 Write failing render tests (chat pod with `emptyDir` `sizeLimit` and no PVC; task pod unchanged), then render `task-data` from `DataVolume` in `agent_volumes`. Verify with `cargo test -p df-k8s render`.
- [ ] 3.2 Write failing scheduler tests, then skip `ensure_task_volume` and `volume_ensured` for chats and build `DataVolume` by kind. Tests: a chat run creates no PVC and no `volume.provisioned` event; a task run still does. Verify with `cargo test -p df-orchestrator scheduler`.

## 4. Storage (`df-db`)

- [ ] 4.1 Add the migration: table `chat_session_states` (design D5), and set `volume_deleted_at` for existing chats with a volume (D9). Regenerate `.sqlx/`. Verify with a migration test and `cargo build --workspace` offline.
- [ ] 4.2 Write failing tests, then implement `df_db::chat_session_states::{store, delete, for_assign}`. `for_assign` returns the state only when its run is the newest earlier connected run of the chat (D6). Tests: overwrite at run end; deletion on a report without state; a lost run in between gives `None`.
- [ ] 4.3 Write failing tests, then limit `due_for_retention` to `kind = 'task'` and add `due_chat_session_states` with the chat rules (archived past `retainAfterCloseSeconds`, idle past `idleTtlSeconds`, no non-terminal run). Verify with `cargo test -p df-db volumes`.
- [ ] 4.4 Write a failing test, then make `reset_workspace` reject a chat with a conflict and no audit entry; map it to 409 in `task_reset_workspace` (`df-web`). Verify with `cargo test -p df-db volumes` and `cargo test -p df-web reset_workspace`.

## 5. Control plane channel (`df-agent-channel`, `df-orchestrator`)

- [ ] 5.1 Write failing tests, then fill `session_state_dirs` (chats only) and `continuation.session_state` with its session id from `for_assign` in `build_assign`. Verify with `cargo test -p df-agent-channel assign`.
- [ ] 5.2 Write failing tests, then store or delete the chat's session state in `record_run_report`, rejecting state above the limit with a warning. Verify with `cargo test -p df-agent-channel run_report`.
- [ ] 5.3 Write failing tests with `tokio::time::pause()`, then compute the idle deadline from `runs.chatKeepAliveSeconds` for chats. Tests: a chat run is not released at 10 minutes and is released at 30; a task run at 10. Verify with `cargo test -p df-agent-channel session`.
- [ ] 5.4 Write failing tests, then make `Retention` delete due chat session states (no lifecycle event). Verify with `cargo test -p df-orchestrator retention`.

## 6. Runtime (`factory-agent`)

- [ ] 6.1 Add `tar` and `flate2` to `[workspace.dependencies]` and `factory-agent` (latest releases, minimal features; skill `rust`), update `Cargo.lock` and the Bazel lock files. Verify with `cargo deny check`.
- [ ] 6.2 Write failing tests, then implement `session_state::pack` (design D3): no-follow walk, regular single-link files and directories only, skipped-entry count, size limit. Tests with `tempfile`: a symlink to a file outside, a hard link, a FIFO, a missing directory, an archive above the limit.
- [ ] 6.3 Write failing tests, then implement `session_state::restore`: entry type, path and prefix checks, no-follow creation with `rustix`, exclusive file creation, 64 MiB and 10,000-entry limits counted while writing, cleanup on failure, skip when a declared directory exists. Tests: `..` entry, absolute path, symlink entry, hard-link entry, entry outside the declared directories, decompression bomb, a pre-existing symlink at a declared path, reconnect skip.
- [ ] 6.4 Write failing tests, then validate `session_state_dirs`, call `restore` before harness start, skip `session/load` after a failed restore (notice `harness.session_restore_failed`), and call `pack` in `finalize_run` after the harness stopped, filling `RunReport.session_state`. Verify with `cargo test -p factory-agent`.

## 7. Fake agent and end-to-end

- [ ] 7.1 Let `fake-acp-agent` write a session file below a declared directory and fail `session/load` when it is missing. Point the fake profile's `session_state_dirs` at it.
- [ ] 7.2 Add an e2e test (skill `e2e`): a chat answers, its run is released (`runs.chatKeepAliveSeconds: 0`), the next message starts a new pod, the session loads and no `context_compacted` notice appears. Verify with `just e2e-local`.

## 8. Deployment and docs

- [ ] 8.1 Add `sandboxNamespace.chatDataSizeLimit` and `runs.chatKeepAliveSeconds` to the Helm values, ConfigMap, schema and `deploy/helm/dark-factory/README.md`. Verify with the Helm tests.
- [ ] 8.2 Update the published docs and verify with `just docs-build`:
  - `docs/content/using/chats.md`: no volume, session restore, what is lost between runs, chat keep-alive, archived chats lose their session state after retention;
  - `docs/content/architecture/agent-runs.md`: data volume per kind, session state flow (sequence diagram), task volumes section;
  - `docs/content/using/opencode.md`: "Session resume" for chats;
  - `docs/content/integrations/lifecycle-events.md`: no volume events for chats;
  - `docs/content/operations/backup.md`: chat session states are in PostgreSQL;
  - `docs/content/operations/upgrades.md`: deleting old chat PVCs (`kubectl get pvc -n <ns> -o name | grep -- '-chat$' | xargs kubectl delete -n <ns>`);
  - `docs/content/security/sandbox.md`: session state packing and unpacking rules;
  - `crates/df-agent-channel/docs/configuration.md`: reference entries for `runs.chatKeepAliveSeconds` and `sandbox.chatDataSizeLimit` (variable, optional, default, effect).
- [ ] 8.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `openspec validate chat-emptydir-session-restore --strict`.
