# Design system of the web UI

Reference for everything that renders in `crates/df-web`. New views (egress log, approvals inbox, audit log) are composed from the components below and need no new styling decisions. Source of truth: tokens in `crates/df-web/styles/input.css`, components in `crates/df-web/src/components/`.

## Principles

1. **Tokens, not colors.** A component never names a palette color (`bg-slate-900`). It uses a semantic token (`bg-panel`, `text-muted`). Both themes come from the same utilities.
2. **One primary action per view.** Only the primary action is filled with the accent color. Everything else is secondary or ghost.
3. **The accent is a fill, not a text color on light surfaces.** `#00ff41` has a contrast of 1.4:1 on white. Text, links and focus rings use `accent-text`; on the rail the focus ring uses `accent`, because `accent-text` on `rail` is only 2.5:1.
4. **Phone first, desktop enhanced.** Controls are at least 44 x 44 px (`min-h-11`) and 36 px from 1024 px on (`lg:`). No hover-only interactions. No page-level horizontal scrolling; wide content scrolls in its own box.
5. **Server-rendered, testable.** Components are maud functions. Each has an HTML unit test; pages are asserted as HTML.
6. **Self-contained.** Fonts, icons, scripts and styles are served by the control plane; no third-party host.

## Tokens

Defined once in `styles/input.css`: CSS variables in `:root` (light), redefined in `@media (prefers-color-scheme: dark)`, exposed as Tailwind utilities through `@theme inline` (`bg-panel`, `text-ink`, `border-line`, ...). The browser picks the theme before the first paint; there is no script and no switch in the UI.

| Token | Light | Dark | Use |
|---|---|---|---|
| `app` | `#fafaf9` | `#0c0a09` | page background |
| `panel` | `#ffffff` | `#1c1917` | columns, cards, dialogs |
| `sunk` | `#f5f5f4` | `#292524` | tool-call rows, inline code, hover |
| `raised` | `#ffffff` | `#34302d` | cards lifted off a `sunk` canvas (graph nodes); in dark lighter than `sunk`, because a shadow does not show |
| `line` | `#e7e5e4` | `#2e2a27` | borders and dividers |
| `line-strong` | `#a8a29e` | `#57534e` | input borders |
| `ink` | `#1c1917` | `#f5f5f4` | primary text |
| `muted` | `#57534e` | `#d6d3d1` | secondary text |
| `faint` | `#6b6560` | `#a8a29e` | tertiary text, keys, timestamps |
| `accent` | `#00ff41` | `#00ff41` | filled primary actions only |
| `on-accent` | `#0c0a09` | `#0c0a09` | text on `accent` |
| `accent-text` | `#00691a` | `#5cff8a` | links, active navigation, selection, focus ring |
| `accent-soft` | `#dcfce7` | `#052e16` | selected row, avatar, own messages |
| `danger` | `#b91c1c` | `#fca5a5` | error text |
| `danger-soft` | `#fee2e2` | `#450a0a` | error and notice background |
| `bubble-me` | `#e7e5e4` | `#44403c` | bubble of a board member's message |
| `bubble-agent` | `#1c1917` | `#e7e5e4` | bubble of an agent message (inverted in dark) |
| `on-bubble-agent` | `#fafaf9` | `#1c1917` | text in an agent bubble |
| `code-on-agent` | `#44403c` | `#d6d3d1` | inline code and code blocks in an agent bubble |
| `rail` | `#1c1917` | `#000000` | navigation rail |
| `rail-ink` | `#d6d3d1` | `#d6d3d1` | text on the rail |
| `rail-hover` | `#292524` | `#1c1917` | hover and current entry on the rail |
| `status-running-bg` / `-fg` | `#e0f2fe` / `#075985` | `#082f49` / `#7dd3fc` | badge: running |
| `status-waiting-bg` / `-fg` | `#fef3c7` / `#92400e` | `#451a03` / `#fcd34d` | badge: waiting |
| `status-queued-bg` / `-fg` | `#f5f5f4` / `#44403c` | `#292524` / `#d6d3d1` | badge: queued |
| `status-completed-bg` / `-fg` | `#dcfce7` / `#166534` | `#052e16` / `#86efac` | badge: completed |
| `status-failed-bg` / `-fg` | `#fee2e2` / `#991b1b` | `#450a0a` / `#fca5a5` | badge: failed |
| `status-limited-bg` / `-fg` | `#ffedd5` / `#9a3412` | `#431407` / `#fdba74` | badge: rate limited, limit reached |
| `status-review-bg` / `-fg` | `#ede9fe` / `#5b21b6` | `#4c1d95` / `#ddd6fe` | badge: in review |

### Contrast ratios

Measured with the WCAG 2.x formula; every text pair meets 4.5:1.

