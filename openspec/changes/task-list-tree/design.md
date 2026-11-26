# Design

## Context

- `df_db::task_views::list_task_items` lists every task of kind `task` matching a `TaskFilter` (status set, project, sort) and counts each row's direct subtasks in a `LATERAL` subquery over `tasks.parent_id` (index `tasks_parent_id_idx`).
- `df-web` renders the rows in `task_pages::view::task_rows`. The status chips are plain links built from `Filters::query()` (`status`, `project`, `sort`); row links append the same query.
- Live updates: `sse::task_list_events` re-renders the whole `#task-rows` on every `TaskChanged` notification and sends it as a morph. Client-side state inside `#task-rows` does not survive a morph.
- A chat is a task of kind `chat`; a task created in a chat has the chat as `parent_id`.
- `subtask_merge` is a project column (`human` default, `parent-agent`). `merge_subtask_pr` merges only when the caller is the subtask's parent and its base task (`task-tree`, "Merging subtask PRs").
- The chat list already draws a needs-you dot (`chat_pages::view::needs_you_dot`, `Tone::Limited`), meaning an open approval request there.
- Scale: well under a hundred open tasks at a time (product owner's estimate).

Requirements: `specs/task-ui/spec.md`. Boards: PenPot page `Tasks - Subtasks`.

## Goals / Non-Goals

**Goals:**
- No JavaScript component for the tree; the server renders every state from the URL.
- One list query, reused for top-level tasks, open rows and the scoped list.

**Non-Goals:**
- Lazy loading of subtasks or pagination: the list stays a full render (scale above).
- Precomputed needs-you counters in a table: rejected while the recursive query is cheap at this scale.
- Changing the chat list's dot meaning: rejected; only its markup is shared.

## Decisions

### D1: Open rows and scope live in the URL

`Filters` gains `open: Vec<String>` (query `open=DF-12,DF-14`) and `parent: Option<String>` (query `parent=DF-17`), parsed in `ListQuery` and written by `Filters::query()`, so row links, chip links and the list stream keep them. The disclosure control is a link to the same page with the key added to or removed from `open`; in a scoped list it leads to `parent=<key>`. Unknown or malformed keys are ignored, like an unknown `status`.

Alternative: Alpine state (`x-data` with open keys) and hidden child rows. Rejected: the list stream morphs `#task-rows` on every task change and would reset it; it also needs every subtree rendered up front and loses the state on reload.

The disclosure link is a sibling of the row link, not nested in it (no nested interactive content), in the row's left gutter with a 44 px touch target. It sets `aria-expanded` and is named "Show subtasks of DF-12" / "Hide subtasks of DF-12". It navigates like the chips (`hx-get` of the list with `hx-push-url`, plain `href` as fallback).

### D2: One query with a scope

`TaskFilter` gains `scope: ListScope` with `TopLevel` (default) and `ChildrenOf(TaskId)`:

- `TopLevel`: `t.parent_id IS NULL OR parent.kind = 'chat'` (`LEFT JOIN tasks parent ON parent.id = t.parent_id`), plus the existing status and project conditions.
- `ChildrenOf(id)`: `t.parent_id = id`, ignoring status and project.
- Link-only filters (`queued`, `running`, `waiting`): no scope condition (flat, as today), because the project counters count every task. `Filters::to_db` picks the scope from the status filter.

Rendering an open row calls the query once per open key with `ChildrenOf`; the scoped list calls it for the scoped task's children and loads the scoped task itself by key. The open keys are few, so N+1 calls bounded by the URL are simpler than one query returning a tree.

Alternative: a recursive query returning the top level plus the children of all open rows in one go, ordered as a tree. Rejected: ordering a tree in SQL with three sort modes costs more code than a handful of extra calls.

### D3: Needs-you count in the list query

`df-core` adds `TaskStatus::NEEDS_YOU = [Waiting, Blocked, InReview]`. Each row gets `needs_you_below: i64`, the number of tasks below it that need you:

```sql
LEFT JOIN LATERAL (
    WITH RECURSIVE below AS (
        SELECT s.id, s.status, s.parent_id, s.base_task_id
        FROM tasks s WHERE s.parent_id = t.id
        UNION ALL
        SELECT c.id, c.status, c.parent_id, c.base_task_id
        FROM tasks c JOIN below b ON c.parent_id = b.id
    )
    SELECT count(*) FILTER (
        WHERE b.status = ANY($n)
          AND NOT (b.status = 'in_review' AND p.subtask_merge = 'parent-agent'
                   AND b.base_task_id = b.parent_id)
    ) AS n
    FROM below b
) nb ON true
```

`p` is the row's project; subtasks share their parent's project (`create_task` creates in the run's project). Whether the row's own status needs you is decided in Rust from `status`, `base_task_id = parent_id` and `subtask_merge`, which the row selects too, as `TaskListItem::needs_you()`. The tree is acyclic (`parent_id` is set at creation and never changes), so the recursion ends.

Alternative: a stored counter updated on every status change. Rejected (Non-Goals).

### D4: Breadcrumb from an ancestors query

`df_db::task_views::ancestors(pool, id)` returns key and title from the scoped task up to its top-level task: a recursive query over `parent_id` that stops at a task without parent or with a chat parent. The header shows "Tasks" (link to the list with the previous `status`, `project`, `sort`), then each ancestor key as a link to `parent=<key>`, the scoped key last and not a link. The scoped list hides the chips and the project menu.

### D5: Status cues in `task_rows`

`task_rows` decides the cue per row: needs you → dot (`components::badges::needs_you_dot`, moved from `chat_pages::view` and shared), `running` → nothing, `completed` → nothing plus `line-through` on the title, any other status → `status_badge`. The dot's accessible name is "Needs you", or "N subtasks need you" when only tasks below need it; in that case the status line shows the same text instead of the agent's status line.

### D6: Live updates

`task_list_events` parses the same `ListQuery` and renders through the same function as the page (top level, open rows, scope). Every `TaskChanged` already re-renders the list, so a status change two levels down reaches the top-level row without new notifications.

## Risks / Trade-offs

- [A subtask whose project differs from its parent's would be judged by the parent row's project] → `create_task` creates in the run's project today; a test pins that the recursive count uses the row's project, and a later cross-project delegation must revisit D3.
- [Long `open` lists in the URL] → each key costs one query; keys are deduplicated, and keys that are no top-level task with subtasks are dropped when rendering.
- [Opening `/tasks/DF-22` directly shows the top level without DF-22] → out of scope (proposal); the details column still shows its parent link.
- [The list stream renders more queries per change (one per open row)] → bounded by the open keys of that viewer; at the stated scale negligible.

## Migration Plan

UI and query changes only; no schema migration. New `.sqlx` metadata for the changed and new queries. Rollback is a revert.
