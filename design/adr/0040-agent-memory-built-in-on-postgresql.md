# ADR-0040: Agent memory built in, on PostgreSQL, explicit memories first

- **Status:** Accepted (planned after iteration 2)
- **Date:** 2026-10-19
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0012](0012-postgresql-and-blob-store.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md), [ADR-0028](0028-integrations-wit-capability-interfaces.md), [ADR-0032](0032-upstream-mcp-through-gateway-with-tool-catalog.md), [ADR-0038](0038-agent-questions-and-approval-requests.md)

## Context

Agents should carry facts they learned from one task into later ones ("test `foo` is flaky under RBE, retry once"). Harness-native memory doesn't do this: `HOME` lives on the task PVC, so it ends with the task. Two related needs are already covered: instructions and conventions (project context document, ADR-0026; `AGENTS.md`; skills), and the history of earlier tasks (transcripts with full-text search, ADR-0012).

Memory is also a persistence channel for prompt injection: an instruction planted in an issue and stored as a memory affects every later task of the project, bypassing per-task isolation. Only Dark Factory knows the project, persona and run behind a write.

Options were PostgreSQL with pgvector, dedicated vector databases (Qdrant, Chroma), vector-graph databases (HelixDB), and third-party memory systems attached over MCP.

## Decision

- **Built into Dark Factory, stored in PostgreSQL.** Scope, provenance, review and deletion are enforced by the control plane. Storage sits behind a small trait where it's consumed; it becomes a WIT world (ADR-0028) only once a second backend is needed.
- **Scopes:** `project` (shared by all personas of the project) and `project + persona` (one persona within one project, like chats, ADR-0026). Nothing crosses project boundaries; no instance scope. The control plane derives the scope from the run binding; agents choose only between the two, never the project or persona.
- **Two levels of trust:**
  - *Memories* are written by agents and treated as hints, not instructions. Each records its provenance (run, task, persona, time) and is visible, editable and deletable in the UI.
  - *Project context* stays human-approved instructions. Promoting a memory into it is an approval request (ADR-0038) decided with `project:context:write` (ADR-0011).
- **Phases:**
  1. `search_history` factory tool over tasks and transcripts, using the existing full-text search.
  2. Explicit memories: factory tools `remember`, `recall`, `forget`; full-text search; pinned memories injected into the first prompt of every run in scope; memory writes pass the same secret scanning as pushes; the run record lists the memories injected into it.
  3. Better retrieval and curation, chosen when phase 2 shows the need: a curator persona running as a routine (consolidate duplicates, replace contradicted memories, expire stale ones) and selection of relevant memories either by embeddings (pgvector, hybrid with full-text search) or by a small selection model on a local provider through the inference gateway.
  4. Entity/relation memory only if phase 3 shows the need, as relational tables queried with recursive CTEs.
- **External memory systems** stay attachable over MCP through the gateway (ADR-0032) as ordinary tools, without the guarantees above.

## Consequences

- No new stateful service; memories are deleted with their project in the same transaction and covered by existing backups.
- Phases 1 and 2 need no embedding or rerank endpoints and no PostgreSQL extensions.
- Memory quality depends on agents writing few, precise memories; the playbook and the built-in skill must say what is worth remembering (not what can be read from the repository).
- Using embeddings in phase 3 makes pgvector a deployment prerequisite (`design/deployment.md`) and ties stored vectors to an embedding model (stored per row; a model change means re-embedding).

## Alternatives considered

- **Dedicated vector database (Qdrant, Chroma):** a second stateful service to run, back up and keep consistent with PostgreSQL; its advantages appear far beyond the scale of a single-tenant instance.
- **Vector-graph database (HelixDB):** young, and graph memory is the least proven part; relational tables cover it if needed, PostgreSQL 19's SQL/PGQ would be a bonus.
- **Memory only through MCP:** scope parameters are set by the agent, no recall before the first turn, no lifecycle with the project, no UI review or audit, no access to transcripts for extraction.
- **Embeddings and reranking from the start:** two more model endpoints for a small number of memories; small selection models may make them unnecessary.
- **Instance scope or persona memories across projects:** little benefit, and poisoned or confidential memories would cross project boundaries.
