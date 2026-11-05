# Tasks

Prerequisites, all merged before this change:
- `iter3-task-dependencies`: `parent_id`, `create_task`, `list_personas`, `send_message`, the task transitions (`transition`, `apply_task_event`, ADR-0055), persona descriptions.
- `iter2-continuation-summary`: the summary the compaction notice shows.
- `iter2-run-resilience`: `runs.maxConcurrent` in `start_queued_runs`.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Schema and domain

- [ ] 1.1 Write failing `df-core` tests, then add `TaskKind::{Task, Chat}` and the chat flag on `TaskKey`. Verify with `cargo test -p df-core`. The tests cover:
  - `DF-12-CHAT` parses and displays;
  - `DF-12-chat` and `DF-12-CHAT-1` are rejected;
  - the branch name helpers return none for a chat key.
- [ ] 1.2 Add the migration of design D1:
  - `tasks.kind`;
  - `archived_at` and `archived_by` with their checks;
  - the key check with the `-CHAT` suffix;
  - the chat list index.

  Then regenerate `.sqlx/`. Verify with a `df-db` migration test:
  - existing rows get kind `task`;
  - a `task` with a `-CHAT` key is rejected;
  - a `chat` without the suffix is rejected;
  - `cargo build --workspace` passes offline.
- [ ] 1.3 Add `runs.maxConcurrentChats` to `InstanceConfig` (default 2, 0 rejected) and the Helm values. Document it next to `runs.maxConcurrent` in the configuration reference. Verify with a `df-core` config test and `helm unittest`.

## 2. Chat lifecycle in the database

- [ ] 2.1 Write failing `df-db` tests, then implement `create_chat` (design D2). Verify with `cargo test -p df-db chat_create`. The tests cover the `chats` scenarios "New chat", "Several chats with one persona", "Empty first message" and "Audit on start", and the `task-management` scenario "Chat between tasks".
- [ ] 2.2 Write failing tests, then implement archive and unarchive as the `Archived` and `Unarchived` events (design D6). Verify with `cargo test -p df-core task_transition` for the rules and `cargo test -p df-db chat_archive` for the writes. The tests cover every scenario of the `chats` requirements:
  - "Archiving a chat";
  - "Archived chats are read-only", the message rejection only;
  - "Unarchiving a chat".
- [ ] 2.3 Write failing `transition` unit tests, then reject `MarkedDone` and `Cancelled` for chats (design D6). Verify with `cargo test -p df-core task_transition` and `cargo test -p df-db`: the `task-management` scenario "Mark a chat done", the `chats` scenario "Cancel rejected", and no audit entry written.
- [ ] 2.4 Count `archived_at` as the close time of a chat in the volume retention scan. Verify with a retention test: an archived chat's volume is deleted after `volumes.retainAfterCloseSeconds`.

## 3. Scheduling

- [ ] 3.1 Write failing `df-agent-channel` tests, then split `start_queued_runs` into task and chat budgets and skip archived chats (design D5). Verify with `cargo test -p df-agent-channel scheduler`. The tests cover:
  - the `chats` scenarios "Tasks don't block chats" and "Chat limit reached";
  - the `run-orchestration` scenario "Archived chat with a queued message".
- [ ] 3.2 Write failing tests, then release the longest idle chat run when a chat is queued without a free slot. This covers the `ReleaseIdle` notification and the session handling it like an expired keep-alive. Verify with tests for the `chats` scenarios "Release the longest idle chat" and "No idle run", and that the released chat keeps its status.
- [ ] 3.3 Add an integration test for the `chats` scenario "Subtask ends while archived" and the unarchive scenarios: a child-ended message on an archived chat starts no run, and unarchiving starts one. Verify with `cargo test -p df-agent-channel archived_chat`.

## 4. Chat runs

- [ ] 4.1 Write failing tests, then build `Assign` for chats (design D3). Verify with `assign.rs` tests for the `run-orchestration` scenario "Assign for DF-12-CHAT" and the `chats` scenarios "System prompt of a chat run" and "Task runs unchanged". This covers:
  - the system prompt with `CHAT_INSTRUCTIONS`;
  - `Workspace.read_only` (additive proto field), with no task or WIP branch;
  - the filtered tool list.
