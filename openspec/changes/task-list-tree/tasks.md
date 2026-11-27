# Tasks

## 0. Precondition

- [x] 0.1 Check that `task-list-active-filter` is merged (chips Active, Done, All; link-only `waiting`). Verify: `StatusFilter::CHIPS` in `crates/df-web/src/task_pages/view.rs` has no `Waiting`; otherwise stop and implement that change first.

## 1. Domain and queries

- [x] 1.1 In `df-core` (`domain.rs`), add `TaskStatus::MAY_NEED_YOU` (`waiting`, `blocked`, `limit_reached`, `in_review`) next to `SUBTASK_DONE`, with a doc comment that `in_review` needs you only under the conditions of design D3, and a test that pins the set. Verify: `cargo nextest run -p df-core domain`.
- [x] 1.2 In `df-db` (`task_views.rs`), add `ListScope` (`TopLevel`, `ChildrenOf(TaskId)`) to `TaskFilter` (design D2). Write the tests first: `TopLevel` lists a task without parent and a task whose parent is a chat, not a subtask; `ChildrenOf` lists the direct subtasks in the sort order, ignoring the status set and project; a filter without scope condition (link-only statuses) still lists subtasks. Regenerate `.sqlx`. Verify: `cargo nextest run -p df-db task_views` (container-backed; state it if no socket is available).
- [x] 1.3 Add `needs_you` and `needs_you_below` to `TaskListItem` (design D3). Tests first:
  - `waiting`, `blocked` and `limit_reached` count, also two levels down; `running`, `rate_limited` and `cancelled` do not.
  - `in_review` subtask with PR under `subtask_merge: human` counts; with PR, base task = parent, `parent-agent` and a running parent does not; the same with a cancelled parent counts; with `parent-agent` and another base task counts.
  - `in_review` subtask without PR and without pending PR job does not count; with a pending open job (outbox, `PR_SYNC_KIND`) it counts.
  - a top-level `in_review` task without parent counts without PR.
  - the PR-or-job fragment gives the same answers as `tree::open_subtasks` for the same fixtures.

  Verify as in 1.2.
- [x] 1.4 Make the Active scope list a top-level task in a final status while `needs_you_below > 0` (design D2). Tests first: a `completed` top-level task with a `waiting` subtask is in Active and leaves it when the subtask runs again; it is in Done either way. Verify as in 1.2.
- [x] 1.5 Add `ancestors(pool, id)` (design D4) with tests: a third-level subtask returns its parent and the top-level task in order; it stops below a chat parent; an unknown id returns nothing. Verify as in 1.2.

## 2. Task list UI

- [x] 2.1 Move `needs_you_dot` from `chat_pages::view` to `components::badges` with an optional label, keep the chat list's markup unchanged (its tests stay green), and implement the status cues in `task_rows` (design D5). Tests first in `task_pages/view/tests.rs`: `running` has no badge; `completed` has no badge and a struck-through title; `waiting` shows the dot named "Needs you" and no badge; `failed`, `cancelled`, `queued`, `rate_limited` keep their badge; a `running` row with one task below that needs you shows the dot and "1 subtask needs you", with two "2 subtasks need you"; a `failed` row with tasks below that need you shows the failed badge and the dot; a resume time wins over the needs-you text. Adapt existing assertions on status badges in list frames (for example `>running<` in `sse/tests.rs`) and list pages. Verify: `cargo nextest run -p df-web task_pages::view chat_pages sse`.
- [x] 2.2 Add `open` and `parent` to `ListQuery`/`Filters` and `Filters::query()` (design D1): parse comma-separated keys, deduplicate, ignore malformed keys; chips and row links keep both. Tests first: `?open=DF-12,DF-12,x y` keeps `DF-12` once; chip links keep `open`. Verify: `cargo nextest run -p df-web task_pages`.
- [x] 2.3 First check in a browser (`just serve-local`, skill `run`) that swapping `ws-list-full` with `hx-select` reconnects `hx-sse:connect` with the new query and closes the old connection (network panel); record the result in `design.md` D1 and drop "(unverified)". If it does not, use plain links (D1 fallback). Then render the tree on `/tasks` and `/tasks/<key>`: top-level rows with the disclosure link (sibling of the row link, 44 px target, `aria-expanded`, names "Show/Hide subtasks of <key>"), open rows with their direct subtasks indented one level, and on listed subtasks with subtasks a scope link named "Open subtasks of <key>". Tests first in `task_pages/tests/list.rs` with the scenarios of "Top-level tasks in the task list" and "Opening a task's subtasks"; a top-level task without subtasks has no disclosure link. Verify: `cargo nextest run -p df-web task_pages` (container-backed).
- [x] 2.4 Render the scoped list (`parent=<key>`): breadcrumb from `ancestors` with the links of "Breadcrumb links", scoped task first with its direct subtasks, no chips and no project menu; an unknown key falls back to the top-level list. Tests first with the scenarios of "List scoped to a subtask" and "Breadcrumb links". Verify as in 2.3.
- [x] 2.5 Make `sse::task_list_events` render through the same function with `open` and `parent` (design D6). Tests first in `sse/tests.rs`: with `open=DF-12`, a status change of a subtask of `DF-12` arrives within 2 seconds and the subtask stays below `DF-12`; a grandchild becoming `blocked` adds the dot to the top-level row; in a scoped list, a status change of a listed subtask arrives. Verify: `cargo nextest run -p df-web sse` (container-backed).
- [x] 2.6 Check `/tasks` at 390 px and 1280 px against the PenPot boards `Task list · tree, roots collapsed`, `Task list · tree, one level open` and `Task list · drilled into DF-17` (page `Tasks - Subtasks`) in light and dark, as described in `docs/design-workflow.md`, with the layout decisions of `design-boards.md` (`/complete-change` moves its rows into the board mapping). Verify: no horizontal scroll at 390 px, both controls have a target of at least 44 px.

## 3. Documentation

- [x] 3.1 Update `docs/content/using/tasks.md`: the list shows top-level tasks with disclosure controls, one indented level, deeper levels as a scoped list with breadcrumb, Active keeping a finished top-level task while something below needs you, the status cues (no badge for running and done, struck-through done titles, the needs-you dot, when a task needs you including `limit_reached`, PRs and `subtask_merge`), and that the project counters' links list subtasks as flat rows. Replace the sentence that subtasks stay ordinary rows. Verify: `just docs-build`.

## 4. Integration checks

- [x] 4.1 Find e2e tests and seed data that rely on subtask rows in the default task list (`crates/df-e2e`: `tests/task_tree.rs`, `tests/demo_seed.rs`, `tests/delegation.rs`) and open the parent row first or use the subtask's URL. Verify: `just e2e-local` is green (skill `e2e`); state it if no container socket is available.
- [x] 4.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `openspec validate task-list-tree --strict`. Verify: all pass.
