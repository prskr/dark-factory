# Tasks

> **Decisions first:** the open questions Q1–Q10 in `design.md` are answered by the user before group 4 starts; write the answers back into `design.md` and the spec deltas. Group 2 (PenPot) must be approved before group 7. Group 3 (phase 1) depends only on group 1 and can ship first. Group 8 needs `iter4-skills` merged.
>
> Tests marked "container-backed" need a PostgreSQL container (`df_test_postgres`); state in the report which of them could not run.

## 1. Secret rules in `df-core`

- [ ] 1.1 Move `SecretRules`, `SecretRule`, `SecretRulesError`, the masking helper and `rules/secrets.yml` with `NOTICE` from `df-gateway` to `df-core` (`src/secrets.rs`, `rules/`), keeping the existing tests passing in their new place; `df-gateway` keeps its git matcher and imports the rules. Update `docs/architecture.md` (ownership line of `df-core` and `df-gateway`) and the Bazel `compile_data`. Verify: `cargo nextest run -p df-core secrets` and `cargo nextest run -p df-gateway git`.

## 2. Design agreement (PenPot)

- [ ] 2.1 Draw in PenPot, light, dark and at 390 px width: the Memory tab (search, filters, two-line rows with scope badge, author link and pin marker, empty state), the memory modal (create, edit, provenance, delete confirmation), the "Delete all N memories of this run" confirmation, and the Memory section on the run details with the read-only block view. Verify: the user approves the boards; every changed label is written back into `specs/agent-memory/spec.md` before group 7.

## 3. Phase 1: history search

- [ ] 3.1 Write failing `df-db` tests first for `history-search` "Agent message indexed", "Tool output not indexed", "Other project" and "Archived chat". Then add the migration (`tasks.search`, `transcript_events.search_text` and `search` with GIN indexes, design D9), fill `search_text` at transcript insert for `UserMessage` and `AgentMessage`, and add module `history_search` with the project-limited query (`websearch_to_tsquery`, `ts_rank`, `ts_headline`, 2 KiB cut). Regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db history_search transcript` (container-backed).
- [ ] 3.2 Write a failing migration test first for "Existing transcripts": events stored before the migration are found. Then add the backfill to the migration after checking the stored payload shape of both event kinds. Verify: `cargo nextest run -p df-db history_search` (container-backed).
- [ ] 3.3 Write failing `df-agent-channel` tool tests first for `factory-tools` "Earlier decision", "Audit of the hits" and the data block of "Stored text marked as data" for `search_history`. Then add `search_history` to `factory-tools.v2.json` (description per design D3) and its handler. Verify: `cargo nextest run -p df-agent-channel tools` (container-backed).

## 4. Memory model and storage

- [ ] 4.1 Write failing unit tests first for `Memory` in `df-core` (design D1): summary one line 1–200 characters, content 1–4096 bytes, scope parsing, id format `m<number>` in both directions, secret scan of summary and content with masked match in the error. Then implement. Verify: `cargo nextest run -p df-core memory`.
- [ ] 4.2 Write failing `df-db` tests first for: create in both scopes, replace (provenance of last change), delete, pin and unpin, protection (agents cannot replace or delete human-written or pinned memories), visibility per run (project scope plus own persona scope), the scope limits 500/200, full-text search with ranking and the `ids` lookup, project and persona deletion (`agent-memory`, "Memory lifecycle"). Then add the migration (`memories`, design D2) and module `memories`, and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db memories` (container-backed).
- [ ] 4.3 Write failing tests first for the `memory.*` audit entries (`audit-log`, "Memory changes audited"), written in the same transaction as the change, for agent and human actors. Implement in `df-db` `memories`. Verify: `cargo nextest run -p df-db memories audit` (container-backed).

## 5. Memory tools

- [ ] 5.1 Write failing `df-agent-channel` tool tests first for every scenario of `factory-tools` "remember", "recall" and "forget", for `agent-memory` "Memory limits" (scope full, 21st write in a run), "Secret scan of memory writes" (agent scenario, including masked arguments in the stored call and audit entry), "Protected memories" and "Memory scopes". Then add the three tools to `factory-tools.v2.json` and implement the handlers. Verify: `cargo nextest run -p df-agent-channel tools` (container-backed).
- [ ] 5.2 Write failing tests first for "Stored text marked as data" (result block of `recall`, tool descriptions) and "Memory tools offered" (task run), and the chats scenario "Memory in a chat". Then add the four tools to `CHAT_TOOLS` and the descriptions. Verify: `cargo nextest run -p df-agent-channel assign tools`.