| Pair | Light | Dark |
|---|---|---|
| `ink` on `app` / `panel` | 16.7 / 17.5 | 18.1 / 16.0 |
| `muted` on `app` / `panel` / `sunk` | 7.3 / 7.6 / 7.0 | 13.3 / 11.7 / 10.2 |
| `faint` on `app` / `panel` / `sunk` | 5.5 / 5.7 / 5.3 | 7.8 / 6.9 / 6.0 |
| `ink` / `muted` / `faint` / `danger` on `raised` | 17.5 / 7.6 / 5.7 / 6.5 | 12.0 / 8.8 / 5.2 / 6.9 |
| `faint` on `accent-soft` | 5.2 | 5.9 |
| `accent-text` on `panel` / `accent-soft` | 6.9 / 6.3 | 13.4 / 11.4 |
| `on-accent` on `accent` | 14.5 | 14.5 |
| `danger` on `app` / `danger-soft` | 6.2 / 5.3 | 10.4 / 8.5 |
| `ink` on `bubble-me` | 13.9 | 9.4 |
| `on-bubble-agent` on `bubble-agent` / `code-on-agent` | 16.7 / 9.8 | 13.9 / 11.7 |
| `rail-ink` on `rail`; `accent` on `rail` | 11.7; 12.8 | 14.1; 15.4 |
| status `-fg` on `-bg`: running, waiting, queued, completed, failed, limited, review | 6.6, 6.4, 9.4, 6.5, 6.8, 6.4, 7.6 | 8.3, 10.4, 10.2, 10.6, 8.5, 9.3, 7.9 |

### Network traffic page

