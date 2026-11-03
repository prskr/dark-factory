# Design

## Context

See proposal.md (Why). The current state this change builds on:

- **Tasks** (`crates/df-db/src/tasks.rs`):
  - Keys come from `projects.next_task_number` under a row lock (`allocate_task_key`).
  - `tasks_key_check` accepts only `^[A-Z][A-Z0-9]{1,9}-[1-9][0-9]*$`.
  - There is no kind column.
  - The first message is a system prompt (`task.assigned`) built by `format_assignment_prompt`, with a hidden `set_task_title` block when no human title was given.
  - `TitleSource` (Placeholder < Agent < Generated < User) already gives a human rename precedence.
- **Runs** (`crates/df-agent-channel`):
  - `start_queued_runs` picks `queued` tasks. With `iter2-run-resilience`, it caps active runs at `runs.maxConcurrent` under an instance-wide advisory lock.
  - A run waiting after a turn stays alive for `runs.keepAliveSeconds`. The session then sends `Release`, and the task stays `waiting`.
  - `EndTurn` maps the task to `waiting`.
- **Assign** (`assign.rs::build_assign`):
  - The persona's system prompt goes into `Harness.system_prompt`, which the runtime writes to `~/.claude/CLAUDE.md`.
  - The workspace carries the repo URL through the proxy, the base, task and WIP branches.
  - `Assign.tools` is the full static list from `factory-tools.v2.json`.
- **Runtime** (`crates/factory-agent/src/workspace.rs`):
  - checks out the task branch from `origin/<task>` or `origin/<base>`;
  - restores WIP;
  - pushes new task branches, commits and WIP snapshots.
- **Git proxy** (`crates/df-gateway/src/git/refs.rs`): `RefScope` allows the task branch (create or fast-forward) and the WIP branch. The scope is built per run from `GitRoute` (`control/snapshot.rs`).
- **Delegation** (`iter3-task-dependencies`): `parent_id`, the principal `created_by`, the tools `create_task`, `list_personas` and `send_message`, and the child-ended message queued in `record_task_event`.
- **Web** (`crates/df-web`):
  - `task_pages.rs` serves `/tasks/...`;
  - `layout.rs` holds `Nav` and `workspace_layout`;
  - `components/shell.rs` holds the rail and the bottom bar;
  - Alpine components live in `assets/<name>.js`.

## Goals / Non-Goals

**Goals:**
- A chat reuses the task machinery for messages, runs, transcript, artifacts, approvals and wake-ups. Chat-specific behavior is a small number of checks on `tasks.kind`.
- Read-only is enforced by the Git proxy. The runtime's read-only mode only avoids pointless pushes.
- A queued chat never waits behind task runs, and only briefly behind idle chats.

**Non-Goals:**
- A separate chat entity or tables: rejected (ADR-0043).
- Per-board-member read state: rejected (proposal).
- Changes to the summary itself: `iter2-continuation-summary`.

## Decisions

### D1: Schema

One migration:

- **`tasks.kind`**: `TEXT NOT NULL DEFAULT 'task' CHECK (kind IN ('task','chat'))`.
- **`tasks.archived_at`** (`TIMESTAMPTZ NULL`) and **`tasks.archived_by`** (`JSONB NULL`, a human principal), plus a `CHECK` that both are null for `kind = 'task'`, and that `archived_by` is set exactly when `archived_at` is.
- **`tasks_key_check`**: becomes `^[A-Z][A-Z0-9]{1,9}-[1-9][0-9]*(-CHAT)?$`, plus a `CHECK` that the suffix is present exactly when `kind = 'chat'`.
- **Index**: `(project_id, kind, archived_at)` for the chat list.

Alternative: an `archived` status. Unarchiving would then need the previous status, and the status would stop describing the run (decided in the grilling, Q3).

### D2: Creating a chat

`df_db::chats::create_chat(tx, project, persona, author, text)`:
- allocates the number with `allocate_task_key`;
- inserts a task with kind `chat`, key `<KEY>-<n>-CHAT`, no parent, `created_by` = the board member, and a placeholder title derived from the text (source `placeholder`);
- queues the text as an ordinary human message with `queue_message_tx`, plus the hidden title block from `ask_for_title()`;
- writes the `chat.create` audit entry.

`format_assignment_prompt` isn't used, so the transcript starts with the board member's own words.

`TaskKey` gets a `chat: bool` flag. Parsing and `Display` handle the suffix, and branch name helpers return `None` for chats.

### D3: Chat runs in `build_assign`

For `kind = 'chat'`:

- **System prompt**: `Harness.system_prompt` = persona prompt + `"\n\n"` + `CHAT_INSTRUCTIONS`, a `const &str` in `df-agent-channel`. It covers:
  - you talk with a board member;
  - the checkout is read-only and reset every run;
  - work that needs a branch or a PR goes into tasks via `create_task`;
  - follow up with `send_message`;
  - a chat is never completed.

  Alternative: a hidden block in the first message. That block is lost when a new session is seeded with a summary (decided, Q17).
- **Workspace**: `Workspace` gets `bool read_only = 8` (additive in `darkfactory.agent.v1`). `task_branch` and `wip_branch` stay empty.
- **Tools**: `tool_descriptors()` is filtered by a `const CHAT_TOOLS: &[&str]`. The dispatch in `tools.rs` rejects any other tool for a chat run with a tool error (`not available in a chat`), so a harness that ignores the list still can't call it.

### D4: Read-only enforcement

- **Gateway**:
  - `GitRoute` gets `bool read_only` (additive in `darkfactory.gateway.v1`), and the snapshot sets it for chat runs.
  - `RefScope::rejection()` returns `df:read-only` for every command when the flag is set.
  - Fetch and `ls-refs` are unaffected.
