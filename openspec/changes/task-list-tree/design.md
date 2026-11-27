# Design

## Context

- `df_db::task_views::list_task_items` lists every task of kind `task` matching a `TaskFilter` (status set, project, sort) and counts each row's direct subtasks in a `LATERAL` subquery over `tasks.parent_id` (index `tasks_parent_id_idx`).
- `df-web` renders the rows in `task_pages::view::task_rows`. The status chips are plain links (`filter_chip`) built from `Filters::query()` (`status`, `project`, `sort`): a chip loads the whole page. Row links append the same query.
- Live updates: the element `hx-sse:connect=/tasks/events{query}` sits in the list column (`ws-list-full`) next to `#task-rows`. `sse::task_list_events` re-renders `#task-rows` on every `TaskChanged` notification and sends it as a morph, so client-side state inside `#task-rows` does not survive, and the stream keeps the query it connected with.
- A chat is a task of kind `chat`; a task created in a chat has the chat as `parent_id`.
- `subtask_merge` is a project column (`human` default, `parent-agent`). `merge_subtask_pr` merges only when the caller is the subtask's parent and its base task (`task-tree`, "Merging subtask PRs"). `df_db::tasks::tree::open_subtasks` already decides "has a PR or a pending PR job": `pr_number IS NOT NULL` or an `outbox` row of kind `PR_SYNC_KIND` for the task with `open` set, not done and not given up.
- `limit_reached` does not resume by itself (only `rate_limited` does); a message continues it. `task-delegation` counts `waiting`, `blocked` and `limit_reached` as waiting for a human and does not wake the parent for them.
- The chat list draws a needs-you dot (`chat_pages::view::needs_you_dot`, `Tone::Limited`) for a chat with an open approval request.
- Scale: well under a hundred open tasks at a time (product owner's estimate).

Requirements: `specs/task-ui/spec.md`. Boards: PenPot page `Tasks - Subtasks`.

## Goals / Non-Goals

**Goals:**
- No JavaScript component for the tree; the server renders every state from the URL.
- One list query, reused for top-level tasks, open rows and the scoped list.

**Non-Goals:**
- Lazy loading of subtasks or pagination: rejected; the list stays a full render (scale above).
- Precomputed needs-you counters in a table: rejected while the recursive query is cheap at this scale.
- Changing what makes a chat need you: rejected; only the dot's markup is shared.

## Decisions

### D1: Open rows and scope live in the URL; the list column is swapped as a whole

`Filters` gains `open: Vec<String>` (query `open=DF-12,DF-14`) and `parent: Option<String>` (query `parent=DF-17`), parsed in `ListQuery` and written by `Filters::query()`, so row links, chip links, breadcrumb links and the list stream carry them. Unknown or malformed keys are ignored, like an unknown `status`; keys are deduplicated.

The disclosure control and the scope control are links to the current page (`/tasks` or `/tasks/<key>`) with the changed query. They `hx-get` that URL, select the list column `ws-list-full` from the response, swap it whole (rows and the `hx-sse:connect` element) and push the URL; the plain `href` is the fallback. Swapping the stream element makes it reconnect with the new query, so the next morph keeps the rows open. htmx 4 closes the old SSE connection and opens the new one on such a swap (checked in Chromium 1148 against `just serve-local`: after the swap the stream `/tasks/events` ended with `ERR_ABORTED`, `/tasks/events?open=WEB-1` opened, and a status change of a subtask afterwards arrived below its open parent). The controls' `href` and `hx-get` are relative (`?open=DF-12`, or `?` when nothing stays open), so the rows of the stream, which does not know the open task, keep working on `/tasks/<key>` too.

Alternatives: swapping only `#task-rows` (rejected: the stream keeps the old query and the next morph closes the row); Alpine state with hidden child rows (rejected: the morph resets it, every subtree would be rendered up front, and reloads lose it).

Both controls are siblings of the row link, not nested in it (no nested interactive content), in the row's left gutter with a 44 px touch target. The disclosure control sets `aria-expanded` and is named "Show subtasks of DF-12" / "Hide subtasks of DF-12"; the scope control is named "Open subtasks of DF-17".

### D2: One query with a scope

`TaskFilter` gains `scope: ListScope` with `TopLevel` (default) and `ChildrenOf(TaskId)`:

- `TopLevel`: `t.parent_id IS NULL OR parent.kind = 'chat'` (`LEFT JOIN tasks parent ON parent.id = t.parent_id`), plus the project condition and the status condition. For the Active chip the status condition is `t.status = ANY(active) OR (t.status = ANY(final) AND nb.n > 0)`, using the needs-you count of D3.
- `ChildrenOf(id)`: `t.parent_id = id`, ignoring status and project.
- Link-only filters (`queued`, `running`, `waiting`): no scope condition (flat, as today), because the project counters count every task. `Filters::to_db` picks the scope from the status filter.

Rendering an open row calls the query once per open key with `ChildrenOf`; the scoped list calls it for the scoped task's children and loads the scoped task's row by key. The open keys are few, so a bounded number of calls is simpler than one query returning a tree.

Alternative: one recursive query returning the top level plus the children of all open rows, ordered as a tree. Rejected: ordering a tree in SQL with three sort modes costs more code than a handful of extra calls.

### D3: Needs you in the list query

`df-core` adds `TaskStatus::MAY_NEED_YOU = [Waiting, Blocked, LimitReached, InReview]`; its doc comment says that `in_review` needs you only under the conditions below. The condition, for a task `x` with parent `xp` (may be absent) in project `p`:

```text
x.status IN (waiting, blocked, limit_reached)
OR (x.status = in_review
    AND NOT (xp exists AND no PR AND no pending PR job)
    AND NOT (p.subtask_merge = parent-agent AND x.base_task_id = x.parent_id
             AND xp.status NOT IN final))
```

The "PR or pending PR job" part is the expression of `open_subtasks`, extracted into one SQL fragment per query (sqlx needs literal SQL, so the fragment is repeated and a test pins both queries against the same cases).

Each row gets `needs_you: bool` (its own task) and `needs_you_below: i64` (tasks below it), both from SQL:

```sql
LEFT JOIN LATERAL (
    WITH RECURSIVE below AS (
        SELECT s.id FROM tasks s WHERE s.parent_id = t.id
        UNION ALL
        SELECT c.id FROM tasks c JOIN below b ON c.parent_id = b.id
    )
    SELECT COALESCE(count(*) FILTER (WHERE <needs-you condition on x = b's task>), 0) AS n
    FROM below b JOIN tasks x ON x.id = b.id
    LEFT JOIN tasks xp ON xp.id = x.parent_id
) nb ON true
```

Statuses bind as `::text[]` like the existing query. Subtasks share their parent's project (`create_task` creates in the run's project), so `p` is the row's project. A correlated `WITH RECURSIVE` inside `LATERAL` is already used in `egress.rs`. `parent_id` never changes after creation, so the recursion ends.

