# Tasks

## 1. Design

- [x] 1.1 PenPot: change the `Bottom bar` component (Components page) to Tasks, Requests (with count badge) and Projects at equal widths, and add the floating create button above it at the bottom right. Update the phone boards that show the bar (task list, Projects, Requests). Export the boards, get the user's approval, and add the mapping row to `docs/design-workflow.md` (design D1)

## 2. Phone shell

- [x] 2.1 Failing rendering tests in `layout.rs`: the bar (`#mobile-menu`) holds Tasks, Requests and Projects and no create action; a floating button with the accessible name "New task" exists on the task list and "New project" on Projects; both bar and button carry the transcript-screen hide variant and `md:hidden`. Implement design D2 in `components/shell.rs` and `layout.rs` with the sizes from the approved board
- [x] 2.2 Phone bottom padding covers the floating button in every scroll container (design D3): `main` (bar plus button) and the task list's own scrolling list inside `.ws-list` (button only). Verify with rendering tests for both padding classes and at 390 px with Playwright on the task list, Projects and the Requests inbox (with a long question card): scrolled to the end, the last row's bottom edge and its actions sit above the button ("Last row stays reachable")
- [x] 2.3 Update `docs/design-system.md` (page shell, breakpoints) for the floating create button

## 3. Check against the design

- [x] 3.1 Compare task list, Projects, Requests and a task transcript at 390 px with the approved PenPot boards (`docs/design-workflow.md`, "Comparing a view"): entries evenly distributed, button position, the composer sheet covering the button, no bar or button on the transcript, no horizontal scroll. Desktop at 1440 px unchanged
