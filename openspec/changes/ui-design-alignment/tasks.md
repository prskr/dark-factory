# Tasks

## 1. Design

- [x] 1.1 PenPot: recolor the scope radios in the approval variants of the `Request card` component (Components page): checked ring and dot `color.accent-text`, unchecked ring `color.border-strong`. Export the variants and the "Requests · inbox" and "Phone · requests" boards and get the user's approval (design D1)
- [x] 1.2 PenPot: create the `Icon button` variant set (design D6) with the size and radius the user confirms (proposal: 36 px, radius 8, 20 px icon; 44 px on phone). Replace every icon-only button on the Desktop, Config, Network, Flows and Phone boards with an instance and adjust rows that get too tight. Export the changed boards and get the user's approval; add `Icon button` to the Components row in `docs/design-workflow.md` (design D1)

## 2. Controls

- [x] 2.1 Failing test in `components/actions.rs`: every `ButtonKind` class contains `min-h-11`, `lg:min-h-10` and `rounded-[10px]`; Secondary uses `border-line` and not `border-line-strong`; Primary is `font-semibold`. Implement design D2 and D3 in `action_class` and update affected rendering snapshots
- [x] 2.2 Failing rendering test in `requests.rs`: the answer and comment inputs of a request card have the `INPUT_CLASS` height (`lg:min-h-10`). Implement it (design D2)
- [x] 2.3 Add `accent-color: var(--accent-text)` for radios and checkboxes to the base layer of `styles/input.css` (design D4). Verify with Playwright at 1440 px in light and dark that the checked scope radio's computed `accent-color` is the `accent-text` value
- [x] 2.4 Failing test in `components/icons.rs`: the `Icon::Credentials` body is Lucide `key` (contains `cx="7.5" cy="15.5" r="5.5"`). Swap the SVG body (design D5)
- [x] 2.5 Failing rendering tests: "Remove linked issue" (`task_pages/view.rs`) and "Remove queued message" (`components/transcript.rs`) use `ICON_BUTTON_CLASS`. Search for further icon-only buttons with their own size classes (`size-6`, `size-8`, `size-10` next to an `aria-label`) and switch them too. Set `ICON_BUTTON_CLASS` to the approved size and radius (design D6)
- [x] 2.6 Update `docs/design-system.md` (principle 4, "Spacing, radius, breakpoints", buttons, icon buttons, radios), document the `Ink` and `Danger` variants with tokens and usage without adding them to `ButtonKind` (design D7), and remove the radio sentence from the approval-card deviation in `docs/design-workflow.md`

## 3. Check against the design

- [x] 3.1 Compare at 1440 px and 390 px with the approved PenPot boards (`docs/design-workflow.md`, "Comparing a view"): Requests inbox (button and input heights, radius, radio color), a task view (header buttons, icon buttons of the queued messages and the linked issue), egress rules (delete icon button), Projects (New project, edit modal footer), the traffic page toolbar, and the rail's Credentials icon. Check that no row overflows or wraps because of the taller buttons