- [ ] 4.2 Write failing tests, then reject tools a chat run isn't offered in the tool dispatch. Verify with `tools.rs` tests: the `chats` scenarios "Completing a chat" and "Delegation from a chat", and `ask_question` rejected.
- [ ] 4.3 Write failing gateway tests, then add `GitRoute.read_only` (additive) and the `df:read-only` rejection in `RefScope`, set from the snapshot for chat runs. Verify with `cargo test -p df-gateway` for the `git-proxy` scenarios "Push from a chat" and "Fetch from a chat".
- [ ] 4.4 Write failing runtime tests, then implement the read-only prepare and skip all pushes in finalize and per-turn (design D4). Verify with `cargo test -p factory-agent workspace` for the `workspace-lifecycle` scenarios "Prepare a chat workspace" and "Commit in a chat", and the `chats` scenario "Base branch moved between runs".
- [ ] 4.5 Write a failing runtime test, then emit the `context_compacted` notice when a new session is seeded with a summary (design D7). Verify with `factory-agent` tests for the `run-orchestration` scenarios "Session not resumable" and "Session resumed".
- [ ] 4.6 Document chat runs in `crates/factory-agent/docs/` (read-only workspace, no pushes) and the `context_compacted` notice in the transcript reference. Verify that the docs name both and the doc tests (if any) pass.

## 5. Web: shell and task pages

- [ ] 5.1 Write failing `df-web` tests, then add `Nav::Chats` to the rail and the bottom bar and the "New chat" floating button on the chat list. Verify with tests for the `web-ui-shell` scenarios "Rail on the chat page", "Task list on a phone" and "Chat list on a phone".
- [ ] 5.2 Write failing tests, then filter chats out of the task list and link parent chats and inbox rows of chats to `/chats/{key}`. Verify with tests for the `task-ui` scenarios "Chat in the project" and "Task created in a chat", and the `request-inbox` scenario "Approval request from a chat".

## 6. Web: chat pages

- [ ] 6.1 Write failing tests, then add `chat_pages.rs` with the routes of design D8, reusing transcript, message box and SSE. Task routes return 404 for chat keys and vice versa. Verify with tests for the `chat-ui` scenarios "Open a chat" and "Default project", and with message, stop, rename and archive round trips over HTTP.
- [ ] 6.2 Write failing tests, then render the project and chat list:
  - badges, needs-you marker and ordering;
  - the topic placeholder;
  - the archived entry;
  - the empty state.

  Verify with tests for the `chat-ui` scenarios "Idle chat without badge", "Running chat", "Topic not set yet", "Open approval request" and "No chats yet".
- [ ] 6.3 Write failing tests, then render the center (header with "New chat", transcript with `create_task` rows, `context_compacted` notice) and the details tabs (Overview with Archive state, Subtasks, Artifacts with the cards of `iter3-artifacts`). Verify with tests for the `chat-ui` scenarios:
  - "Delegation in the transcript";
  - "Expand the summary";
  - "Archive disabled while running";
  - "Subtasks tab";
  - "Many artifacts".
- [ ] 6.4 Add the Alpine component `chatLayout.js`, registered in `COMPONENT_SCRIPTS`, for collapsing both columns and the persona strip; the artifact split view is the component of `iter3-artifacts` (`artifactSplit.js`), reused as is. Verify with:
  - `script_source` tests for its state transitions;
  - the `chat-ui` scenarios "Persona with a chat that needs you", "Collapse details", "Open from the transcript" and "Resize", checked in a browser with `just serve-local`.
- [ ] 6.5 Write failing tests, then add the new chat dialog (fixed project, persona picker with descriptions). Verify with a test for the `chat-ui` scenario "Start from the dialog".
- [ ] 6.6 Write failing tests, then add the archive modal (`archiveModal.js`): list, search, sidebar with Transcript and Artifacts tabs (cards and rendered view of `iter3-artifacts`), opened artifact, Unarchive. Verify with tests for the `chat-ui` scenarios "Search", "Unarchive from the modal" and "Open an archived artifact".
- [ ] 6.7 Add the phone layouts of all chat screens. Verify the `chat-ui` scenarios "Open a chat on a phone" and "Artifact on a phone" with tests, and check every phone screen of the Penpot page "Chat" at 390 px in a browser with viewport control.

## 7. Docs and end-to-end

- [ ] 7.1 Write the how-to "Plan work in a chat": start a chat, delegate, archive, unarchive. Link it from the docs index. Verify that the steps match the running UI.
- [ ] 7.2 Extend `crates/df-e2e` with the flow of design D9, scripted with the fake ACP agent. Verify with `just e2e-local`.
- [ ] 7.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and the Bazel format check, and fix any findings. Verify that all pass.