Alternative: a stored counter updated on every status change. Rejected (Non-Goals).

### D4: Breadcrumb from an ancestors query

`df_db::task_views::ancestors(pool, id)` returns key and title from the scoped task's parent up to its top-level task: a recursive query over `parent_id` that stops at a task without parent or with a chat parent. Links as in the spec ("Breadcrumb links"): "Tasks" to the list with the previous `status`, `project`, `sort`; the top-level task to that list with `open=<key>`; every other ancestor to `parent=<key>`; the scoped key as plain text. The scoped list hides the chips and the project menu.

### D5: Status cues in `task_rows`

`task_rows` decides per row:

1. Own cue: needs you → dot (`components::badges::needs_you_dot`, moved from `chat_pages::view` and shared); `running` → nothing; `completed` → nothing plus `line-through` on the title; any other status → `status_badge`.
2. If the task itself does not need you and `needs_you_below > 0`: the dot next to the own cue, named "1 subtask needs you" / "N subtasks need you".
3. Status line: resume time if shown (as today), else the needs-you text of step 2, else the agent's status line.

### D6: Live updates

`task_list_events` parses the same `ListQuery` and renders through the same function as the page (top level, open rows, scope). Every `TaskChanged` already re-renders the list, so a status change two levels down reaches the top-level row without new notifications.

## Risks / Trade-offs

- [htmx 4 may not reconnect a swapped SSE element] → task 2.3 checks it first; fallback is a full page load (D1).
- [A subtask whose project differs from its parent's would be judged by the row's project] → `create_task` creates in the run's project today; a later cross-project delegation must revisit D3.
- [The PR-or-job fragment exists twice (`open_subtasks`, list query)] → a test runs both against the same fixtures.
- [Long `open` lists in the URL] → each key costs one query; keys are deduplicated, and keys that are no top-level task with subtasks are dropped when rendering.
- [Opening `/tasks/DF-22` directly shows the top level without DF-22] → out of scope (proposal); the details column still shows its parent link.

## Migration Plan

UI and query changes only; no schema migration. New `.sqlx` metadata for the changed and new queries. Lands after `task-list-active-filter`. Rollback is a revert.
