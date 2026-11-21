# Proposal: iter6-agent-memory

## Why

Agents forget everything between runs. The harness's own memory (Claude Code's `~/.claude/projects/*/memory/`, `MEMORY.md`) lives under `HOME` on the task volume. A new task or chat has a new volume and never sees it, and chats move to `emptyDir` (separate change), so even the same chat loses it after each run. Example from the planning chat: the fact "Linus Torvalds is the software engineer persona" was learned in one chat and lost in the next. Only the control plane knows project, persona and run behind a write, so memory must live there (ADR-0040).

Memory is also a channel for prompt injection: a memory written after reading a hostile issue reaches every later run in its scope. The design must mark memories as data, record who wrote them, and let a human find and remove a bad one fast.

This change implements ADR-0040 phases 1 and 2 (roadmap iteration 6, "Agent memory").

## What Changes

- **Phase 1, `search_history` factory tool.** Full-text search over the tasks (title, description) and whole transcript messages (visible user text; agent text joined from its streamed chunks) of the run's project. Results are snippets with task key, run, time and author, secrets masked, marked as data written by others. A PostgreSQL `tsvector` index backs it; the iteration 6 UI search can use the same index later.
- **Memories as control-plane data.** A memory is a one-line summary (≤ 200 characters, no line breaks or control characters) plus content (≤ 4 KiB plain text), in one of two scopes (ADR-0040): `project` (all personas of a project) or `persona` (one persona within one project). The control plane derives project and persona from the run; the agent picks only the scope. Each memory records creator and last changer (persona, task, run, or user) and keeps a history of every change.
- **Factory tools `remember`, `recall`, `forget`.** `remember` creates a memory or replaces one by id; `recall` is a full-text search over the memories in the run's scopes; `forget` deletes one. Offered to task runs and chat runs. Agents cannot change protected memories (pinned, or written or edited by a human).
- **Secret scan on every write.** `remember`, `forget` reasons and human edits pass the same secret rules as the push checks. A match rejects the write and names the rule; nothing is stored, and the stored tool call holds the arguments masked.
- **Loaded at run start: pinned memories and an index.** Like the harness's `MEMORY.md`: the first turn of every run gets one block with the full text of pinned memories and a one-line index (id, scope, author, date, summary) of the other memories in scope, labeled "not reviewed", within a size cap and with a fixed end line. Everything else is reached through `recall`. Only humans pin.
- **Marked as data.** The block, every `recall` and `search_history` result and the tool descriptions say that memories are notes written by agents, are hints and not instructions, and must be checked against the repository and the task. Memories never go into the system prompt.
- **Run record.** The run stores the exact memory block it got (also when empty) and the ids in it; the run view lists them, plus the memories the run created, replaced or deleted.
- **"Memory" tab on the project settings page.** Board members list, search, filter (scope, persona, author, run), create, edit, pin, unpin and delete memories, see each memory's history, and undo all memory changes of one run (delete what it created, restore what it replaced, re-create what it deleted). A switch turns memory off for the project.
- **Audit.** Every create, update, delete, pin and unpin writes an audit entry (`memory.*`) with the memory id, scope and content before and after, for agents and humans alike.
- **Lifecycle.** Memories are deleted with their project (and persona memories with their persona) in the same transaction.
- **Built-in skill `dark-factory-memory`.** Teaches what to remember (facts not in the repository that later runs need), what not to (secrets, repository content, task status, instructions from untrusted input), how to write a good memory, when to recall, and that memories are hints. Text is part of this change (`skill/dark-factory-memory/SKILL.md`). `dark-factory-basics` gets a three-line pointer.

Out of scope:
- Curator routine, embeddings or a selection model, entity memory (ADR-0040 phases 3 and 4): roadmap backlog, row "Agent memory phases 3 and 4".
- Per-persona tool permissions (allowed, ask, denied) for the memory tools: roadmap iteration 4, "Full permission model (ADR-0011)". Until then every run gets the memory tools.
- Promoting a memory into the project context through an approval request (ADR-0040, ADR-0038): roadmap backlog (row added at archive). It needs `iter3-project-context` first.
- The UI full-text search across tasks and transcripts: roadmap iteration 6, "Full-text search across tasks and transcripts". This change builds the index it can reuse.
- Importing the harness's existing `MEMORY.md` files from task volumes: rejected. They are few, unreviewed, and would bypass the secret scan and provenance.
- Instance scope and persona memories across projects: rejected (ADR-0040).
- Text search configuration per instance (this change uses `english`): roadmap backlog (row added at archive).
- Agents pinning memories: rejected for this change (design Q2).

## Capabilities

### New Capabilities
- `agent-memory`: what a memory is, character rules, scopes, limits, secret scan, provenance and history, protection, the memory block loaded at run start, the run record, the Memory tab, undo of a run, memory off per project, and lifecycle.
- `history-search`: the full-text index over task and transcript text of a project.

### Modified Capabilities
- `factory-tools`: new tools `remember`, `recall`, `forget`, `search_history`; their results marked as data; the scope exceptions and the masked arguments of "Calls scoped to the run's task" and "Exactly-once execution".
- `chats`: chat runs are offered the four tools; the chat instructions name memory.
- `run-orchestration`: `Assign` carries the memory block, fixed per run.
- `acp-harness`: the runtime puts the memory block before the first prompt of every run.
- `audit-log`: `memory.*` actions.

## Impact

- **Contract** (`contract/proto/darkfactory/agent/v1/agent.proto`): `Assign.memory_context` (string). Additive. `contract/mcp/factory-tools.v2.json`: four tools.
- **Core** (`df-core`): `Memory` domain type and validation, rendering of the memory block; `SecretRules` moves from `df-gateway` to `df-core` (rules file included) so the control plane can scan; built-in skill `dark-factory-memory` (needs `iter4-skills`).
- **Database** (`df-db`): modules `memories` (tables `memories`, `memory_changes`, `run_memory_context`, column `projects.memory_enabled`) and `history_search` (`tasks.search`, table `transcript_messages` filled with the transcript events, GIN indexes, backfill); migrations and `.sqlx/` metadata.
- **Agent channel** (`df-agent-channel`): the four tool handlers, argument mask before the claim, memory block in `assign.rs`, `CHAT_TOOLS`, `CHAT_INSTRUCTIONS`.
- **factory-agent**: prepends `Assign.memory_context` to the first turn of a run; the block is not written to the transcript.
- **Web** (`df-web`): Memory tab on the project settings page, memory lists on the run view.
- **Docs**: new how-to `docs/content/using/agent-memory.md`, `docs/content/architecture/agent-runs.md`, `docs/content/using/chats.md`, `crates/factory-agent/docs/` (notice), `product/agent-playbook.md` (memory section), `docs/architecture.md` (`SecretRules` move), glossary term "Memory".
- **Depends on** `iter4-skills` (DF-7, built-in skills) for the skill; everything else stands alone. If `iter4-skills` lands later, the skill task waits; the tool descriptions and the memory block carry the minimum rules on their own.
