# ADR-0053: Parallel chats, topics and archiving

- **Status:** Accepted (planned for iteration 3)
- **Date:** 2026-11-02
- **Related:** [ADR-0016](0016-runs-and-triggers.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md), [ADR-0044](0044-task-artifacts-and-cross-task-reading.md)

## Context

ADR-0026 and ADR-0043 allow one long-lived chat per (persona, project). Working on several topics in one project over weeks mixes them in one context. Summary-based compaction keeps the context small but loses detail, and it can't separate topics. A chat also never completes, so it never leaves the list.

## Decision

- **Several chats per (persona, project):** a human starts a chat by picking a persona of the instance and writing the first message, in the project shown in the chat UI. Each chat is a task of kind `chat` as in ADR-0043 (own key with the `-CHAT` suffix, read-only checkout of the default branch, no task branch and no PR). Agents don't start chats.
- **Topic:** the title of the chat task. The agent sets it in its first run with `set_task_title`, as it does for tasks; until then the UI shows the first message. A human can rename it like a task title.
- **Archiving instead of completion:** a human archives and unarchives a chat. Archiving follows marking a task done: it is rejected while the chat is `queued` or `running`, and it doesn't touch the chat's sub-tasks, which may still run. An archived chat is read-only (no new human messages, no runs) and hidden from the chat list; the archive lists it with its transcript and artifacts. Unarchiving returns it to the list unchanged.
- **Archived chat as parent:** a sub-task that ends still queues its message on the chat (ADR-0043), but the message neither starts a run nor unarchives the chat; it is delivered with the first run after unarchiving. A question escalated to an archived chat goes to a human, as for a completed parent (ADR-0043).
- **Own concurrency limit:** chat runs count against `runs.maxConcurrentChats` (instance-wide, default 2), not against `runs.maxConcurrent` (iteration 2, default 4). Chats are expected to be lighter than implementation tasks, and a chat should not wait behind long-running tasks while a human waits for a reply.
- **Compaction is visible:** when summary-based compaction replaces earlier messages, the transcript gets a notice (`context_compacted`) that expands to the summary.

## Consequences

- Each chat takes a task number, so project task numbers get more gaps than with one chat per persona (ADR-0043).
- The chat list shows persona, topic and status per chat; the escalation target of a task is still its parent chat (`parent_id`), now one of several.
- Archiving a chat whose sub-tasks still run is allowed; their results wait in the chat's queue until a human unarchives it.
- Two concurrency limits to configure instead of one.

## Alternatives considered

- **One chat per (persona, project), compaction only (ADR-0026):** topics can't be separated, and compaction loses the detail of earlier topics.
- **One active chat per (persona, project), earlier ones archived:** separates topics only one after another, not side by side.
- **Chats share `runs.maxConcurrent`:** a chat reply waits for a free slot behind implementation tasks.
