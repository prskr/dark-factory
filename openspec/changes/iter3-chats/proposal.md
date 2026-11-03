# Proposal

## Why

`iter3-task-dependencies` lets agents create and coordinate tasks, but a human can only start that work by writing a task. Iteration 3 needs a conversation with a persona per project as the entry point and the root of the delegation chain ([ADR-0043](../../../docs/adr/0043-delegation-chain-chats-as-tasks.md)). Several topics in one project must stay apart, so a persona can have several chats, each with a topic, and a finished chat is archived instead of completed ([ADR-0053](../../../docs/adr/0053-parallel-chats-topics-and-archiving.md)).

## What Changes

- **Chats as tasks**:
  - A chat is a task of kind `chat`.
  - Its key is `<PROJECT>-<n>-CHAT`, with `n` taken from the project's task counter.
  - A board member starts a chat in a project by choosing a persona and writing the first message. That message is stored as an ordinary message from the board member, with the hidden `set_task_title` instruction attached.
  - A persona can have several chats in a project. Agents don't start chats.
- **Topic**: the chat's task title. The agent sets it in the first run; a board member can rename it.
- **Chat runs**:
  - The persona's system prompt is followed by fixed chat instructions.
  - Tools: `set_task_title`, `report_progress`, `create_task`, `list_personas`, `send_message` and `request_approval`. A chat run gets no `complete_task`, `ask_question`, `update_pull_request` or `comment_on_issue`.
  - Checkout:
    - A read-only checkout of the project's base branch, reset to `origin/<base>` at the start of every run.
    - There is no task or WIP branch, and the Git proxy rejects every push from a chat run.
  - Egress rules and volume retention apply as for tasks; archiving counts as closing.
- **Actions on a chat**: Stop and Archive. Cancel and Mark done are not offered for chats and are rejected.
- **Archiving**:
  - `archived_at` and `archived_by` are attributes; the status is unchanged.
  - Archiving is rejected while the chat is `queued` or `running`, or has an open approval request.
  - Subtasks are not touched. A subtask that ends still queues its message on an archived chat, but no run starts.
  - Unarchiving starts a run when messages are waiting.
  - An archived chat takes no human messages.
- **Concurrency**:
  - Chat runs count against the new `runs.maxConcurrentChats` (default 2), not `runs.maxConcurrent`.
  - When a chat is queued and no chat slot is free, the chat run that has been idle the longest (in its keep-alive window) is released.
- **Compaction notice**:
  - A run that seeds a new harness session with the transcript summary writes a `context_compacted` notice. This applies to tasks too.
  - The notice expands to the summary.
- **Audit**: `chat.create`, `chat.archive` and `chat.unarchive`; renames stay `task.rename`.
- **Web UI** (Penpot page "Chat"):
  - **Pages**: `/chats?project=<KEY>` and `/chats/<KEY>`. Without `project`, the page opens the project of the most recently active chat.
  - **Navigation**: Chats in the rail and as the first entry of the phone bottom bar.
  - **Left column** (collapsible):
    - the projects, then the project's chats;
    - each chat shows persona, status, topic and last message;
    - chats that need you come first, then the rest by last activity;
    - "needs you" means an open approval request;
    - idle chats show no badge;
    - the column ends with an "Archived chats" entry.
  - **Center**: a header with only "+ New chat", the transcript and the message box.
  - **Details column** (Overview, Subtasks and Artifacts tabs):
    - Overview shows the agent with its description, Now, Status with Archive, Topic, Project, Persona, Key, Checkout and the runs.
    - Subtasks shows the direct subtasks.
    - Artifacts shows artifact cards.
  - **Artifacts**: a Markdown artifact opens in a resizable split view next to the transcript.
  - **Dialogs**:
    - The new chat dialog has a fixed project and a persona picker with descriptions.
    - The archive modal holds a searchable list, then a sidebar with Transcript and Artifacts tabs, the opened artifact, and Unarchive.
  - **Other screens**:
    - phone layouts for all of the above;
    - an empty state for a project without chats.