- **Runtime**: `prepare` with `read_only`:
  - clones if needed;
  - fetches the base branch;
  - runs `git checkout --detach origin/<base>`, `git reset --hard` and `git clean -fdx`;
  - skips the WIP restore.

  Finalize and the per-turn push skip all pushes, and `WorkspaceState` reports empty SHAs.

  `git clean` is allowed here, unlike for task workspaces: a chat workspace holds nothing worth keeping.
- **Precedence**: the gateway is the security boundary; the runtime only avoids pointless rejected pushes (Goal).

### D5: Scheduler with two limits

`start_queued_runs` keeps one transaction and the advisory lock:

- **Free slots**: computes `free_tasks = maxConcurrent - active(kind='task')` and `free_chats = maxConcurrentChats - active(kind='chat')`.
- **Picking**: selects queued candidates with `kind`, oldest first, skipping archived chats (`archived_at IS NULL`). It takes them while the matching budget lasts.
- **No free chat slot**:
  - If queued chats remain, it picks the chat run in `awaiting_input` with the oldest `last_turn_ended_at` and publishes a `ReleaseIdle { run_id }` notification.
  - The session owning that run handles it like an expired keep-alive deadline: it sends `Release`, and the task keeps its status.
  - The freed slot is used in the next scheduling pass, triggered by the run ending.

Alternative: a shorter keep-alive for chats. That loses warm sandboxes even when nobody waits (decided, Q19).

`runs.maxConcurrentChats` goes into `InstanceConfig` next to `runs.maxConcurrent`: default 2, 0 rejected.

### D6: Kind checks at the edges

- **`mark_task_done` and cancel**: reject `kind = 'chat'` with `Conflict` before any write.
- **Archive and unarchive** (`df_db::chats`):
  - Archive locks the task row. It rejects a chat that is `queued` or `running`, or has an `agent_requests` row in state `pending`.
  - Archive then sets `archived_at`/`archived_by` and writes the audit entry.
  - Unarchive clears both, sets the status to `queued` if undelivered messages exist (with the same `CASE` as `queue_message_tx`), and writes the audit entry.
- **Messages**:
  - `queue_message` from the web rejects an archived chat.
  - The child-ended message (`record_task_event`) is queued regardless. The scheduler's `archived_at IS NULL` filter keeps it from starting a run.
- **Volume retention**: treats `archived_at` as the close time of a chat.

### D7: Compaction notice

Only the runtime knows whether `session/load` succeeded or a new session was seeded with `Continuation.summary`. When it seeds a new session with a non-empty summary, it emits a notice (the existing transcript event for runtime reports) with code `context_compacted`, level `info`, and the summary as its message, before the first prompt. No contract change: notices are already free-form code and message. The existing notice component renders it with the bot icon, expandable.

### D8: Web UI

- **Routes**: a new module `chat_pages.rs` with:
  - `/chats` (GET list, POST create);
  - `/chats/compose`;
  - `/chats/{key}`;
  - `/chats/{key}/events`, `/messages`, `/title`, `/stop`, `/archive`, `/unarchive`;
  - `/chats/archive` (modal content, with `?q=` search).

  Handlers reuse the task-page transcript, message box and SSE plumbing with `kind` checks. Task routes return 404 for chat keys and vice versa.
- **Layout**: `workspace_layout` gets a chat `View`.
- **Alpine components**:
  - `chatLayout.js`: collapse state of both columns (in `localStorage`, wrapped in `try`/`catch`); the open artifact and the split divider width live in `artifactSplit.js` from `iter3-artifacts`;
  - `archiveModal.js`: selection and tabs.

  Both are added to `COMPONENT_SCRIPTS`.
- **"Needs you"**: an `EXISTS` over pending `agent_requests` per chat, in the chat list query.
- **Ordering**: needs-you first, then `last_activity_at` = the latest transcript event or message.
- **Search** in the archive: `ILIKE` over the title, the persona name and transcript message text, limited to the project's archived chats. That's sufficient for the expected volume; full-text search arrives in iteration 6.
- **Shell**: `Nav::Chats`, first in the rail after the create action and first in the bottom bar. The floating button on the chat list says "New chat".
- **Task list and inbox**: the task list query filters `kind = 'task'`. A parent of kind `chat` and an inbox row of a chat link to `/chats/{key}`.

### D9: Fake agent and e2e

The fake ACP agent scripts `create_task` and `send_message` (from `iter3-task-dependencies`). The e2e:
1. starts a chat;
2. sees the topic set;
3. the chat creates two tasks;
4. archives the chat while a task runs;
5. the task completes;
6. unarchives the chat and sees the child-ended message delivered;
7. checks that a push from the chat sandbox is rejected.

## Risks / Trade-offs

- [Releasing an idle chat run drops its warm sandbox] → The next message starts a new run, which resumes the harness session from the volume. The cost is startup time, not context.
- [Two limits can be misconfigured so that chats and tasks together exceed cluster capacity] → Both limits are documented together in the configuration reference, and quota rejections already re-queue (`iter2-run-resilience`).
- [`git clean -fdx` in a chat workspace deletes files the agent wrote in an earlier run] → Intended: chat results belong in artifacts or tasks. The chat instructions say so.
- [`ILIKE` search over transcripts gets slow with many archived chats] → It's limited to one project's archived chats. Full-text search is planned for iteration 6.
- [The archived-chat check is only in the scheduler query] → The only path that starts runs. A test covers a queued message on an archived chat.

## Migration Plan

- One forward migration (D1). Existing tasks get `kind = 'task'`, and the key check stays valid for them.
- Regenerate `.sqlx/`.
- Contract changes are additive (`Workspace.read_only`, `GitRoute.read_only`).
- Rollback: pre-MVP, forward-only.
