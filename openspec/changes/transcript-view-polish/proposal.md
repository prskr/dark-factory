## Why

Validating the conversation layout showed three gaps in the task page: the transcript opens at the top and does not follow new messages, many queued messages push the input off screen (the PenPot board "Desktop · many queued messages" defines the fix), and the runs of the details column read oldest first with no way to fold them away.

## What Changes

- The transcript opens scrolled to the latest item and keeps following new items while the reader is at the bottom. A reader who scrolled up is not pulled down.
- The queued message stack becomes a header ("N queued · <persona> reads them at the next turn") above a list that shows at most three rows and scrolls inside itself.
- The runs list of the details column is ordered newest first and can be collapsed. The choice survives live updates.
- No changes to storage, the SSE protocol or the run lifecycle.

Out of scope, answered separately and not part of this change:
- Sandbox cooldown after a turn (no cooldown exists today, the run ends with the turn).
- Setting a task status by hand and an "in review" status (statuses are set by run outcomes only).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `task-ui`: transcript scroll behavior (new requirement), queued message stack layout, order and collapsing of the runs list.

## Impact

- `crates/df-web`: `task_pages.rs` (transcript container), `task_pages/view.rs` (`message_area`, `pending_region`, `runs_region`), `components/transcript.rs`, tests.
- `crates/df-db`: `task_views::list_runs` orders by `number DESC`.
- Design: PenPot board for the collapsed runs list (page "Desktop"), `docs/design-workflow.md` mapping row, `docs/design-system.md`.