## 6. Memory block at run start

- [ ] 6.1 Write failing `df-core` unit tests first for rendering the block (design D5): header, pinned section with the 8 KiB cap and overflow into the index, index ordering, the 16 KiB cap with the "N more" line, no block without memories, author lines. Then implement. Verify: `cargo nextest run -p df-core memory`.
- [ ] 6.2 Add `string memory_context = 8;` to `Assign` in `agent.proto` with a field comment. Verify: `cargo build --workspace` and `bazel --bazelrc=.bazelrc.agents build //contract/...`.
- [ ] 6.3 Write failing tests first for `run-orchestration` "Memory block in Assign" (all three scenarios) and `agent-memory` "Memory block at run start" ("Pinned and index", "Other persona") and "Memory block fixed per run" ("Reconnect"). Then add `run_memory_context` (migration, `df-db` module `memories`), render and store the block on the first `build_assign` of a run and read it back afterwards. Verify: `cargo nextest run -p df-agent-channel assign` and `cargo nextest run -p df-db memories` (container-backed).
- [ ] 6.4 Write failing `factory-agent` tests first for `acp-harness` "Memory block before the first prompt" (first turn with and without a continuation summary, resumed session, second turn, `memory.loaded` notice). Then implement in `build_acp_prompt_blocks` and the runner. Verify: `cargo nextest run -p factory-agent`.

## 7. Web UI

- [ ] 7.1 Write failing `df-web` tests first for the `agent-memory` "Memory tab" scenarios (search, filters, edit and pin, empty tab, delete with confirmation, delete all of a run), the board-member scenario of "Secret scan of memory writes", and content shown as plain text (a memory containing `<script>` and a Markdown link is shown literally). Then build the tab per the approved PenPot boards. Verify: `cargo nextest run -p df-web memory`.
- [ ] 7.2 Write failing `df-web` tests first for `agent-memory` "Memory on the run view" (both scenarios) on the task page and the chat page. Then implement. Verify: `cargo nextest run -p df-web memory`.
- [ ] 7.3 Seed demo and e2e data: a project with one pinned human memory, two agent memories in both scopes. Write an e2e test first: a run with the fake ACP agent receives the block in its first prompt, calls `remember`, and the new memory appears on the Memory tab with a link to the run. Verify: `just e2e-local` (needs a container socket; otherwise CI).

## 8. Skill

- [ ] 8.1 Requires `iter4-skills`. Write a failing test first that `dark-factory-memory` is a built-in with scope `always` that passes skill validation, and that `dark-factory-basics` contains the "Memory" section. Then copy `skill/dark-factory-memory/SKILL.md` of this change to `crates/df-core/skills/dark-factory-memory/SKILL.md`, add it to the built-in list, and add `skill/dark-factory-basics-memory-section.md` to `dark-factory-basics`. Verify: `cargo nextest run -p df-core skill`.

## 9. Documentation

- [ ] 9.1 Published docs: new how-to `docs/content/using/agent-memory.md` (what memories are, scopes, what agents get at run start, finding and removing a bad memory after a poisoned run, the Memory tab, security model); `docs/content/architecture/agent-runs.md` (four new tools, memory block in the first prompt); `docs/content/using/chats.md` (chat tools list); `crates/factory-agent/docs/` (notice `memory.loaded`, `Assign.memory_context`). Verify: `just docs-build`.
- [ ] 9.2 Contributor and product docs: `product/agent-playbook.md` (memory section matching the skill), `product/GLOSSARY.md` (term "Memory", "Pinned memory"), ADR-0040 amended for the decisions taken on Q1–Q4, `design/tech-stack.md` agent memory row status, roadmap iteration 6 memory line (status) and a backlog row for promoting a memory into the project context. Verify: review of the diff; `openspec validate iter6-agent-memory --strict`.

## 10. Final checks

- [ ] 10.1 Verify: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `cargo xtask check-test-names`, `bazel --bazelrc=.bazelrc.agents test //...` (state which container-backed tests could not run).
