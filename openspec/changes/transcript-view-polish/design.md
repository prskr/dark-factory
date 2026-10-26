## Context

The transcript is a plain `overflow-y-auto` div (`selected_columns`); items are appended by SSE partials (`beforeend` into `#transcript`, `outerMorph` for growing items). Opening a task swaps `#ws-detail` through htmx. The queued stack is `#pending` (`pending_region`), the runs are `#runs` (`runs_region`), both morphed live. htmx is configured to keep `open` on morph (`HTMX_CONFIG`). The page has no inline `<script>`; behavior comes from Alpine attributes.

## Goals / Non-Goals

**Goals:** items 1 to 3 of the request, following the PenPot board "Desktop · many queued messages".
**Non-Goals:** a "jump to latest" button, virtualized transcript, sandbox cooldown, manual task status.

## Decisions

1. **Scroll follow in Alpine, no new script.** The transcript container gets `x-data="{ pinned: true }"`, `x-init` scrolls to the bottom and starts a `MutationObserver` (childList, subtree, characterData) that scrolls to the bottom when `pinned`, and `x-on:scroll` sets `pinned` to "within 48 px of the bottom". A swapped-in container re-runs `x-init`, which covers opening another task. Alternative: htmx events per partial; rejected, they do not fire for in-place text growth and need per-partial logic.
2. **Queued list: a wrapper inside `#pending`, compact rows as in the board.** The board's "queued item" is a single 36 px row (text cut to one line, time and the edit/remove buttons inline), not today's card with up to three text lines and the time below it. The rows switch to that layout, always, so the list does not change shape when the fourth message arrives: the row height equals the existing icon button (`ICON_BUTTON_CLASS`: `size-11 lg:size-9`) with no vertical padding, so `h-11 lg:h-9` holds the buttons and keeps the 44 px touch target below `lg`; text `truncate` with the full text in `title` and in the edit form, time and buttons inline. Header line plus `div` with `max-h` of three rows plus two 4 px gaps, written with the same size classes as the rows (`max-h-[calc(3*2.75rem+2*0.25rem)] lg:max-h-[calc(3*2.25rem+2*0.25rem)]`) so they cannot drift apart, and `overflow-y-auto`. `#pending` keeps its id and the morph keeps the node, so the scroll position of the list survives updates. The persona name is passed to `message_area` / `pending_region` from `TaskDetail.persona_name`.
3. **Runs newest first in the query.** `list_runs` orders `number DESC`; its only consumer is the details column. The transcript dividers are independent of it.
4. **Runs collapse with native `<details open>`.** `runs_region` renders the existing `section` look with `<details>`/`<summary>` ("Runs" + count). `open` is in `morphIgnore`, so live updates do not reopen it. No persistence across reloads (unlike the details column): the state is cheap to redo. Alternative: Alpine + `localStorage`; rejected as more code for no requested benefit.
5. **Design stays the master.** The collapsed runs list has no board yet. It is drawn in PenPot (page "Desktop") before the markup is finished and added to the mapping table in `docs/design-workflow.md`.

## Risks / Trade-offs

- A `MutationObserver` on a long transcript fires per appended node; the callback only sets `scrollTop`, which is cheap. Batches arrive at most every `COALESCE` while streaming.
- Reversing the runs order is visible to anything that reads `list_runs` by position; the only caller and the db test are updated.
