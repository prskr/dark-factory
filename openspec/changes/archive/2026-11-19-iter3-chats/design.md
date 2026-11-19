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
- **Delegation** (`iter3-task-dependencies`): `parent_id`, the principal `created_by`, the tools `create_task`, `list_personas` and `send_message`, and the child-ended message queued by the parent's `ChildEnded` transition.
- **Task transitions** ([ADR-0055](../../../docs/adr/0055-task-transitions-through-one-transition-function.md), from `iter3-task-dependencies` D2): every status change goes through the pure `df_core::task_transition::transition(&TaskSnapshot, TaskEvent)` and the applier `df_db::apply_task_event`.
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
- **Branch**: a chat stores an empty `branch`. The unique index `tasks_open_branch` skips `branch = ''`, so chats never hold a branch.
- **`runs.idle_since`** (`TIMESTAMPTZ NULL`): set when the run enters `awaiting_input` (it waits in its keep-alive window), cleared when it leaves that state. It orders the idle chat runs for D5.
- **`task_last_activity(task_id, created_at)`**: SQL function for the last activity of a task (latest transcript event or message, else creation), used by the chat list ordering and its queries.

Alternative: an `archived` status. Unarchiving would then need the previous status, and the status would stop describing the run (decided in the grilling, Q3).

### D2: Creating a chat

`df_db::chats::create_chat(tx, project, persona, author, text)`:
- allocates the number with `allocate_task_key`;
- inserts a task with kind `chat`, key `<KEY>-<n>-CHAT`, an empty branch, no parent, `created_by` = the board member, and a placeholder title derived from the text (source `placeholder`);
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
- **Workspace**: `Workspace` gets `bool read_only = 11` (additive in `darkfactory.agent.v1`; lower numbers are taken). `task_branch` and `wip_branch` stay empty.
- **Tools**: `tool_descriptors()` is filtered by a `const CHAT_TOOLS: &[&str]`: the six tools of the `chats` spec plus `write_artifact`, `list_artifacts` and `read_artifact`, which the Artifacts tab and split view need. The dispatch in `tools.rs` rejects any other tool for a chat run with a tool error (`not available in a chat`), so a harness that ignores the list still can't call it.

### D4: Read-only enforcement

- **Gateway**:
  - `GitRoute` gets `bool read_only = 9` (additive in `darkfactory.gateway.v1`), and the snapshot sets it for chat runs.
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

`df_db::runs::start_queued_runs`, called by the scheduler in `df-orchestrator`, keeps one transaction and the advisory lock:

- **Free slots**: computes `free_tasks = maxConcurrent - active(kind='task')` and `free_chats = maxConcurrentChats - active(kind='chat')`.
- **Picking**: selects queued candidates with `kind`, oldest first, skipping archived chats (`archived_at IS NULL`). It takes them while the matching budget lasts.
- **No free chat slot**:
  - If queued chats remain (the same due-task query as the picking), it picks the chat run in `awaiting_input` with the oldest `runs.idle_since` and publishes a `ReleaseIdle { run_id }` notification on the channel `release_idle`.
  - The session owning that run handles it like an expired keep-alive deadline: it sends `Release`, and the task keeps its status.
  - The freed slot is used in the next scheduling pass, triggered by the run ending.

Alternative: a shorter keep-alive for chats. That loses warm sandboxes even when nobody waits (decided, Q19).

`runs.maxConcurrentChats` goes into `InstanceConfig` next to `runs.maxConcurrent`: default 2, 0 rejected.

### D6: Kind checks at the edges

`TaskSnapshot` gains `kind` and `archived`. The chat rules are arms of `transition`, so they are unit tests in `df-core`:

- **`MarkedDone` and `Cancelled`**: rejected for `kind = chat` (`Rejected` → `Conflict`), so nothing is written or audited.
- **Archive and unarchive** (`df_db::chats`) raise two new events through `apply_task_event`:
  - `Archived { by }`:
    - rejected while the chat is `queued` or `running`, or has an `agent_requests` row in state `pending` (snapshot field `pending_requests`, read from the chat's own rows);
    - otherwise the status is unchanged, with the effects `SetArchived(Some(by))` and `Audit(chat.archive)`;
    - the same transaction publishes `ReleaseIdle` for the chat's run in `awaiting_input`, so a warm sandbox doesn't keep a chat slot or receive messages while the chat is archived.
  - The rule is one function, `archive_rejection`, which the web uses for the disabled Archive action and its reason.
  - `Unarchived`:
    - effects `SetArchived(None)` and `Audit(chat.unarchive)`;
    - the status follows the `MessageQueued` rule when undelivered messages exist, otherwise it is unchanged.
  - Both are rejected for `kind = task`.
- **Messages**:
  - `queue_message` from the web rejects an archived chat. That is a check at the web edge, not in `transition`, because the child-ended message must still be queued.
  - The child-ended message is queued regardless: the `ChildEnded` arm treats an archived chat like any open parent, so the chat becomes `queued` (`running` while its released run still ends). The scheduler's `archived_at IS NULL` filter keeps it from starting a run.
  - A live run never receives messages of an archived chat: `claim_held_back_messages` skips archived chats, and `Finalize(Idle)` of an archived chat's run ignores undelivered messages (the flag is read under the task row lock, after the run row lock). The messages wait for the unarchive.
- **Base task**: `create_task` rejects a chat as `base_task` (`InvalidBaseTask`); a chat has no branch to start from.
- **Volume retention**: treats `archived_at` as the close time of a chat.

### D7: Compaction notice

Only the runtime knows whether `session/load` succeeded or a new session was seeded with `Continuation.summary`. When it seeds a new session with a non-empty summary, it emits a notice (the existing transcript event for runtime reports) with code `context_compacted`, level `info`, and the summary as its message, before the first prompt. No contract change: notices are already free-form code and message. The existing notice component renders it with the bot icon, expandable.

### D8: Web UI

- **Routes**: a new module `chat_pages.rs` with:
  - `/chats` (GET list, POST create);
  - `/chats/compose`;
  - `/chats/{key}`;
  - `/chats/{key}/events`, `/messages`, `/title`, `/stop`, `/archive`, `/unarchive`;
  - `/chats/archive` (modal content, with `?q=` search);
  - `/chats/events` (SSE stream of the chat list);
  - `/chats/{key}/interrupt` and `/chats/{key}/messages/{id}` (PATCH, DELETE: edit or withdraw a queued message), as on the task page.

  `/chats/{key}` also serves an archived chat, read-only, with Unarchive in place of the message box.

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
