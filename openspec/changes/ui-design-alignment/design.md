# Design

## Context

See proposal.md for the motivation. Current state:

- `action_class` (`crates/df-web/src/components/actions.rs`) builds every text button: `min-h-11 … rounded-lg … lg:min-h-9`, Secondary with `border-line-strong`, all labels `font-medium`. `docs/design-system.md` documents "36 px from 1024 px" and "inputs and buttons 8 px (`rounded-lg`)".
- `INPUT_CLASS` (`components/forms.rs`) is already `lg:min-h-10` (40 px). The request card (`requests.rs`) writes its own input classes with `lg:min-h-9`.
- No stylesheet rule sets `accent-color`. The scope radios (`requests.rs`) and the DNS checkbox of the egress rule form (`egress/allowlist.rs`) render in the browser's default blue. The verdict radios of the traffic page are `sr-only` and are not affected.
- PenPot `Button` (Components page): 40 px high, radius 10, padding 16, gap 8, label 14 px. Secondary: `color.panel` fill, `color.border` stroke, label weight 500. Primary: `color.accent` fill, label weight 600. One Primary variant without an icon has padding 18; the others use 16.
- Icon-only buttons: `ICON_BUTTON_CLASS` (`size-11 … lg:size-9 rounded-lg`, 18 uses). Some buttons write their own classes: "Remove linked issue" (`task_pages/view.rs`) is `lg:size-6`, "Remove queued message" (`components/transcript.rs`) has its own. PenPot has no icon-button component; the boards draw square icon buttons at 36 px radius 8 (Config, Network tables), 28 px radius 6 (task view), 32 px radius 8 and 40 px.
- PenPot `Button` also has `Ink` (`color.ink` fill, `color.on-ink` label) and `Danger` (`color.panel` fill, red stroke, `color.failed` label) variants. No screen of the app uses either.
- PenPot `Rail` draws Credentials as Lucide `key` (circle at 7.5/15.5, r 5.5, plus two strokes). The app ships Lucide `key-round`.

## Goals / Non-Goals

**Goals:**
- Buttons, icon-only buttons, request card inputs, radios and the Credentials icon match the PenPot file.
- `Ink` and `Danger` are documented, so the first screen that needs one uses the designed style.

**Non-Goals:**
- No new button kinds in code. `Ink` and `Danger` are documented, not built.
- No change to the round actions (`ROUND_ACTION_CLASS`, `ROUND_PILL_CLASS`) or the 44 px touch size below 1024 px.

## Decisions

**D1: Design first for the radios and the icon button.** The orchestrator recolors the radios in the PenPot `Request card` variants (checked ring and dot in `color.accent-text`), draws the `Icon button` component (D6) and replaces the icon buttons on the boards with its instances, exports them for approval, and only then changes the code. `Button` and the key icon already show the target state, so they need no design step. Alternative: implement first and draw afterwards. Rejected: PenPot is the master of the UI design.

**D2: One height token for all text controls.** `action_class` becomes `min-h-11 … lg:min-h-10 rounded-[10px]`, the same height as `INPUT_CLASS`. The request card uses `INPUT_CLASS` height for its answer and comment inputs, so a field and its button line up. Padding stays 16 px (`px-4`); the 18 px of one PenPot variant is treated as a drawing slip. Primary uses `font-semibold`, Secondary and Ghost keep `font-medium`. Alternative: keep 36 px and change the design. Rejected: the user asked to follow the design.

**D3: Secondary border uses `line`.** The design's `color.border` maps to the `line` token. The button's label identifies it, so the border does not need 3:1 contrast (WCAG 1.4.11 applies to the boundary only when nothing else identifies the control). Inputs keep `line-strong`, because an empty field has nothing else that marks it.

**D4: `accent-color: var(--accent-text)` in the base layer.** One rule in `styles/input.css` (`input[type=radio], input[type=checkbox]`) covers every current and future radio and checkbox. The bright `accent` (#00ff41) reaches only about 1.4:1 on the light panel and would fail the 3:1 for the checked state; `accent-text` passes in both themes and is the accent tone the design already uses for text and active links. The browser picks the dot color for contrast. Alternative: custom-drawn radios. Rejected: more markup and CSS for the same result.

**D5: Swap the SVG body of `Icon::Credentials`.** The variant keeps its name; only the paths change to Lucide `key` (`<circle cx="7.5" cy="15.5" r="5.5"/><path d="m21 2-9.6 9.6"/><path d="m15.5 7.5 3 3L22 7l-3-3"/>`), which matches the PenPot rail. Lucide is already vendored under its licence (`LICENSE-Lucide.txt`).

**D6: One `Icon button` component.** PenPot gets an `Icon button` variant set (Components page) with one desktop size and radius, a ghost style (no fill, `color.muted` icon, `color.sunk` on hover) and a 44 px phone size. Proposal: 36 px, radius 8, 20 px icon, because it is the most common size on the boards and what the app renders today; the user confirms it in task 1.2. Smaller icon buttons inside dense rows (the 28 px ones of the task view) become instances of the same component. Where a row is too tight, the row changes, not the button. In code, `ICON_BUTTON_CLASS` is the only class for icon buttons; buttons with their own classes switch to it. Alternative: a small and a regular size. Rejected: two sizes are what drifted in the first place, and one size is enough for every current use.

**D7: `Ink` and `Danger` are documented, not built.** `docs/design-system.md` describes both variants with their tokens and when to use them: `Danger` for a destructive text action (delete, revoke) next to Cancel in a confirmation, `Ink` for a strong action on a light surface where the accent would compete with another primary action. `ButtonKind` gets no new variants until a screen needs one. Alternative: add the variants now. Rejected: no caller, and unused variants drift unnoticed.

## Risks / Trade-offs

- [Taller buttons on desktop change the height of rows that hold buttons (page headers, dialogs, the traffic toolbar)] → The comparison task checks those views at 1440 px; inputs are already 40 px, so most rows get more even, not taller.
- [The 28 px icon buttons in the task view grow to 36 px and can crowd their rows] → Task 1.2 redraws those rows in PenPot with the larger button before any code change, so space problems show up in the design first.
- [Rendering snapshots that contain button classes change] → Update them in the same task as the class change.
