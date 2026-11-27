# Proposal

## Why

`docs/design-system.md` promises that new views are built from components and tokens and need no new styling decisions. The code doesn't keep that promise. Confirmation dialogs, tab rows, status dots, count pills and field labels are rebuilt by hand in several places. Two PenPot components (`Task row`, `Request card`) live in page modules. 85 arbitrary type and radius values (`text-[13px]`, `rounded-[10px]`, …) bypass the token set. Several CSS tokens have different names from their PenPot tokens (`ink` vs `color.text`), so every comparison with the design needs a translation table that is only partly written down. The guard against raw palette colours covers only the task pages. Pre-MVP is the time for the breaking renames this needs.

## What Changes

- **New components in `crates/df-web/src/components/`:** a confirmation dialog (`confirm_dialog`, replacing the 3 hand-built confirmations and the frame of the "Verify key" dialog), `tabs` (one tab row for the 5 hand-built `role="tablist"` rows), `status_dot` (replacing about 8 copies of `size-2 rounded-full … dot_class()`), `count_pill` (the neutral count after a tab label, an archive entry and the subtask `done/total` chip), `field_label` / `legend_label` and `SMALL_HEADING_CLASS` (the 11 hand-written `block text-sm font-medium text-muted` labels and the uppercase `faint` headings).
- **Moved into `components/`:** the PenPot components `Task row` (`task_row`, from `task_pages/view.rs`) and `Request card` (`request_card` with `CardData`, from `requests.rs`), and `count_badge` (the live pending count of the navigation, today imported by `components/shell.rs` from `requests.rs`).
- **Copies replaced** by the components above. The two hand-built tables (the egress events table and the token table of a run card) were checked against `data_table`. Neither fits, and the design records why.
- **PenPot mapping:** `docs/design-system.md` gets a table that maps each PenPot component and variant to its Rust function and enum value, and each PenPot token to its CSS token.
- **BREAKING (internal): CSS tokens renamed to their PenPot names:** `line` → `border`, `line-strong` → `border-strong`, `ink` → `text` (text) and `ink` (fills, as PenPot `color.ink`), `accent-text` → `accent-strong`, `danger` → `failed`, `bubble-agent` / `on-bubble-agent` → `agent-bg` / `agent-fg`, plus the renames task 1.1 finds for the tokens whose PenPot names are not yet known (unverified). Utilities change with them (`border-line` → `border-border`, `text-ink` → `text-text`). The rendered UI looks the same.
- **Type and radius tokens:** `@theme` tokens in `styles/input.css` for every arbitrary font size (`text-meta` 13 px, `text-lead` 15 px, `text-lead-lg` 17 px, `text-2xs` 11 px, `text-3xs` 10 px, `text-4xs` 9 px, `text-code` 0.9 em) and radius (`rounded-control` 10 px, `rounded-t-sheet` 18 px). The arbitrary values are removed.
- **Styling guard for every page:** a test over the compiled stylesheet fails on any Tailwind palette colour utility and any arbitrary font-size, line-height, letter-spacing or radius utility anywhere in the crate. The rendered-page check of the task pages stays and uses the same rules.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui-shell`: "Controls follow the design system" names the renamed tokens (`border`, `accent-strong`). A new requirement says every page uses only design tokens for colour, type size and radius.

## Impact

- **Code:** `crates/df-web` only: `styles/input.css`, `src/components/` (new modules `dialog`, `tabs`, `task_row`, `request_card`; additions to `badges`, `forms`, `lists`), the page modules that used the copies (`task_pages`, `config_pages`, `chat_pages`, `requests`, `artifacts`, `egress`, `project_webhook`, `project_subscriptions`), `editor/markdown-editor.ts` (CSS variables by name), the tests that assert class strings, and the snapshots under `src/**/snapshots/`. No route, no database, no configuration key, no protobuf message changes.
- **Docs:** `docs/design-system.md` (mapping table, token and type tables, new components), `docs/design-workflow.md` and skill `penpot` (the token-name exception table goes away). The published docs (`docs/content/`) don't describe the UI's tokens or components, so they don't change.
- **Parallel work:** `task-list-tree` rewrites the task rows. This change moves the row after `task-list-tree` has landed (design D9). Every open change that edits `df-web` markup rebases over the token rename. The rename is one mechanical commit, so redoing it is cheap.
- **Domain model:** unchanged. No glossary term and no ADR is added. The token rename is easy to revert, so it isn't an ADR (design D5).
- **Out of scope:**
  - A style-guide route that renders every component: backlog (row added when this change is archived).
  - htmx error handling, region IDs and inline JS: a separate task (backlog).
  - Splitting large files, except where a moved component needs it: backlog.
  - Arbitrary spacing and size values (`pl-[17px]`, `w-[3px]`, `grid-cols-[76px_…]`): backlog.
  - Drawing PenPot main components for the new code components (confirmation dialog, tabs, status dot, count pill): backlog.
  - Arrow-key navigation for the archive's link tabs: backlog (D3).
