# Design

## Context

`df-web` renders server-side with maud, swaps with htmx 4, uses Alpine for small client state and Tailwind v4 (standalone binary, ADR-0037) for styles. Live updates use SSE (`/tasks/events`, `/tasks/{key}/events`). Today every page is a full page; shared markup is limited to `layout()` and a few helpers in `ui.rs` (`text_field`, `select_field`, `status_badge`, …). Colors are hard-coded Tailwind palette classes for one dark theme.

The target look is the mockup on the design canvas "Dark Factory UI redesign". Requirements: `specs/web-ui-shell`, `specs/task-ui`, `specs/task-management`, `specs/factory-tools`.

## Goals / Non-Goals

**Goals:**
- One set of semantic tokens drives both themes; a component never names a raw palette color.
- Pages are composed from documented components, so a new view (egress log, approvals inbox) needs no new styling decisions.
- Everything stays testable without a browser: components and pages are asserted as HTML in unit tests.

**Non-Goals:**
- A theme switch in the UI (the system setting decides), user preferences, custom accent colors.
- The minion (model-generated titles and summaries); this change only reserves the `generated` title source.
- Search over tasks or transcripts (roadmap iteration 5), a component gallery page.

## Decisions

### D1 — Tailwind stays, with semantic tokens as CSS variables
Tokens are defined once in `styles/input.css` with Tailwind v4 `@theme` and point to CSS variables; `@media (prefers-color-scheme: dark)` redefines the variables. Utilities like `bg-panel`, `text-ink`, `border-line` then switch theme without `dark:` variants.

| Token | Light | Dark | Use |
|---|---|---|---|
| `app` | `#fafaf9` | `#0c0a09` | page background |
| `panel` | `#ffffff` | `#1c1917` | columns, cards, dialogs |
| `sunk` | `#f5f5f4` | `#292524` | tool-call rows, inline code, hover |
| `line` / `line-strong` | `#e7e5e4` / `#a8a29e` | `#2e2a27` / `#57534e` | borders |
| `ink` / `muted` / `faint` | `#1c1917` / `#57534e` / `#6b6560` | `#f5f5f4` / `#d6d3d1` / `#a8a29e` | text levels |
| `accent` / `on-accent` | `#00ff41` / `#0c0a09` | same | filled primary actions only |
| `accent-text` | `#00691a` | `#5cff8a` | links, active nav, selection, focus ring |
| `accent-soft` | `#dcfce7` | `#052e16` | avatar and selection backgrounds |
| `rail` | `#1c1917` | `#000000` | navigation rail |
| `status-*` | per status, bg + fg | per status | badges (running, waiting, queued, completed, failed, limited) |

`#00ff41` is never used as text on a light background (1.3:1); it is a fill with black text (15:1) or text on dark surfaces. Every text pair in the table meets 4.5:1; `docs/design-system.md` lists the measured ratios.

*Alternatives:* `dark:` variants on every element (doubles the classes, easy to forget one); hand-written CSS without Tailwind (loses the utility workflow ADR-0037 chose; the generated CSS is 14 KB, so size is no argument).

### D2 — Components are maud functions in `df-web::components`
`ui.rs` becomes the module `components/` with one file per group: `actions` (button, icon button), `badges` (status badge, chip), `forms` (field, select, textarea, menu with search), `overlay` (dialog, sheet, slide-over panel), `lists` (list row, definition list, section), `transcript` (message, tool-call row, pending message), `shell` (rail, bottom bar, page header, avatar). Components take typed arguments (`ButtonKind::{Primary, Secondary, Ghost}`, `Size`, `TaskStatus`) instead of class strings; call sites add only layout utilities (spacing, grid placement). Each component has an HTML unit test; markup that changes on purpose updates an `insta` snapshot.

Interactive state (menus, sheets, dialogs, inline edit) uses Alpine with the same small set of patterns, documented with the component. Dialogs use the native `<dialog>` element (`showModal()`), which provides focus trapping and Escape for free.

*Alternative:* a template engine with partials — maud is already the convention and gives compile-time checked markup.

### D3 — Icons and fonts are vendored
Icons: a Lucide subset (ISC licence) as inline SVG via `icon(Icon::Plus)`, `stroke="currentColor"`, `aria-hidden` unless labelled. Fonts: Inter and JetBrains Mono variable `woff2` (SIL OFL 1.1, Latin subset) under `assets/fonts/`, served with the existing hashed asset URLs and `font-display: swap`. Licence texts go next to the files.
*Alternative:* Google Fonts or an icon CDN — violates "self-contained assets" and leaks visits to third parties.