- **Tasks and inbox**:
  - The task list shows no chats.
  - A task whose parent is a chat links to the chat page.
  - Approval requests from chats link to the chat page.
- **Docs**:
  - ADR-0053, the glossary terms Chat, Topic and Archived chat, `iteration-3.md` and `roadmap.md` (written);
  - the configuration reference for `runs.maxConcurrentChats`;
  - a how-to "Plan work in a chat".

Out of scope:
- **Project context** (settings tab, injection into the chat prompt, proposals through `request_approval`, the Overview link, the archive exemption for `project-context` requests): `iter3-project-context`, which follows this change.
- **Questions escalated to a chat**, and "archived chat → human": `iter3-parent-questions`.
- **Stored, incremental transcript summary**: `iter2-continuation-summary`. This change adds only the notice.
- **Model-generated compaction** for chats (F3): backlog ("Context compaction plugins").
- **Artifact cards, rendered view and split view**: built in `iter3-artifacts`, which also adds them and a collapsible task list to the task page; this change reuses them.
- **Unread state per board member**: rejected. "Needs you" is derived from open requests.
- **Deleting chats**: rejected. Transcripts, artifacts and subtasks' parents stay intact.
- **Chats started by agents**: rejected (ADR-0053).

## Capabilities

### New Capabilities
- `chats`: chats as tasks of kind `chat`. Covers starting a chat, keys, topic, chat runs (instructions, tools, read-only checkout), Stop and Archive, archiving and unarchiving, archived chats as parents, the chat concurrency limit and audit.
- `chat-ui`: the chat pages. Covers the project and chat list, ordering and "needs you", the transcript, the details tabs, the artifact split view, the new chat dialog, the archive modal, the empty state and the phone layouts.

### Modified Capabilities
- `task-management`: chat keys use the project counter, and chats can't be cancelled or marked done.
- `run-orchestration`:
  - `Assign` for chat runs;
  - chat runs don't count against `runs.maxConcurrent`;
  - the `context_compacted` notice.
- `git-proxy`: no ref update from a chat run is forwarded.
- `workspace-lifecycle`: a chat run prepares a read-only checkout of the base branch and never pushes.
- `task-ui`: the task list excludes chats, and a parent chat links to the chat page.
- `request-inbox`: requests from chats link to the chat page.
- `web-ui-shell`: Chats in the rail and the bottom bar.

## Impact

- `crates/df-db`:
  - migration:
    - `tasks.kind` (`task` | `chat`);
    - `tasks.archived_at` and `tasks.archived_by`;
    - the key check accepts the `-CHAT` suffix;
  - chat creation, archiving and unarchiving, and the chat-aware scheduler query;
  - `.sqlx/`.
- `crates/df-core`: the task kind, the chat key, and the config `runs.maxConcurrentChats`.
- `crates/df-agent-channel`:
  - `build_assign` for chats (instructions, tool subset, no branches);
  - rejection of `complete_task` and `ask_question` in chats;
  - the scheduler's chat slots and release of idle chat runs;
  - the `context_compacted` notice.
- `crates/df-gateway`: the read-only ref scope for chat runs.
- `crates/factory-agent`: the read-only prepare (reset to base, no task-branch or WIP pushes).
- `crates/df-web`:
  - the chat pages and their Alpine components (collapsing, split divider);
  - `Nav::Chats` in the rail and the bottom bar;
  - task list filter, parent link and inbox links.
- `crates/fake-acp-agent`, `crates/df-e2e`: a chat that creates two tasks, archiving with a running subtask, then unarchiving.
- Depends on:
  - `iter3-task-dependencies` (`parent_id`, `create_task`, `send_message`, the child-ended message, persona descriptions);
  - `iter2-continuation-summary` (the summary the notice shows).
- Coordinates with `iter3-task-tree`: the Subtasks tab uses its "direct subtasks". Chats have no `complete_task` and can't be cancelled, so the completion gate and subtree cancel never apply to them.
