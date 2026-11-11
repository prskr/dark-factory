# Tasks

Prerequisite, merged before this change: `iter3-chats` (task kind `chat`, `CHAT_INSTRUCTIONS`, the archive check, the chat Overview tab). Its `chats` and `chat-ui` specs must be archived so that the MODIFIED deltas here apply. Also merged before: `ui-model-provider-page` (tab "Security", tab script fix).

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Schema and domain

- [ ] 1.1 Write a failing `df-core` test, then add `PROJECT_CONTEXT_MAX_BYTES` (32 KiB) and a validation helper. Verify with `cargo test -p df-core`: exactly 32,768 bytes are accepted and 32,769 are rejected, counted in UTF-8 bytes, not chars.
- [ ] 1.2 Add the migration (design D1, D2):
  - `projects.context` with its length check;
  - `projects.context_version`;
  - `runs.project_context` and `runs.project_context_version`.

  Then regenerate `.sqlx/`. Verify with a `df-db` migration test (existing projects get `''` and version 0, and a 40,000-byte update is rejected by the check), and with `cargo build --workspace` offline.
- [ ] 1.3 Write failing `df-db` tests, then extend project read and update with `context`. The version is bumped only when the text changes, and the `project.update` audit entry carries `context` before and after. Verify with `cargo test -p df-db`: a name-only edit keeps the version, and a context edit bumps it and writes the audit entry.

## 2. Injection into chat runs

- [ ] 2.1 Write failing tests, then add `chat_system_prompt(persona, context)` (design D3). It produces persona, `## Project context` + text, then chat instructions, and leaves out the middle block when the context is empty. Add the proposal line to `CHAT_INSTRUCTIONS`. Verify with `df-agent-channel` unit tests for both cases.
- [ ] 2.2 Write failing tests, then make `build_assign` store `runs.project_context` and its version on the first assign of a chat run, and reuse them on resend. Verify with `df-agent-channel` DB tests:
  - a context edit between first assign and resend leaves `Assign` identical;
  - the next run gets the new text;
  - a task run in the same project has no context and NULL columns.

## 3. Proposals

- [ ] 3.1 Write failing `df-db` tests, then implement `ProjectContextApproval` (design D4) and register it:
  - payload validation and size;
  - SHA-256 fingerprint;
  - scope `project`;
  - lock on the project row;
  - `apply` with the version guard.

  Verify that:
  - approving replaces the text, bumps the version and audits with the decider;
  - a proposal whose `base_version` is behind fails with "context changed since this proposal" and stays pending;
  - the same content twice merges into one pending request.
- [ ] 3.2 Write failing tests, then let `request_approval` accept `project-context`:
  - a tool error from a task run;
  - a size error;
  - `base_version` taken from the raising run, never from the agent's payload.

  Add `project-context` to the type enum in `contract/mcp/factory-tools.v2.json`. Verify with `df-agent-channel` tool tests, including one where the payload carries a forged `base_version` that is ignored.
- [ ] 3.3 Write a failing test, then exclude `project-context` from the open-request check when archiving a chat (design D5). Verify with a `df-db` test: a chat with only a pending `project-context` request archives; one with `network-access` doesn't; and deciding the proposal queues `request.decided` on the archived chat without starting a run.

## 4. Web UI

- [ ] 4.1 Write failing `df-web` tests, then add the Context tab (design D7):
  - `config_pages/project_context.rs` with panel, validation and the `AGENTS.md` hint;
  - `ProjectForm` flatten, `save_project`;
  - `/projects/{key}?tab=context` opens the tab;
  - tab order General, Security, Context, Integrations (Integrations for `admin` only).

  Verify with `df-web` handler tests: saving the context with "Save project", the 32 KiB error opening the Context tab, the tab opened by the query parameter, and the tab order for an admin and a board member. Add the Context tab to the PenPot project settings boards ("Config - Projects") and to `docs/design-workflow.md` if the board "Projects · settings page, context" does not match.
- [ ] 4.2 Write failing tests, then render the unified diff in `project-context` request cards in the inbox and on the chat timeline (design D6), with Approve disabled on a stale version. Add `similar` to `[workspace.dependencies]` (default features off) and to `df-web`. Verify with `df-web` tests (an escaped `<script>` line, added/removed classes, disabled Approve when stale), then `cargo deny check` and `cargo machete`.
- [ ] 4.3 Point the chat Overview's project context link at `/projects/<KEY>?tab=context`. Verify with a `df-web` test of the Overview markup.

## 5. Docs

- [ ] 5.1 Write the how-to "Keep planning knowledge in the project context" in `docs/` (edit in the Context tab; what belongs here vs. `AGENTS.md`; approving a chat's proposal; a stale proposal) and a reference entry for the context (limit, chat runs only, applied from the next run). Verify that the steps match the UI from group 4.

## 6. Integration

- [ ] 6.1 Extend `fake-acp-agent` with a `request_approval` step and `df-e2e` with the scenario: a chat proposes a context, a board member approves it in the inbox, and the next chat run's system prompt contains it. Verify with `cargo test -p df-e2e` (`just e2e-local`).
- [ ] 6.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and the Bazel format check, and verify that all pass.