### D4 — One workspace template for `/tasks` and `/tasks/{key}`
Both routes render the same page: rail, list column, transcript column, details column. The `<body>` carries `data-view="list|task"`; CSS decides per breakpoint which columns are visible (1280+: all three; 1024–1279: list and transcript, details as slide-over; below 1024: list or transcript). Selecting a task is an `hx-get` of `/tasks/{key}` with `hx-select` of the transcript and details regions and `hx-push-url`, so the list and its SSE stream stay untouched. The transcript and details regions carry their own `hx-sse:connect` for the selected task; replacing them closes the old stream and opens the new one. A direct load of `/tasks/{key}` still returns the full page (phones, bookmarks, reloads).

*Alternative:* client-side routing — needs a JS app; the htmx swap keeps server rendering and the existing SSE handlers.

### D5 — Filters in the URL, project search on the client
`/tasks?status=active|waiting|done|all&project=KEY` (defaults `active`, all projects). Status groups map in `df-core` (`TaskStatus::group()`), so list query, SSE stream and UI share one definition. `/tasks/events` takes the same parameters and filters before rendering rows. The project menu renders all projects with task counts; its search filters the rendered entries with Alpine (instances have tens of projects, not thousands).

### D6 — Composer as a lazily loaded dialog
The new-task action does `hx-get /tasks/compose`, which returns the composer (projects, personas, current filter as default project) into a `<dialog>`; on phones the same markup is styled as a bottom sheet. `POST /tasks` creates the task and answers with `HX-Redirect: /tasks/{key}`, or with the composer and an error (status 422). The last used project and persona are remembered in `localStorage` as defaults. `Ctrl/Cmd+Enter` submits, Escape closes. `/tasks/new` is removed.

### D7 — Title sources in one conditional update
New column `tasks.title_source TEXT NOT NULL CHECK (title_source IN ('placeholder','agent','generated','user'))`; existing rows get `user`. `df-db` gets one write path, `set_task_title(task, title, source)`, whose `UPDATE … WHERE` encodes the precedence (`user` always; `generated`/`agent` only over `placeholder`, `generated` also over `agent`), so concurrent writers cannot violate it. Sanitizing (control characters and newlines to spaces, collapse whitespace, trim, 80 characters on a character boundary) lives in `df-core` and is used by the tool, the rename handler and the future minion. The placeholder is the description up to the first sentence end (`. `, `! `, `? ` or line break), cut to 80 characters at a word boundary with `…`.

`set_task_title` is added to `contract/mcp/factory-tools.v1.json` and executed in `df-agent-channel` like `report_progress` (stored call, audit, `task_changed`). The assignment prompt (built where the task is created) contains the description and one sentence asking the persona to call `set_task_title` first; tasks with a user title keep today's prompt.

The default PR title (`<KEY>: <title>`) keeps working: the first PR opens after the first push, by which time the persona has usually set a title; otherwise the placeholder is used.

### D8 — Rename inline in the transcript header
The title in the header is a button; activating it swaps in an input (Alpine), Enter submits `POST /tasks/{key}/title`, Escape cancels. The handler sanitizes, stores with source `user`, audits `task.rename` and publishes `task_changed`, which updates the list through the existing stream.

### D9 — The design system document
`docs/design-system.md` is the reference new views follow: principles, token table with contrast ratios, type scale (Inter 13/14/15/16/20/22, JetBrains Mono for keys, times, paths, commands and numbers), spacing and radius scale, breakpoints, each component with its maud signature, variants, states and do/don't, and patterns (page shell, three-column workspace, list–detail, sheets and dialogs, forms and validation, empty and loading states, live updates). A short checklist for reviewing a new view closes it. `crates/df-web/README.md` links it.

## Risks / Trade-offs

- [htmx partial swaps with SSE regions behave differently than expected] → the selection flow gets HTML tests for the swapped regions and an SSE test that a replaced region's stream is closed; manual check in a browser before archiving.
- [Agents ignore the instruction to call `set_task_title`] → the placeholder stays readable; the board member can rename; the minion change adds a second source.
- [A title is agent input shown in the UI and used as PR title] → sanitized, length-limited, rendered escaped; PR creation already treats the title as plain text.
- [Three `iter2-*` changes modify `task-ui` at the same time] → their UI parts must use the components; whichever change archives second rebases its delta on the then-current `task-ui` spec.
- [Fonts add ~300 KB on first load] → Latin subset, hashed URLs with long cache headers, `font-display: swap` so text shows immediately.
- [Restyling the configuration pages touches code without behavior change] → existing handler tests stay unchanged and must keep passing.

## Migration Plan

1. Migration adds `title_source` with default `user` for existing rows, then drops the default. Rollback: the column can stay; older code ignores it.
2. Deploy control plane and runtime contract together: `set_task_title` is only offered in `Assign.tools` by the new control plane; an older runtime ignores unknown tools.

## Open Questions

- A keyboard shortcut to open the composer (`C`) — can be added later without changing the design.
- Empty states (no tasks, no projects yet): the components exist; the copy is decided during implementation.
