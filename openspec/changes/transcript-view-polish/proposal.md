## Why

Validating the conversation layout showed three gaps in the task page: the transcript opens at the top and does not follow new messages, many queued messages push the input off screen (the PenPot board "Desktop · many queued messages" defines the fix), and the runs of the details column read oldest first with no way to fold them away.

## What Changes

- The transcript opens scrolled to the latest item and keeps following new items while the reader is at the bottom. A reader who scrolled up is not pulled down.
- The queued message stack becomes a header ("N queued · <persona> reads them at the next turn") above a list that shows at most three rows and scrolls inside itself.
- The runs list of the details column is ordered newest first and is collapsed by default and can be opened. The choice survives live updates.
- From 768 px the sections of the details column above Runs stay fixed. The runs list is the only scrolling area, inside the Runs section, which fills the remaining column height; the chevron collapses it. A long task description is cut after three lines (full text in `title`).
- Tool-call rows get a Lucide wrench in front, a chevron at the end that turns when the row is open, and a thin separator above and below; consecutive tool calls share one separator.
- The end of a turn is no longer a transcript item: the run divider separates runs and the run card has the model and its usage.
- The run number in the runs list links to the start of that run in the transcript.
- The token table of a run uses compact counts (k, M) with the exact value as tooltip, and the model name gets its own line, so long model names do not squeeze the columns.
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
- `crates/df-web`: `components/overlay.rs` (`details_panel` body) and the `ws-details` styles.
- `crates/df-web`: `components/transcript.rs` (`tool_row`), `components/icons.rs` (`Wrench`), `styles/input.css` (separators).
- `crates/df-web`: `transcript/mod.rs`, `components/transcript.rs` (`turn_end` removed), `task_pages/view.rs` (`run_card`: run link, token table).
- `crates/df-db`: `task_views::list_runs` orders by `number DESC`.
- Design: PenPot board for the collapsed runs list (page "Desktop") and chevron and count on the open runs header of "Desktop · light"; tool rows of the Desktop boards (wrench, chevron at the end, separators), `docs/design-workflow.md` mapping row, `docs/design-system.md`.
