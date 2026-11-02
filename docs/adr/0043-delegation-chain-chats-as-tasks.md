# ADR-0043: Delegation chain, chats as tasks and waiting on subtasks

- **Status:** Accepted (planned for iteration 3)
- **Date:** 2026-10-27
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0036](0036-code-review-stays-in-the-forge.md), [ADR-0038](0038-agent-questions-and-approval-requests.md)

## Context

Dark Factory is meant to work like a small company: a human owner, a CEO persona that receives requests in a chat, and specialists (implementation, QA, research) that get work delegated. Two behaviours are expected from agents, as in Claude Code sub-agents:

- **Delegation:** an agent hands part of its work to another persona (a coder asks a researcher).
- **Escalation:** an agent that can't decide passes the question up (coder → CEO → owner).

The existing ADRs cover the parts but not the whole: ADR-0034 defines the task tree for branches and rollups, ADR-0038 allows questions to "the parent or creating task", ADR-0026 defines chats without saying what a chat is in the data model, and `tasks.created_by` can only reference a human. Without one answer to "who does a task report to", escalation has no target.

## Decision

- **One pointer:** a task's `parent_id` is its decomposition parent, the task it reports to, and the target of its escalations. A human creating a task from the UI sets no parent or picks one; an agent creating a task (`task:create`) becomes its parent. `created_by` becomes a principal (human user or task).
- **Chats are tasks:** a chat (ADR-0026) is a task of kind `chat`, one per (persona, project), long-lived. Messages, runs, wake-ups and the transcript use the normal task machinery. A chat task has no task branch and no PR, but a read-only checkout of the project's default branch so the persona can read the code; the Git proxy accepts no push from it (it owns no task branch). Its key is the next project number plus a suffix (`DF-12-CHAT`), and it has its own chat UI instead of appearing in the task list: a chat can write artifacts (ADR-0044), anything that needs a branch, a PR or other task features belongs in a regular task. Tasks created in a chat have the chat as parent. A chat never completes, doesn't block on open subtasks, and cancelling it doesn't cancel the tasks it created.
- **No group tasks:** every task has a persona (ADR-0034 amended). Grouping work for a feature means a task with a coordinating agent.
- **Delegation:** the agent creates a task for another persona and ends its turn. The parent is not blocked: it wakes on **any** child ending (completed, failed, cancelled) through a queued message carrying the child's summary (ADR-0016). It decides whether anything is left to do; a wake-up with nothing to do costs a few tokens. The existing `blocked_by` relation stays the only way to hold a task back before its start (ADR-0026).
- **Escalation is a question to the parent:** `ask_question` can be addressed to the parent task's agent or to a human (ADR-0038 amended). The parent answers with `answer_question`. If it can't decide, it asks its own parent or a human and relays the answer. There is no forward primitive. If the parent is completed or cancelled, the question goes to a human.
- **Approvals stay with humans** (ADR-0038, ADR-0011). An agent can only pass the need on by asking a question.
- **Choosing a persona:** personas are global to the instance (one Dark Factory instance is one company working on several projects). A persona gets a `description` (role, strengths) and agents list personas with a factory tool. A new task always belongs to the creating task's project.
- **Between siblings:** implementer and reviewer talk through the forge (ADR-0036); there is no direct task-to-task messaging beyond parent and child.
- **No org tree:** no `reports_to` between personas. The parent chain exists per task at run time. A static org chart (default escalation, delegation allowlists, who may hire whom) can be added later with the full permission model (ADR-0011); it needs only additive columns.
- **Naming:** "escalation" in this context means passing a question up. ADR-0011's "no privilege escalation" keeps its meaning (an agent can't grant itself more permissions).
- **Limits:** depth and children per task (ADR-0034) bound delegation loops; the instance concurrency limit (iteration 2) bounds parallel runs.

## Consequences

- Escalation and delegation are the same chain: task → parent → … → chat → human. No separate org model to keep consistent.
- Chats need a task row and a key. They share the project's task counter, so task numbers have gaps (`DF-12-CHAT`, next task `DF-13`); the task key pattern accepts the `-CHAT` suffix. The task list filters on kind; the chat UI is separate and chat-shaped (message stream, no tabs for PR or sub-tasks).
- A delegation costs a sandbox start and a clone for the child; accepted (ADR-0033 makes the same trade-off for fan-out).
- `tasks.created_by` and `agent_requests.decided_by` change from user references to principals in iteration 3; `agent_requests` gets an addressee. Both are additive migrations on top of iteration 2.
- Waking on any child completion makes parents robust against partial results; the playbook tells coordinators to check the state of all children before continuing.

## Alternatives considered

- **Org tree with `reports_to` between personas:** static, duplicates the per-task chain, and the same persona reports to different parents in different tasks.
- **Chat as a separate entity:** needs its own message queue, wake-ups and escalation target next to the task machinery.
- **Wake only when all children are done:** needs a wait-set model, and a failed or slow child blocks the others' results.
- **A forward tool for questions:** adds a primitive; asking the parent's own question is enough for now.
- **Direct messaging between sibling tasks:** another channel next to the forge; revisit if forge comments prove too slow.
