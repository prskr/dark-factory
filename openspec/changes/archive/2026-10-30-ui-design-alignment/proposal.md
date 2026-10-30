# Proposal

## Why

Redrawing the PR #35 views in PenPot showed five places where the web UI does not follow the design:

- Every button is 36 px high from 1024 px, with an 8 px radius and the strong border. The design's `Button` component is 40 px high, has a 10 px radius and uses the light border.
- The approval scope radios use the browser's default blue, because the app sets no `accent-color`.
- The rail's Credentials icon is Lucide `key-round`. The design uses Lucide `key`, which the user prefers.
- Icon-only buttons have no component in the design, and the boards draw them in four sizes (28, 32, 36 and 40 px). The app uses one size (36 px from 1024 px).
- The design's `Button` has `Ink` and `Danger` variants that `docs/design-system.md` does not mention.

## What Changes

- Buttons (`action_class`: Primary, Secondary, Ghost) are 40 px high from 1024 px and stay at least 44 px below. Their radius is 10 px. Secondary buttons use the `line` border token instead of `line-strong`. Primary labels are semibold, as in the design.
- The request card's answer and comment inputs follow `INPUT_CLASS` and get the same height as buttons (40 px from 1024 px). Today they are 36 px.
- Radios and checkboxes use the design's accent color (`accent-text`) through `accent-color`. This applies everywhere, not only to the scope picker.
- The Credentials icon becomes Lucide `key`, the icon in the PenPot `Rail` component.
- PenPot gets an `Icon button` component with one size and radius. The icon-only buttons on all boards become instances of it. `ICON_BUTTON_CLASS` and the hand-written icon buttons in the app follow it.
- `docs/design-system.md` documents the `Ink` and `Danger` button variants of the design for later use. Neither is built: no screen needs them yet.
- PenPot: the radios in the `Request card` component get the accent color and the `Icon button` component is drawn before the code changes. The `Button` component and the key icon already show the target state.
- `docs/design-system.md` records the new button height and radius, and the radio color. The known deviation about the radios in `docs/design-workflow.md` is removed.

## Capabilities

### New Capabilities

_None._

### Modified Capabilities

- `web-ui-shell`: a new requirement "Controls follow the design system" covers button and icon-button size and radius and the color of checked radios and checkboxes.

## Impact

- `crates/df-web/src/components/actions.rs` (`action_class`) and its tests.
- `crates/df-web/src/requests.rs` (answer and comment inputs).
- `crates/df-web/styles/input.css` (`accent-color` in the base layer).
- `crates/df-web/src/components/icons.rs` (`Icon::Credentials`).
- `ICON_BUTTON_CLASS` (`components/actions.rs`) and icon buttons with their own classes (for example "Remove linked issue" in `task_pages/view.rs`, "Remove queued message" in `components/transcript.rs`).
- `docs/design-system.md`, `docs/design-workflow.md`.
- PenPot file "App Design": `Request card` component (radio color), new `Icon button` component, icon buttons on the Desktop, Config, Network, Flows and Phone boards.
- No API, database or dependency changes.
