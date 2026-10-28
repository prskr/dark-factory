# Design

## Context

See proposal.md for the motivation. Current state below 768 px:

- `bottom_bar` (`crates/df-web/src/components/shell.rs`) is a fixed `nav` (`h-16`, `z-30`, `justify-around`) with `bar_link` entries for Tasks and Requests, a centred `create_button` slot and Projects. It hides on the transcript screen through `[body[data-view=task]_&]:hidden` on the bar itself (`docs/design-system.md`, "Phone sheet").
- `create_action` (`layout.rs`) picks label and target per screen: "New project" on Projects, "New task" elsewhere. The same `create_button` serves the rail.
- `main` reserves `pb-20` (80 px) on phones for the 64 px bar.
- Sheets and modals are native `<dialog>` elements opened with `showModal()` (`components/overlay.rs`), so they render in the browser's top layer above any fixed element.

## Goals / Non-Goals

**Goals:**
- One bar layout that stays balanced for any number of navigation entries.
- The create action keeps its behaviour, accessible names and touch size.

**Non-Goals:**
- No change to the desktop rail.
- No hide-on-scroll or other motion for the floating button.
- No new sections in the bar.

## Decisions

**D1: Design first in PenPot.** The orchestrator changes the `Bottom bar` component (Components page) and the phone boards that show it, exports them for approval, and adds the mapping row to `docs/design-workflow.md` before code changes. The approved boards are the reference for size, offset, shadow and colours. Alternative: implement first and draw afterwards. Rejected: PenPot is the master of the UI design.

**D2: The floating button is a sibling of the bar, rendered by the shell.** `layout` renders the bar and, next to it, the floating `create_button` with its own fixed position (bottom right, above the bar, phone only, `md:hidden`) and the same transcript-screen variant the bar uses. The bar keeps only `bar_link` entries with `flex-1`, so equal widths follow from flexbox for any count. Alternative: keep the button inside the bar's markup and position it absolutely. Rejected: the bar's hide variant and `justify-around` would still have to work around it.

**D3: The scroll container reserves room for the button.** Every phone scroll container that ends above the bar reserves space for the button at its end. `main` on pages that scroll the document reserves bar height + button height + both gaps (about 152 px, set per the PenPot board) instead of `pb-20`. The task list scrolls inside `.ws-list`, whose `.ws` already stops at the bar (`h-[calc(100dvh-4rem)]`), so `main`'s padding never reaches it: its scrolling list gets bottom padding of button height + both gaps (about 88 px) on phones. Alternative: hide the button while the list is scrolled to its end. Rejected: needs scroll listeners and changes what the user can tap.

**D4: Layering stays native.** The button uses the bar's `z-30`. Dialogs and sheets cover it through the top layer; the details backdrop on the task page is not affected, because the button is hidden on that screen.

## Risks / Trade-offs

- [The button covers content near the bottom right that is not the last row, such as an action at the right end of a row in the middle of the screen while scrolling] → It is the usual floating-button trade-off; the padding keeps the end of every list reachable. Check the Requests inbox (answer buttons at the right) at 390 px against the board.
- [The rendering tests assert the create action inside `#mobile-menu`] → Update them to assert the bar holds no create action and the floating button exists with the right label.
