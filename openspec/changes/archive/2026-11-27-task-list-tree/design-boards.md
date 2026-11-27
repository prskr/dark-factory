# Design boards: task-list-tree

- Status: approved (2026-11-27)
- Tokens added: none
- Components added or changed: none

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Tasks - Subtasks / Task list · tree, roots collapsed | `/tasks` | Active chip; top-level DF-15 `queued`, DF-14 `running` (1/4), DF-12 `running` (2/3, two tasks below need you), DF-11 `in_review` with PR (needs you), WEB-3 `queued`, DF-9 `rate_limited`, DF-8 `failed` (1/3, two below need you), DF-10 `completed` (2/3, one below needs you). Partial board: task list column only | 390 |
| Tasks - Subtasks / Task list · tree, one level open | `/tasks?open=DF-12` | as above; DF-12 open: DF-13 `completed`, DF-16 `in_review` with PR (needs you), DF-17 `running` (1/2, DF-22 needs you), DF-18 `cancelled`. Partial board: task list column only | 390 |
| Tasks - Subtasks / Task list · drilled into DF-17 | `/tasks?parent=DF-17` | as above; DF-17 with DF-21 `completed` and DF-22 `waiting`. Partial board: task list column only | 390 |

## Decisions the specs don't carry

- The boards are the list column (340 px content); the same column serves 390 px phones and the desktop list column (1280 px).
- Meta line order: needs-you dot, key, status badge, subtask chip, spacer, time. The dot is the existing `needs_you_dot` (10 px, `color.status.limited.fg`).
- Disclosure control: chevron in the row's left gutter, outside the row body, at the height of the meta line; `›` when closed, `˅` when open. The scope control on a listed subtask uses the same `›` (user decision: no distinct glyph; the indent tells them apart).
- Open subtask rows: indented one level (content starts 20 px further right), a 1 px `color.border` tree guide 16 px from the row's left edge, no dividers between subtask rows; the block ends with the normal divider. Subtask rows are more compact than top-level rows: 70 px instead of 92 px, title 14/500 instead of 15/500.
- The highlighted row (`color.selected` with selection edge) is the selected task, as today; on the scoped board it is the scoped task.
- Scoped list: header holds only the breadcrumb ("Tasks › DF-12 › DF-17", the scoped key in bold, ancestors in `color.muted`, separators `›`), no title count, no chips, no project menu. The scoped task's row has no chevron.
- Struck-through title (`completed`) keeps `color.text`; no badge.
- A needs-you subtask shows its agent status line (DF-22 "Asks you a question"), not "N subtasks need you".
