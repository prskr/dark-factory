# Design

## Context

- `components/table.rs` renders the configuration tables:
  - `data_table(head, rows)` draws a card that scrolls sideways inside itself (`overflow-x-auto`, `relative`).
  - `th` and `td` take a `Cell` kind (First, Text, Mono, Number, Actions).
  - `tr` draws a bordered row.
- The projects, credentials, model providers and egress rules lists use these tables.
- Projects (`config_pages.rs`, `projects_region`):
  - From 768 px: a table with Project, Repository, Branch, four count columns and an edit column. The edit column holds `project_link`, a pencil linking to `/projects/{key}`.
  - Below 768 px: a separate card list (`md:hidden`) with its own edit column.
  - `count_link` renders every count as a link. A zero is muted (`text-faint`).
- Credentials (`credentials_region`): Name, Secret, Key and `edit_button`. The edit button is an `open_modal` `<button hx-get=… hx-target="#composer-slot">`.
- Model providers (`config_pages/providers.rs`, `row`):
  - Columns: Name, Wire APIs, Scheme, Credential, Used by, and edit + delete.
  - Delete is `hx-delete` with `hx-confirm` and `aria-label="Delete model provider {name}"`.
  - `ui-model-provider-page` (implemented before this change) turns the name and the edit action into links to `/model-providers/{id}`.
- Styling is Tailwind v4 (`styles/input.css`, semantic tokens such as `bg-sunk`, `bg-panel`, `text-muted`). The settings sidebar already uses `hover:bg-sunk` for its entries.
- The approved boards are listed in proposal.md. Their sizes:
  - The key column is 76 px.
  - Rows are about 71 px high, with padding 14 px top and bottom.
  - Details text is 13 px, `text-muted`.

## Goals / Non-Goals

**Goals:**
- One shared row pattern in `components/table.rs` that the three lists use, so the layout, the click layer and the states stay identical.
- Keep `<table>` semantics, so the count columns keep their headers for assistive technology.

**Non-Goals:**
- **Changing the phone project list:** see proposal, rejected.
- **A JavaScript row click handler:** rejected. The row link is plain HTML and CSS.
- **Applying the pattern to the egress rules table:** rejected. It has no edit target.

## Decisions

### D1: Keep the table; two lines live inside one cell

The name cell holds two block lines, the name and the details. Projects get an extra leading key cell, with a screen-reader-only header "Key". The counts, "Used by" and the actions stay their own cells, vertically centered (`align-middle`). The name cell takes the remaining width (`w-full max-w-0`), and both of its lines truncate with an ellipsis (`truncate`). With the automatic table layout, `min-w-0` and `truncate` alone don't stop the cell from growing to the full length of a nowrap URL. `max-w-0` takes the cell's content out of the minimum-width calculation, so a long repository URL never widens the table.

- **Alternative: a `<ul>` with CSS grid, like the phone list.** Rejected. It loses the `th`/`td` association that tells a screen reader "Running" for a count.

### D2: A stretched link makes the row clickable

The row (`tr`) is `relative`. The entry control in the name cell covers the row with `after:absolute after:inset-0`. Controls that keep their own action (count links, delete) are `relative z-10`, so they sit above the cover. A helper in `table.rs` renders the entry control in one of two forms:

| Form | Element | Used by |
|---|---|---|
| Page | `<a href>` with the entry name as text | projects (`/projects/{key}`), model providers (`/model-providers/{id}`) |
| Modal | `<button>` with the existing `open_modal` attributes (`hx-get`, `hx-target="#composer-slot"`), the name as text and `aria-label="Edit credential reference {name}"` | credential references |

- A link keeps middle-click and "open in new tab" for pages.
- A modal opener is a button, because it does not navigate.
- Projects and providers keep their existing accessible link name, the visible name.

**Alternatives:**
- **Wrap the row in `<a>`:** invalid HTML in a table, and the link name would become all of the row's text. Rejected.
- **`onclick` on `<tr>`:** the row can't be reached with the keyboard or announced to a screen reader. Rejected.

### D3: Hover and focus are drawn by the row and the cover

- **Hover:** the `tr` is a `group` with `hover:bg-sunk`. The name gets `group-hover:underline`. The project key chip switches from `bg-sunk` to `group-hover:bg-panel`, so it stays visible on the shaded row.
- **Focus:** the entry control drops its own outline. Its cover draws the ring instead: `focus-visible:after:ring-2 focus-visible:after:ring-inset focus-visible:after:ring-ink`. Because the cover spans the row, the ring frames the row.
- Count links and the delete button keep the default focus ring of their component, so focus on them never rings the row.

**Alternative:** an outline on the `tr` through `:has(:focus-visible)` or `:focus-within`. Rejected. Outlines on table rows render inconsistently across browsers, and `:focus-within` would also ring the row when delete has focus.

### D4: Zero counts on the desktop

`count_link` keeps its signature. With `dot == None` (desktop table) and `count == 0`, it renders only `span.sr-only` with the existing label text ("Running: 0 tasks of WEB"), with no number and no link. The phone path (`dot == Some`) is unchanged.

### D5: Details lines

- **Projects:** `forge_icon` + repository URL, then `Icon::GitBranch` (or the existing branch glyph; add it to `icons.rs` if missing) + base branch, in `font-mono text-[13px] text-muted`.
- **Model providers:** wire API labels joined with ", ", then the scheme label, then the credential (`name (secret/key)`, or "missing"). The parts are separated by `·`; each separator is `aria-hidden`. The scheme and wire APIs use the sans font, the credential uses mono, as on the board.
- **Credentials:** `secret / key` in mono.

### D6: PenPot boards and the design workflow

The three "two-line rows (1350)" boards are the reference. After the UI matches them at 1350 px, the superseded boards are deleted:
- "Settings · projects"
- "Model providers · table"
- "Credentials · table"

The rows in `docs/design-workflow.md` then point to the new boards, at viewport 1350. The other boards on these pages (modals, settings pages) are unaffected.

## Risks / Trade-offs

- **[Risk] Text under the cover can't be selected,** for example the repository URL or a credential name. → Accepted. The values are on the settings page or in the modal. Copying from the row is rejected (proposal).
- **[Risk] A near-miss on delete opens the entry instead.** → Opening is harmless. Delete keeps its 36 px hit area and its confirmation.
- **[Risk] `ui-model-provider-page` and this change both edit `providers.rs::row`.** → This change is applied after that one. If that one is not yet implemented, apply stops and says so.
- **[Trade-off] Ctrl-click on a credential row opens nothing new,** because the entry is a button that opens a modal. That matches the current edit button.
- **[Risk] `position: relative` on `<tr>`** has been supported in all evergreen browsers since 2021. → The e2e test clicks the repository URL of a row and expects the settings page. That covers the cover in a real browser.

## Migration Plan

UI-only and stateless; deploy and roll back with the control plane image. Old bookmarks are unaffected (no route changes).
