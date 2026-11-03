# ADR-0026: Chats per persona and project, task dependencies, project context

- **Status:** Accepted (planned for iteration 3, amended by ADR-0041, ADR-0043, ADR-0052, ADR-0053 and on 2026-11-03: project context)
- **Date:** 2026-10-16
- **Related:** [ADR-0016](0016-runs-and-triggers.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0041](0041-tasks-based-on-another-tasks-branch.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md), [ADR-0052](0052-dependencies-gate-the-first-start.md), [ADR-0053](0053-parallel-chats-topics-and-archiving.md), [ADR-0040](0040-agent-memory-built-in-on-postgresql.md)

## Context

The typical workflow is an ongoing chat with the CEO persona per project, in which the CEO creates tasks for other agents — e.g. an implementation task and a review task that depends on it. Paperclip offers only one chat per agent.

## Decision

- One chat per (persona, project): a long-lived conversation scoped to a project. A chat is a task of kind `chat` (ADR-0043). Amended by ADR-0053: several chats per (persona, project), each with a topic, archived instead of completed.
- Task dependencies: `depends_on` relations. When a task completes (`complete_task`), dependent tasks in `awaiting_dependencies` start automatically. Dependencies gate only the first start; later rounds go through the parent (ADR-0052).
- Project context: a Dark Factory–side project document injected into every run of the project; agents with `project:context:write` can update it, e.g. by extracting context from a chat.
  Amended 2026-11-03:
  - The project context holds planning knowledge and is injected into chat runs only, never into task runs or the workspace. Implementation conventions belong in the repository's `AGENTS.md`, and a task gets what it needs in its prompt from the chat that creates it.
  - Agents only propose changes, through an approval request of type `project-context` with the full new text. A human with `project:context:write` approves it, and the approval fails if the context changed since the version the proposing run received. Reason: text injected into every chat run is a persistent prompt-injection channel. This matches ADR-0040, where the project context stays human-approved instructions.

## Consequences

- Chats need context compaction earlier than tasks (F3 in `product/context.md`).
- Generalizes the "child completes → wake parent" trigger (ADR-0016).

## Alternatives considered

- **Project context in every run, written directly by agents with a grant** (the original decision): one prompt-injected run could steer every later run in the project, and tasks would get planning text they don't need.
- **One chat per agent (Paperclip)**: the project context has to be re-established constantly.
