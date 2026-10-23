# ADR-0026: Chats per persona and project, task dependencies, project context

- **Status:** Accepted (planned for iteration 3, amended by ADR-0041)
- **Date:** 2026-10-16
- **Related:** [ADR-0016](0016-runs-and-triggers.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0041](0041-tasks-based-on-another-tasks-branch.md)

## Context

The typical workflow is an ongoing chat with the CEO persona per project, in which the CEO creates tasks for other agents — e.g. an implementation task and a review task blocked by it. Paperclip offers only one chat per agent.

## Decision

- One chat per (persona, project): a long-lived conversation scoped to a project.
- Task dependencies: `blocked_by` relations. When a task completes (`complete_task`), dependent tasks become runnable and start automatically.
- Project context: a Dark Factory–side project document injected into every run of the project; agents with `project:context:write` can update it, e.g. by extracting context from a chat.

## Consequences

- Chats need context compaction earlier than tasks (F3 in `product/context.md`).
- Generalizes the "child completes → wake parent" trigger (ADR-0016).

## Alternatives considered

- **One chat per agent (Paperclip)**: the project context has to be re-established constantly.
