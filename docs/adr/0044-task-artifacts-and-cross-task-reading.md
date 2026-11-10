# ADR-0044: Task artifacts and cross-task reading

- **Status:** Accepted (planned for iteration 3; amended by [ADR-0048](0048-image-artifacts.md): image artifacts, and on 2026-11-03: tool details, UI)
- **Date:** 2026-10-27
- **Related:** [ADR-0012](0012-postgresql-and-blob-store.md), [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0041](0041-tasks-based-on-another-tasks-branch.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md), [ADR-0057](0057-factory-activity-in-the-transcript.md)

## Context

Artifacts (Markdown documents an agent writes for the human or as intermediate results, `product/context.md`) were planned for iteration 5 as a UI feature. With delegation (ADR-0043) they become the way results travel between tasks: a research agent summarizes its findings, the parent reads them. Research on a repository doesn't need a commit or a PR; a document is enough. A child's `complete_task` summary is too short for that, and the child's instructions already travel as the first message (the task description).

## Decision

- **Artifact:** a named Markdown document that belongs to one task, chat tasks included. Names are unique per task; writing an existing name replaces the content (no history). Stored in PostgreSQL (ADR-0012), text only, with size and count limits per task (instance defaults).
- **Factory tools:**
  - `write_artifact(name, markdown)`: only on the agent's own task.
  - `list_artifacts(task?)` and `read_artifact(task?, name)`: own task by default, other tasks as below.
- **Who can read what:** an agent reads artifacts of its own task, its ancestors (including the chat), its descendants, the tasks it depends on (`depends_on`) and its `base_task` (ADR-0041). Everything else is refused. Reads of other tasks are audited. Nobody writes to another task's artifacts.
- **Untrusted content:** an artifact is agent-written text. The tool result marks it as data from another task, and the playbook tells agents to treat it as input, not as instructions.
- **Hand-over:** the message that wakes a parent on a child's end (ADR-0043) lists the child's artifact names. A coordinator's instructions to a child stay the task description, delivered as the first message.
- **Humans:** read artifacts in the task page and in the chat UI as rendered Markdown (the renderer of ADR-0004). Cards with previews, types and times are iteration 6.
  Amended 2026-11-03:
  - Names are 1–100 characters from `[A-Za-z0-9._-]` without a leading dot, so they work in URLs and Markdown image references unescaped. A write may replace an artifact of a different type. There is no delete; artifacts live as long as their task.
  - Tools name tasks by key. `list_artifacts` lists one task. `read_artifact` returns text in pages of at most 64 KiB with an `offset`, because harnesses truncate large tool results (Claude Code: about 25k tokens). An unrelated task and a missing task give the same error, so keys can't be probed. Cross-task reads are recorded in the existing tool-call audit entry with the target task and artifact name.
  - Limits live in the instance config: 1 MiB per text artifact, 50 artifacts per task.
  - Humans get artifact cards, a resizable split view next to the transcript and a bookmarkable page per artifact in iteration 3, on the task page and in the chat UI.

## Consequences

- Research, plans and findings don't have to be committed to the repository.
- A chat can leave a brief for the tasks it creates; they find it as an ancestor artifact.
- Artifacts are not versioned; a replaced artifact is gone. Add history only if it proves necessary.
- Binary artifacts, images and cross-project reading are out of scope.

## Alternatives considered

- **Read access to every task of the project:** simpler, but any task could pull in unrelated, possibly injected content.
- **Results only in the completion summary:** too short and not addressable by name.
- **Committing research documents to a branch:** needs a PR for every finding and a base branch the reader can fetch.
