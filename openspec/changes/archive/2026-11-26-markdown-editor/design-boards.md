# Design boards: markdown-editor

- Status: approved (2026-11-26)
- Tokens added: `color.code.bg`, `color.code.text`, `color.code.keyword`, `color.code.string`, `color.code.function`, `color.code.comment` in sets `light` and `dark` (values below). They mirror the default code themes for the boards only; the UI keeps its colours from `/assets/code-theme.css`, so no CSS token is added.
- Components added or changed: none (the view switch and the "Highlighting" switch are drawn locally on the boards)

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Config - Persona page / Persona page · general | `/personas/<id>` | persona `linus` ("Linus", `linus@example.org`, description set), harness `claude-code`, provider "Claude subscription", model `claude-sonnet-5-5` | 1440 |
| Config - Persona page / Persona page · general, model error | `/personas/<id>` | as Persona page · general; model cleared on General, "Save persona" clicked on System prompt. Partial board: main column only | 1440 |
| Config - Persona page / Persona page · commits | `/personas/<id>?tab=commits` | as Persona page · general, with a signing key, on an instance with the `forgejo` plugin. Partial board: main column only | 1440 |
| Config - Persona page / Personas · new persona | `/personas` | "New persona" clicked, fields filled. Partial board: modal only | 1440 |
| Config - Persona page / Phone · persona page, general | `/personas/<id>` | as Persona page · general | 390 |
| Config - Personas / Personas · card, no system prompt yet | `/personas` | persona `ada` without description and system prompt. Partial board: one card only | 1440 |
| Config - Persona page / Persona page · system prompt | `/personas/<id>?tab=system-prompt` | as Persona page · general with a prompt holding a heading, a list (bold, inline code, link), a `rust` fence and a `mermaid` fence; editor bundle embedded; "Save persona" clicked | 1440 |
| Config - Persona page / Persona page · system prompt, preview | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt; "Preview" chosen. Partial board: main column only | 1440 |
| Config - Persona page / Persona page · system prompt, 1024, editor | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt; editor focused. Partial board: main column only | 1024 |
| Config - Persona page / Markdown field · preview states | `/personas/<id>?tab=system-prompt` | switch row with "Highlighting" off; preview with empty text, after a failed live request, after a failed request in "Preview", for a text over 256 KiB, after the session ended. Partial board: field parts only | 1440 |
| Config - Persona page / Phone · persona page, system prompt | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt | 390 |

## Decisions the specs don't carry

- Header: avatar (40 px, palette colour of the persona), display name (24 px semibold), "harness · model" (14 px `text-muted`) in one row, the save status ("✓ Persona saved", `text-accent-text`) left of "Save persona" right-aligned. On the phone the "harness · model" line sits below the name row (13 px).
- General: groups "Identity" and "Agent" at the full width of the panel, like the project settings page (it has no narrower field column); Identity keeps the modal's row (avatar preview, Display name, Handle), Description full width below; Agent keeps Harness and Model provider in one row and Model full width below.
- Commits tab: the group "Commits" exactly as in the modal (board "Personas · commits, Forgejo"), with its heading, at the full width of the panel.
- A field error: red border (`danger`) on the control and the error line (`text-sm text-danger`) below the hint.
- "New persona" modal: the edit modal's layout without System prompt; the group "Commits" holds Commit email and the no-key line (`text-sm text-muted`); primary button "Create persona".
- Phone: tab row of the project phone page (pill tabs, scrolls); Identity stacks avatar, Display name, Handle; Agent stacks Harness, Model provider, Model.
- Card: "No system prompt yet" in `text-faint`, in the place of the description or prompt start.
- Markdown field on the System prompt tab: full width of the panel (as the other tabs). Top row: the view switch centred, the "Highlighting" switch right-aligned in the same row; 12 px gap to the panels; the indent hint (12 px `text-faint`) below the editor.
- View switch: one bordered group (`border-line-strong`, radius 8, 2 px padding, 2 px gap, `bg-panel`); each segment 13 px text with a 14 px Lucide icon (`pencil`, `columns-2`, `eye`), padding 4 × 10 px, radius 6. Selected: `bg-sunk`, `text-ink`, medium weight; others `text-muted`.
- "Highlighting" switch: label 13 px medium `text-muted`, track 28 × 16 px rounded; on: `accent-text` track, knob right; off: `muted` track (3:1 against the page), knob left; knob `panel`.
- Split: two equal columns, 16 px gap. Editor: `bg-panel`, `border-line-strong`, radius 8, 10/12 px padding, 13 px monospace, line height 1.6; focus ring 2 px `accent-text`. Preview: `bg-panel`, `border-line`, radius 8, 14/16 px padding, the usual `.markdown` styles; fenced code on the code theme's background with a `border-line` border, inline code on `sunk`.
- Editor highlighting (step 2): markers (`#`, `-`, `**`, backticks, fence lines, link URL and brackets) `faint`; heading text semibold in `color.code.function` (classes `hl-entity hl-name hl-function`, so the configured code theme colors it); strong text semibold `ink`; link text `accent-text`; plain text `ink`; fence code in the code theme (`hl-` classes). The boards show `ink` headings; this line supersedes them (decided after review, boards not redrawn).
- Preview states: "Nothing to preview" 14 px `text-faint`; "Preview not up to date" 12 px `text-faint` above the old preview; "Preview not available" (with a secondary "Retry" button below it), "Text too long for a preview (max 256 KiB)" and the session message 14 px `text-danger`.
- Phone and 1024 px: the switch row has only Editor and Preview; on phones no "Highlighting" switch and 16 px monospace text.
- Code tokens (light / dark): `color.code.bg` `#FFFFFF` / `#2B303B`, `color.code.text` `#323232` / `#C0C5CE`, `color.code.keyword` `#A71D5D` / `#B48EAD`, `color.code.string` `#183691` / `#A3BE8C`, `color.code.function` `#795DA3` / `#8FA1B3`, `color.code.comment` `#969896` / `#65737E` (InspiredGitHub and base16-ocean.dark).
- New text pairs for `TEXT_PAIRS`: `danger` on `panel`, `ink` on `sunk`. The code-theme comment colour is below 4.5:1 in both default themes (operator theme, unchanged by this change).
