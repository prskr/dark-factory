# Proposal

## Why

With delegation, most rows of the task list are subtasks: they report to a parent task and rarely need a board member. The flat list buries the few tasks that wait for a human, and every row carries a coloured status badge, even when the task just runs. Board members cannot see at a glance which work needs them.

## What Changes

- **The task list shows top-level tasks only** (with the chips Active, Done, All). Subtasks no longer appear as their own rows there. The link-only filters (`queued`, `running`, `waiting`), which the project counters use, keep listing every matching task as flat rows.
- **A top-level task with subtasks gets a disclosure control.** Opening it lists its direct subtasks below it, indented one level. Which rows are open is part of the URL (`open`), so it survives live updates and reloads.
- **Deeper levels open as their own list.** A subtask with subtasks of its own leads to a list scoped to it (`parent`): that subtask on top, its direct subtasks below, and a breadcrumb up to the top-level task. The list never indents more than one level.
- **Leaner status cues on rows.** `running` shows no badge. `completed` shows no badge and a struck-through title. A task that needs you shows the needs-you dot in place of its badge. All other statuses keep their badge.
- **Needs you rolls up.** A task shows the dot when it or any task below it needs you; if only tasks below it do, its status line reads "N subtasks need you". An `in_review` subtask does not need you when its project has `subtask_merge: parent-agent` and its base task is its parent (its parent task's agent merges it).
- **BREAKING (UI):** subtasks leave the default list; bookmarks of `/tasks` show fewer rows. Pre-MVP, no compatibility needed.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `task-ui`: "Task list" (status cues instead of a badge per row), "Subtask progress on task rows" (subtasks no longer ordinary rows of the list); new requirements for top-level tasks, opening a task's subtasks, the list scoped to a subtask, status cues and the needs-you dot.

## Impact

- `df-core` (`domain.rs`): the statuses that need you (`waiting`, `blocked`, `in_review`) as a constant next to `SUBTASK_DONE`.
- `df-db` (`task_views.rs`): `list_task_items` gains a scope (top-level tasks, or the direct subtasks of one task) and returns, per row, the count of direct subtasks and the count of tasks below it that need you (recursive over `parent_id`, joined with the project's `subtask_merge`). A query for a task's ancestors feeds the breadcrumb. New `.sqlx` metadata.
- `df-web` (`task_pages`, `sse.rs`): `Filters`/`ListQuery` take `open` and `parent`; `task_rows` renders the disclosure control, the indented subtask rows, the breadcrumb, the dot and the struck-through title; the list stream re-renders with the same query. Dot markup shared with the chat list (`needs_you_dot`).
- Depends on `task-list-active-filter` (chips Active, Done, All); whichever lands second adapts its tests to the other.
- Docs: `docs/content/using/tasks.md` (list, status cues, subtasks).
- Design: PenPot page `Tasks - Subtasks`, boards `Task list · tree, roots collapsed`, `Task list · tree, one level open`, `Task list · drilled into DF-17`.
- Domain model: glossary terms **Top-level task** and **Needs you** added. No ADR: the change is UI only and easy to reverse.

## Out of Scope

- A "Needs you" group or filter in the task list: not now, revisit if the dots prove insufficient (backlog).
- Open requests (approvals, questions) as a needs-you signal in the task list: rejected; the request inbox tracks them.
- Revealing the current task in the tree when a subtask's page is opened by a direct link (opening its ancestors automatically): backlog.