`/egress/log` (`crates/df-web/src/egress/log/`) is a split view of a connection graph and an events table, driven by one SSE stream per viewer. The page is served with the backlog already rendered (rows, graph, counts, empty state: the same state the stream's first message carries), so it shows the buffered events before the stream connects; the stream's first message replaces it. The stream is unfiltered; the server keeps the latest event per (sandbox, destination, port) edge and the counts, so graph and counts see every event while rows are filtered. Messages are `<hx-partial>`s: `#flows tbody` (rows), `#graph` (replaced, not morphed: positions are inline styles), `#count-all|blocked|allowed`, `#event-count` and `#flows-empty`; graph and counts are only sent when they changed.

- **Header and filters.** Title, a *Live* / *Paused* / *Reconnecting* badge, *Pause*/*Resume* and a link to the egress rules. The filter bar (`#filters`) sits outside `#traffic`; a change sends `hx-get` of the page with `hx-select="#traffic"`, `hx-select-oob` for the segment counts (they depend on the filter) and `hx-push-url`, so the URL is shareable and the search input keeps focus. The bar is right-aligned as in the design: *Clear filters* (only while a filter is set), search (300 px), verdict segments, *Advanced* with a count badge. *Advanced* opens an inline right-aligned row (controls as wide as their content, not `INPUT_CLASS`, which is full width): *Persona* (a select of the personas by name, value = persona id) and *Task* (task key, any case). *Rows* (100, 250, 500, 1000, 2500, 5000; default 500) at the end of the bar sets how many rows the table keeps; it is a view setting, so *Clear filters* keeps it. Parameters: `q`, `verdict` (`all|blocked|allowed`), `persona`, `task`, `rows` (only when not 500); `denied=1` (= `verdict=blocked`) and `host` (= `q`) still work, `project` and `sandbox` are ignored. *Blocked* means `Verdict::Denied`; open tunnels count as allowed.
- **Pause** aborts the stream (`_htmx.sse.abortController.abort()`, then `reader.cancel()`) and then removes the SSE connect element. Removing the element alone would leave the stream open, because htmx does not observe the removal and the extension's cleanup never runs. *Resume* inserts the element again; the backlog replaces rows, graph and counts. A filter change while paused resumes.
- **Graph.** Sandboxes (run number and task key) at `x = 20`, 176 px wide, 160 px pitch; destinations right-aligned (host icon, `host`, port chip) with a 70 px pitch; the first node of the taller column 60 px from the top and the shorter column centred on it; bezier edges in a gutter SVG with an explicit width (`calc(100% - columns)`: an SVG does not stretch between `left` and `right`), stretched horizontally only. The graph is a size container (`container-type: inline-size`) and the destination width is CSS, `clamp(120px, 100cqw - 276px, 260px)` (276 = sandbox column 196 + minimum gutter 60 + right padding 20), shared by the destination nodes, the *Destinations* heading and the SVG width, so nodes never overlap and the gutter stays at least 60 px at any panel width down to the 398 px minimum. The arrowhead is part of each destination node (one per node, `danger` when blocked), so the stretch never distorts it and the edges always end at the node. At most 12 sandboxes and 30 destinations (most recently seen), the rest is counted in a "not shown" note. Allowed edges are solid `faint`, blocked edges dashed `danger` with a `danger` destination border; nodes and edges excluded by the filters have `opacity-30`. The graph sits on the panel as in the design; the sandbox column (heading and nodes) is in a dashed group box (`border-dashed border-line-strong`, `bg-sunk/50`, 12 px around the nodes) that holds the column heading and grows only as far as the nodes need, so it moves with the centred sandboxes (as in the design, its top need not line up with the destinations heading). Nodes are `raised` cards with a `line-strong` border and a light shadow (`raised` is `panel` in light and lighter than `panel` in dark). A sandbox node is a `rounded-xl` card with an icon tile on the left (`rail` square, near-black in both themes, with a green `accent` Lucide `box` icon, `Icon::Box`) and two lines: the run number (`#2`, mono, medium) and the task key (`DF-7`, muted, truncated); an event without a run number (an older gateway) shows the short run id (8 characters) instead of `#2`, a connection from an unknown source its source IP; destination nodes use the same card. The nodes are text (the SVG is `aria-hidden`), so a screen reader gets sandbox, destination and port. Edges are drawn on the panel (non-text, needs 3:1): `faint` 5.7 (light) / 6.9 (dark), `danger` 6.5 / 9.2 (inside the sandbox box, on `sunk/50`, `faint` stays above 5.4). `line-strong` (2.5 in light) is not used for strokes.
- **Divider.** `role="separator"` with `aria-valuenow/min/max`, pointer capture drag (text selection is suppressed while dragging), arrow keys (32 px, Shift 96 px), Home/End for the limits, double click resets to 520 px. The graph panel is at least 398 px (graph minimum 396 px plus the 1 px border on both sides; at that width the destination nodes are 120 px wide and the gutter 60 px), the table keeps at least 520 px. The width is stored in `localStorage['df.traffic.graph-w']` (`try/catch`); the Alpine component sits outside `#traffic`, so the width is re-applied after every swap.
- **Events table.** At most the *Rows* limit of rows, the newest: the backlog sends only those, and after each live swap the page drops the oldest past `data-limit` on the `tbody`; the viewer mirrors this, so the close of a dropped open flow is a new row. "Events · newest first" with "N of M" (flows matching the filters of flows seen, independent of the row limit; an opening and its closing event are one flow; gaps count as neither). Columns: time (UTC), sandbox (task key and run number, `DF-7 #2`; the short run id without a number), destination with host icon, verdict pill (*Allowed*, *Blocked*, *Open*) and actions (*Details*, *Rule* for rows with a matched rule that were not blocked). The events panel is a container (`@container`): from 680 px the verdict column and the inline actions show; below, the verdict is the left edge of the row (solid green, or dashed red with a tint for blocked; the verdict stays in the text for assistive technology) and the actions are one menu button named "Actions for host:port at time". Bytes and duration are in the details only.
- **Request details.** A dialog (`role="dialog"`, closes with Escape and the close button, focus returns to the row) filled by Alpine from the row's `data-flow` JSON, which the server builds and maintains: destination and host icon, time, sandbox and task, method, status (HTTP status or verdict), duration, bytes sent and received, SNI, source IP, gateway pod, reason with a one-sentence explanation (every documented reason code has one) and the matched rule linked to `/egress/allowlist?highlight=<rule>:<port>`, otherwise "No rule matched". The panel re-reads its row after every swap, so an open tunnel that closes updates it; it closes when the row disappears. **What it cannot show:** URL, query, headers and response content type. HTTPS is tunnelled and not inspected (`design/egress.md` §7), so the data does not exist, and the panel says "Requests inside TLS tunnels are not inspected."
- **Empty states.** "No network traffic yet" (nothing seen) and "No events match these filters" with *Clear filters*.

## Type

Inter (variable, Latin) for text, JetBrains Mono (variable, Latin) for task keys, times, paths, commands and numbers. Both are `woff2` under `assets/fonts/` (SIL OFL 1.1, licence texts next to the files), served with immutable cache headers and `font-display: swap`.

| Use | Size |
|---|---|
| Metadata, badges, keys | 12-13 px (`text-xs`) |
| Body, list rows, labels | 14 px (`text-sm`) |
| Inputs (no zoom on phones) | 16 px (`text-base`) |
| Section and header titles | 16-18 px (`text-base`, `text-lg`) |
| Page titles | 20 px (`text-xl`) |

## Spacing, radius, breakpoints

- Spacing: Tailwind's 4 px scale; gaps between related controls 8 px (`gap-2`), blocks 12-16 px (`space-y-3`, `p-4`).
- Radius: inputs and buttons 8 px (`rounded-lg`), cards and dialogs 16 px (`rounded-2xl`), chips and badges full (`rounded-full`).
- Breakpoints: below 768 px phone layout (bottom bar, one screen at a time); `md` 768 px (rail, list and transcript side by side, details as a panel over the transcript, so a browser split screen keeps the desktop shell); `lg` 1024 px (smaller touch targets); `xl` 1280 px (details become a column).

## Icons

`logo(class)` renders the factory mark (Lucide `factory`) inline; `assets/logo.svg` holds the same paths for the favicon and follows the system theme (`<link rel="icon">` in `layout`). `icon(Icon::Plus)` renders an inline Lucide SVG (ISC licence, `src/components/LICENSE-Lucide.txt`) with `currentColor` and `aria-hidden`. A control that holds only an icon needs an `aria-label`. Add an icon by copying the inner elements of its Lucide SVG into `Icon::body`. `Wrench` (tool call), `BotMessage` (`bot-message-square`, a row written by the system), `Trash` (delete), `Box` (sandbox), `Server` (cluster service, IP literal), `Globe`, `Filter` (`list-filter`, the *Advanced* filters) and `Ellipsis` are Lucide icons as well (ISC).

`host_icon(host, HostIconSize::{Table, Graph, Panel})` (`components::host_icon`) is the icon of an external host in the egress pages (rules table, events table, graph nodes, details panel). Precedence: a project host, a brand monogram, the `Server` icon for IP literals, the globe. The third argument is the `ProjectHosts` lookup (`components::project_host`), built from the project catalog with `ProjectHosts::from_projects(forge, &projects)`; the egress handlers build it once per stream connection and per rules page request. A project host matches exactly (case, trailing dot and brackets ignored; no subdomains, no `*.` entries) and shows the forge icon (`forge_icon`: Forgejo, GitHub or Git; `data-host-icon="forge-forgejo"`, `forge-github`, `forge-git`) of the repository host, or the issue icon (`issue`) of a tracker on another host. `HostRole` and `project_roles` are the one rule that maps a project URL to such a role; the project form and list use it too. Otherwise the icon is a rounded square with a brand monogram from a static table (Anthropic A, GitHub GH, npm n, Go, PyPI Py, Google G) or the globe. The table lists provider domains (`anthropic.com`, `github.com`, `githubusercontent.com`, `npmjs.org`, `golang.org`, `pypi.org`, `pythonhosted.org`, `googleapis.com`); a host matches if it is the domain or a subdomain of it on a label boundary (`api.github.com` yes, `evilgithub.com` no). IP literals (IPv4, IPv6) get the `Server` icon. A wildcard entry (`*.github.com`) uses its suffix. The tones are status tokens (so both themes work); only letters are drawn, no logos. The icon is decorative: the host is always written next to it.

## Components

All in `df_web::components`; signatures abbreviated.

### Actions

- `ButtonKind::{Primary, Secondary, Ghost}`: filled with the accent, outlined, text only.
- `button(kind, label)`, `submit_button(kind, label)`, `link_button(kind, href, label)`: a `type=button` button, a submit button, a link that looks like a button.
- `action_class(kind)`: the classes, for elements that need their own attributes (`hx-*`, `x-*`).
- `ICON_BUTTON_CLASS`: square 44 px (36 px from 1024 px) icon-only control; needs `aria-label`.
- `ROUND_ACTION_CLASS` / `ROUND_PILL_CLASS`: the round accent action inside a message box or card: an icon in a 44 px circle (Send, Create task; needs `aria-label`) and the pill with a word (Save). Use them instead of repeating the classes.

Do: one primary action per view; give icon buttons an `aria-label`. Don't: use `Primary` for navigation or dismissal.

### Badges and chips

- `Tone::{Running, Waiting, Queued, Completed, Failed, Limited, Review}`: color family (`Review` for `in_review`); `Tone::of_status(TaskStatus)`, `Tone::of_run(RunState)`.
- `status_badge(TaskStatus)`: the status name in its tone. `tone_badge(tone, label)`: any label in a tone.
- `chip(label)`: neutral label, for project keys and similar.
- `filter_chip(label, href, active)`: a filter choice; the current one has `aria-current` and the accent-soft style.

Do: derive the badge from the status. Don't: invent a color for a new status; map it to a tone.

### Forms

- `FormErrors`: validation errors by field; the empty name is a message for the whole form.
- `field(label, name, errors, hint, control)`, `text_field`, `textarea_field`, `select_field`: labelled controls with hint (`<name>-hint`) and error (`<name>-error`, `role=alert`); the control references both with `aria-describedby` and sets `aria-invalid` while the field has an error. `INPUT_CLASS` is the shared control style.
- `readonly_field(label, name, value, errors, hint)`: a value that is shown, submitted and linked to its hint and error but cannot be changed (the project key after creation).
- `form_error(errors)`: the form-wide message. `field_error(Option<&str>)`: the error of a field or input group.
- `search_menu(button_label, items)` with `MenuItem { href, label, hint, current }`: a button that opens a list of links with a search field (Alpine; filtering happens in the browser over the rendered entries).

Do: render the submitted values again with the errors (HTTP 422, htmx swaps it). Don't: validate in the browser only.

### Overlay

- `dialog(id, title, body)`: native `<dialog>` that opens itself (`showModal()`: focus trap and Escape), removes itself on close and is a bottom sheet below 768 px. A click on the backdrop closes it. `dialog_with(id, title, style, body)` takes a `DialogStyle`: `Sheet` (the default), `Form` (a full-height sheet below 768 px, for forms with many fields) and `Bare` (no frame: the body is the card, as in the new-task composer; the body adds `dialog_close()` itself). `dialog_close()` is a plain close button that may sit inside the dialog's own form. A `modal-close` event on the window (`HX-Trigger`) closes the open dialog.
- `details_panel(title, body)`: the details of the workspace: bottom sheet on phones, slide-over from 768 px, permanent column from 1280 px. Expects the workspace Alpine state in a parent: `WORKSPACE_STATE` in `task_pages.rs` documents `details`, `collapsed`, `sheet`, `dragging`, the named thresholds (`closeBelow`, `fullAbove`, `dragSlop`) and the methods (`setCollapsed`, `sheetToggle`, `sheetDrag*`). The header has the heading on the left and the controls grouped on the right: "Close details" below 1280 px, "Hide details" from 1280 px (no display utility on the header: the stylesheet hides it when collapsed). "Hide details" collapses the column to a 4 rem **strip** that shows only: a "Show details" button, the status dot and icon links to the pull request and issue (name and tooltip as `aria-label` / `title`). The strip is rendered by the live `#task-details` region (`ws-strip`); what it hides is marked `ws-details-full`. The choice is kept in `localStorage['df.details.collapsed']` (read and written in `try/catch`) and applies to all tasks.

**Phone sheet.** Below 768 px the details are a bottom sheet with an 18 px top radius; handle and header stay in view and only the body scrolls, and the sheet does not animate while it is dragged (`data-dragging`). A grey handle (40 x 5 px, `line` token, 8 px from the top) is a real button, "Resize details" (`aria-expanded` while full height). The sheet opens at half the viewport height; dragging the handle up makes it 90 % high, dragging down returns to half, or closes it when released below 25 % of the viewport. A tap or the keyboard toggles half and full. Escape, the backdrop and "Close details" close it. The drag is a small Alpine pointer handler (`setPointerCapture`, no library); the height transition runs only under `prefers-reduced-motion: no-preference`. Without pointer support the sheet keeps its half height. From 768 px nothing changes. The phone bottom bar hides on the transcript screen through a variant on the bar itself (`[body[data-view=task]_&]:hidden`), because a stylesheet rule in the components layer loses against its `flex` utility; the same applies to any element that mixes a display utility with a state rule.

Do: put the dialog's trigger target in `#composer-slot` (htmx `hx-get`, `innerHTML`). Don't: nest a `<form method="dialog">` in a form; use `dialog_close()`.

### Tables

`data_table(head, rows)` with `th(Cell, …)`, `td(Cell, …)` and `tr(…)` (`components::table`) is the one table of the configuration lists (Projects, Credentials; PenPot: Config / "Projects · table with task counters", "Credentials · table"): a card (`rounded-xl`, border, `panel`), an uppercase `faint` header row, `px-5` first cells and `px-3` others, `py-3.5` rows with a top border. `Cell` is `First` (the name), `Text`, `Mono` (URLs, branches, keys), `Number` (right-aligned counters) or `Actions` (the edit button). The card has `min-w-0 max-w-full overflow-x-auto`: a wide table scrolls inside it and never widens the page; its parents must be able to shrink (`min-w-0`).

### Configuration views

The project, persona and credential lists share one shape: a `#config-list` region that holds the `page_header` (title, count, create button, phone profile menu on Projects) and the list, so a save updates the count with the list.

- **Form layout.** A modal form is a two-column grid from 768 px and one column below (`form_grid`, `full_row`): project key and name share a row, repository URL, issue tracker URL and image take full rows, base branch and forge credential share a row; persona harness and model share a row; credential fields stack. The footer sits at the bottom right: Cancel (`type=button`, closes the dialog) and the primary action, "Create project" / "Save project" (likewise "persona" and "credential reference"). The project key is typed at creation and read-only afterwards (muted field, hint "The key cannot change after creation."); an edit request with another key is refused with 422.
- **Modal forms.** Create and edit buttons `hx-get` a form fragment (`/projects/new`, `/projects/{key}/edit`, same for personas and credentials) into `#composer-slot`. The fragment is a `dialog_with(…, DialogStyle::Form, …)`: a centered card from 768 px, a full-height sheet below. A 422 replaces only the `#form` inside the open modal (values kept, error at the field). A successful save answers with the updated list region and the headers `HX-Retarget: #config-list`, `HX-Reswap: outerHTML` and `HX-Trigger: modal-close`, so the list updates and the modal closes without a page load. A plain browser request for a fragment route (no `HX-Request`) redirects to the list; there are no full-page form routes.
- **Project table.** The page uses the full content width (padding 32 / 40 px from 768 px); the header reads "Projects <count>" with the New project button. From 768 px a table card: Project (key chip and name), Repository (forge icon and URL), Branch and the counters Queued / Running (running and rate limited) / Waiting (waiting and limit reached) / Done (completed and failed), each with a status dot in the header, and an edit button. Each count is a right-aligned link to the task list of the project with exactly those statuses (`status=queued|running|waiting|done`); zeros are muted (`faint`). Below 768 px the same data is a compact list (PenPot: Config / "Phone · projects"): per row the key chip, the name and a "Tasks ›" link on top, the forge icon next to the repository URL (mono, truncated), the counts as `<dot> <number> <state>` links with lower-case states, and the edit button as a full-height column at the right. The table scrolls inside its card, never the page.
- **Forge icon.** `forge_icon(Option<&ForgeKind>)`: Forgejo, GitHub or generic Git, named by `aria-label` and `title`. The kind is the integration the instance runs (`WebState.forge`), shown for projects that have a forge credential; never guessed from the host. Licences: the Forgejo logo is the one on the design canvas (viewBox 212, orange and red strokes, so it does not follow the text color), Forgejo logo (c) Forgejo contributors, CC BY-SA 4.0, https://forgejo.org, shown only to identify the forge; the GitHub mark (CC0, simple-icons) is used only to identify GitHub as the forge next to a repository; the generic Git glyph is Lucide (ISC).
- **Forge feedback in the project form.** The Repository URL and Issue tracker URL fields hold their icon inside the field. Typing (`keyup changed delay:300ms`) fetches `GET /projects/forge` and swaps three htmx partials: the two icons and the one-line hint (`#forge-hint`, `aria-live="polite"`). A valid HTTPS repository URL (owner and repository segments) shows the integration and names it in the hint; anything else shows the generic icon and "No forge integration applies". The issue tracker shows the forge icon on the repository's host and a plain tracker icon elsewhere.
- **Persona card.** One card per persona: initial avatar, name, "Claude Code · model" (or "default"), a three-line excerpt of the system prompt (`line-clamp-3`), the credential reference name (or "none") and the edit button (`aria-label="Edit persona <name>"`).
- **Empty task list and workspace** (`EmptyPanel`). The list says "No active tasks" (the filter's name) with one sentence about what shows up there and a "Show all tasks" text link; the workspace without a task shows a round accent-soft icon, "No task selected", a sentence and the primary New task button with a plus.
- **Empty lists.** `empty_panel` with a sentence on what the item is for and the create button; the empty project list also links to the credential references.

Do: render the list page and the save answer from the same region function. Don't: link to a form page.

### Lists

- `section(title, body)`: titled block of a panel: uppercase faint 12 px header, 16 / 20 px padding and a bottom border. `definition_list(&[(label, value)])`: label and value pairs in a grid with a 76 px label column (faint 13 px labels, left-aligned medium values).
- `empty_state(text)`: what an empty list says. `empty_panel(title, text, action)` and the fuller `EmptyPanel { size, icon, title, text, phone_hint, action }` (sizes `Page`, `List`, `Workspace`): the empty state of a list or column: an optional round icon, a title ("No active tasks", "No task selected"), at most one sentence on what the thing is for (plus a phone-only hint) and its one action (a "Show all tasks" text link, a "New task" button).

### Transcript

`components::transcript`: `user_message`, `agent_message(id, author, at, body, markdown)`, `thought`, `run_divider`, `queued_message`, `tool_row(&ToolRow)`, `code_block`, `plan`, `turn_end`, `notice`, `unreadable`. Each takes the DOM id that live updates target. All text is escaped; Markdown is rendered without raw HTML (`crate::transcript::markdown`).

The transcript reads as a conversation:

- **Bubble.** A board member's message is a right-aligned bubble on `bubble-me`, the agent's a left-aligned bubble on `bubble-agent` with `on-bubble-agent` text (dark in the light theme, light in the dark one). Name and time (`Peter · 10:42`, UTC) sit under the bubble. Inline code and code blocks in an agent bubble use `code-on-agent`; links keep the bubble's text color.
- **Avatar.** Every message renders its avatar (`avatar(initials)`, class `avatar`). Items carry `data-author="human|agent"` (tool rows and thoughts count as `agent`); CSS hides the avatar of an item when the next sibling has the same author, so only the last message of a group shows it and live appends regroup without re-rendering. Without `:has()` every message keeps its avatar.
- **Tool row.** One line indented to the bubble: the Lucide `wrench` in front (muted, decorative), title, kind, status and a `ChevronDown` at the end that turns 180° when the row is open; expandable to input and output. Every tool row has a thin `line` border above and below; a tool or notice row directly after another drops its top border, so consecutive rows share one line (CSS sibling rule on `[data-tool]` and `[data-system]`). Thoughts and plans are no tool rows and get no line.
- **Notice row.** `notice(id, level, code, message)` is a `details` like the tool row (`ml-11`, `data-author="agent"`, `data-system`): the Lucide `bot-message-square` in front (muted; the same for every level, it marks the source), the code in mono tinted by level (`danger` for error, `status-waiting-fg` for warning, `muted` otherwise), the message truncated and a `ChevronDown` that turns when open. Opening the row shows the full message. No background box.
- **Hidden block.** A content block marked `hidden` (an instruction for the agent) is not shown unless the instance option `web.showHiddenBlocks` is set. Then `user_message` shows it below the visible text in the bubble: a `line` rule above, `muted` text and the `faint` label "Instruction for the agent" (`text-xs`). The queued stack shows it inline in the single-line row, `muted` and with the same label (`data-hidden`, no rule above); a queued message of only hidden blocks has no row and is not counted unless the option is set.
- **Run divider.** `run_divider(id, number, started_at)`: a rule with "Run #2 started · Oct 23 10:20" in front of the first item of each run.
- **Queued stack.** `pending_region` renders the `#pending` stack above the message input, oldest first: a header "N queued · <persona> reads them at the next turn" ("N queued · sent with the next run" on a `failed` or `completed` task) and a list capped at three rows that scrolls inside itself, so the input keeps its place (board "Desktop · many queued messages"). Each message is a row, `queued_message(key, id, text, queued_at, QueuedState)`: a single line (`h-11 lg:h-9`, text truncated with the full text in `title`, queue time and buttons inline). `QueuedState` is `Editable`, `WaitingForDelivery` or `NotDelivered`. Held-back messages offer "Edit queued message" (loads the text into the input) and "Remove queued message" (`hx-delete`); forwarded ones offer neither and say where they stand: "waiting for delivery", or "not delivered, sent with the next run" on a `failed` or `completed` task (the next run sends them again). The row has a dashed `line-strong` border, a 10 px radius and the app background; it is highlighted (accent-soft) while it is being edited.
- **Message input.** One rounded box holds the text and the round Send button ("Send"; "Save" while editing), as on the canvas.
- **Details overview.** Sections in this order, each with the uppercase header: Now (status dot and the status line, or the status label), Links (PR and issue cards, or "No pull request or issue yet."), the status / project / persona (with its harness, muted) / branch list, Description, Runs. The **Status** row is a select like the project filter (plain trigger with the status name and a chevron; menu with a check on the current status and "Mark done", no chips); a status that does not allow marking done shows the plain status name instead. Board: "Desktop · in review" (menu open). A run without recorded usage (finished or still running) shows "<model or default model> · no token usage recorded" instead of the token table. Runs is the last section and has no bottom border. Runs is a disclosure (`<details>`, closed by default, not persisted) whose summary is the section header with a chevron (right when closed, down when open) and the run count ("Runs 2"); the runs are listed newest first. From 768 px only the runs list scrolls: the sections above stay fixed, the open `<details>` fills the remaining column height and its header stays in view; the Description text is cut after three lines with the full text in `title`. Boards: "Desktop · runs collapsed" (closed), "Desktop · light" (open) and "Desktop · many runs" (scrolling list). The Description is a disclosure whose summary looks like the other section headers.
- **List row.** The selected task has a 3 px accent edge and the accent-soft background, the others a transparent edge (nothing shifts); title 15 px medium on one line.
- **Run card.** In the details (each card is an `li` with the id `run-card-<run id>`, so morphing keeps an open disclosure with its run): `#n` as a link to the run divider in the transcript (`#run-n`; `accent-text`, underline on hover), status badge, start time on the right, one line of the failure reason (longer or multi-line text: the full text in a `<details>` disclosure) and the token table per model: the model name on its own line (truncated, full name in `title`), below it the columns in / out / cache (read / write) with right-aligned compact counts (`9.4k`, `182k`, `1.2M`; the exact value in `title`). A run without recorded usage shows the muted line "<model or default model> · no token usage recorded" instead of the table.
- **Editing bar.** While a queued message is edited, the message input shows "Editing a queued message" with Cancel (restores the earlier draft) and Save replaces Send (`hx-patch`). If the message was forwarded meanwhile, the answer (409) keeps the text in the input and shows "The agent has already received this message." above the stack.

Do: keep the stack and the input in one Alpine scope (`draft`, `saved`, `editing`). Don't: show queued messages inside the transcript.

### Shell

- `rail(active, compose_href)`: navigation rail from 768 px (sections: Tasks, Projects, Personas, Credentials, "Egress rules" at `/egress/allowlist` and "Network traffic" at `/egress/log`). First the logo (link to `/tasks`, name "Dark Factory"), then the accent new-task button, then icon-only links to the sections. A section's name is its `aria-label` and `title` (tooltip); there is no visible label. The current section has `aria-current="page"` and a bar at the rail's edge. Log out is the last, icon-only button.
- `bottom_bar(active, &CreateAction)`: below 768 px: Tasks, the screen's create action and Projects. `CreateAction { label, href }` creates what the screen lists: "New project" on Projects, "New task" elsewhere (`layout` derives it from `Nav`). Personas and Credentials are not part of it and Log out moved to the profile menu. The bar is hidden while a transcript is shown (`body[data-view=task]`).
- `profile_menu(display_name)`: phones only (`md:hidden`, below 768 px), top right of the task list and Projects: the avatar with the user's initials (`aria-label="Account menu of …"`) opens a menu with name, role ("board member") and Log out (`hx-post="/logout"`).
- `page_header(title, count, actions)`: the title row of a page, "Projects 3" with the number of entries when `count` is given, and the page's actions on the right. `avatar(initials)`, `initials(name)`.
- `layout(title, nav, body)` is the frame of a page outside the task workspace: it fills the content width (padding 16 px, 32 / 40 px from 768 px) and has no maximum width; the task workspace uses the full width too.

## Patterns

- **Page shell.** Rail (desktop) or bottom bar (phone) around `main`. Pages outside the navigation (errors) use `Nav::None`.
- **Three-column workspace.** List, transcript, details (`/tasks`, `/tasks/{key}`, one template). `body[data-view=list|task]` says which screen a phone shows. Selecting a task is an `hx-get` of the page with `hx-select="#ws-detail"` and `hx-push-url`: the list and its stream stay untouched, the old detail stream closes with the replaced region.
- **List and detail on phones.** The list is a screen of its own; the transcript has a back link and the details open as a bottom sheet.
- **Composer details.** The project chip and the filter menu trigger are 10 px rounded rectangles; the assignee picker has no border; Create task uses the arrow-up icon; the text has no outline and the card shows focus with a stronger neutral border (`focus-within:border-line-strong`, no accent; the transcript message box does the same, both are `border-line` at rest); the close button has no hover background.
- **New-task composer.** A `Bare` dialog: no frame, the project chip and a linked-issue chip above a rounded card that holds the description, a round `+` button on the left ("Link issue", `aria-expanded`, accent border and soft fill while open) that opens a 280 px popover below the card (label "Linked issue", a mono `#` prefix and the numeric field `issue_number`, a help sentence naming the selected project; Escape and a click outside close it; inline below the card on phones, where the bottom sheet leaves no room) and, once a number is entered, an "Issue #n" chip above the card with a remove button, the assignee picker next to the round submit button ("Create task"); there is no "Options" text.
- **Sheets and dialogs.** A modal task (composer) is a `dialog`; context that belongs to a view (details) is a `details_panel`.
- **Forms and validation.** `hx-post` on the form with `hx-target="this"` (or `#form`) and `outerHTML`; failure answers 422 with the form. Success answers `HX-Redirect` (the composer opens the new task) or, for configuration forms in a modal, the updated list with `HX-Retarget` and `HX-Trigger: modal-close`.
- **Empty and loading states.** `empty_panel("No active tasks", …)` with its action; there are no spinners because pages are server-rendered and updates stream in.
- **Live updates.** Regions with a stable id (`#task-rows`, `#task-head`, `#task-details`, `#runs`, `#pending`, `#transcript`) are replaced over SSE with `outerMorph`; the morph ignores `open`, `style` and `aria-current` so client state survives.

## Review checklist for a new view

- [ ] Built from components; no raw palette color, no hand-written hex value.
- [ ] Looks right in light and dark (switch the system setting).
- [ ] Usable at 390 px: no horizontal page scroll, targets of 44 px, nothing hover-only.
- [ ] One primary action; icon-only controls have an `aria-label`.
- [ ] Keyboard: every action reachable, focus ring visible, dialogs close with Escape.
- [ ] Text contrast of any new token pair is at least 4.5:1 and listed above.
- [ ] Forms re-render with values and errors on 422.
- [ ] HTML unit test for new components; page test for the view.
- [ ] No third-party host (fonts, icons, scripts).
