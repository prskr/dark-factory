# Proposal

## Why

`iter2-agent-requests` added Requests to the phone bottom bar. The bar was designed for three slots with the create action in the middle (Tasks, create, Projects); with four slots the "+" sits third of four and looks off-centre. Any further section would make it worse.

## What Changes

- The phone bottom bar (below 768 px) holds only navigation entries: Tasks, Requests (with its pending count) and Projects, evenly distributed. It works for any number of entries.
- The create action leaves the bar and becomes a floating action button ("+") above the bar at the bottom right. Its behaviour is unchanged: it creates what the current screen lists (a project on Projects, a task elsewhere) and has the same accessible names ("New task", "New project").
- The floating button is hidden wherever the bar is hidden (task transcript screen), and sheets and dialogs cover it.
- Pages with the bottom bar get enough bottom padding that the button never covers the last row of a list.
- Desktop (rail, from 768 px) does not change.
- PenPot: the `Bottom bar` component and the phone boards get the new layout before the code changes.

## Capabilities

### New Capabilities

_None._

### Modified Capabilities

- `web-ui-shell`: "Phone bottom bar" lists Tasks, Requests and Projects and moves the create action to a floating button above the bar.
- `task-ui`: the scenario "New task on a phone" taps the floating button instead of the bar.

## Impact

- `crates/df-web/src/components/shell.rs` (`bottom_bar`, `create_button`), `crates/df-web/src/layout.rs` (shell, page padding), their rendering tests.
- `docs/design-system.md` (page shell, breakpoints), `docs/design-workflow.md` (mapping row for the changed boards).
- PenPot file "App Design": `Bottom bar` component, phone boards.
- No API, database or dependency changes.
