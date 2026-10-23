# Design system of the web UI

Reference for everything that renders in `crates/df-web`. New views (egress log, approvals inbox, audit log) are composed from the components below and need no new styling decisions. Source of truth: tokens in `crates/df-web/styles/input.css`, components in `crates/df-web/src/components/`.

## Principles

1. **Tokens, not colors.** A component never names a palette color (`bg-slate-900`). It uses a semantic token (`bg-panel`, `text-muted`). Both themes come from the same utilities.
2. **One primary action per view.** Only the primary action is filled with the accent color. Everything else is secondary or ghost.
3. **The accent is a fill, not a text color on light surfaces.** `#00ff41` has a contrast of 1.4:1 on white. Text, links and focus rings use `accent-text`.
4. **Phone first, desktop enhanced.** Controls are at least 44 x 44 px below 1024 px (`min-h-11`), 36 px from there on. No hover-only interactions. No page-level horizontal scrolling; wide content scrolls in its own box.
5. **Server-rendered, testable.** Components are maud functions. Each has an HTML unit test; pages are asserted as HTML.
6. **Self-contained.** Fonts, icons, scripts and styles are served by the control plane; no third-party host.

## Tokens

Defined once in `styles/input.css`: CSS variables in `:root` (light), redefined in `@media (prefers-color-scheme: dark)`, exposed as Tailwind utilities through `@theme inline` (`bg-panel`, `text-ink`, `border-line`, ...). The browser picks the theme before the first paint; there is no script and no switch in the UI.

| Token | Light | Dark | Use |
|---|---|---|---|
| `app` | `#fafaf9` | `#0c0a09` | page background |
| `panel` | `#ffffff` | `#1c1917` | columns, cards, dialogs |
| `sunk` | `#f5f5f4` | `#292524` | tool-call rows, inline code, hover |
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
| `rail` | `#1c1917` | `#000000` | navigation rail |
| `rail-ink` | `#d6d3d1` | `#d6d3d1` | text on the rail |
| `rail-hover` | `#292524` | `#1c1917` | hover and current entry on the rail |
| `status-running-bg` / `-fg` | `#e0f2fe` / `#075985` | `#082f49` / `#7dd3fc` | badge: running |
| `status-waiting-bg` / `-fg` | `#fef3c7` / `#92400e` | `#451a03` / `#fcd34d` | badge: waiting |
| `status-queued-bg` / `-fg` | `#f5f5f4` / `#44403c` | `#292524` / `#d6d3d1` | badge: queued |
| `status-completed-bg` / `-fg` | `#dcfce7` / `#166534` | `#052e16` / `#86efac` | badge: completed |
| `status-failed-bg` / `-fg` | `#fee2e2` / `#991b1b` | `#450a0a` / `#fca5a5` | badge: failed |
| `status-limited-bg` / `-fg` | `#ffedd5` / `#9a3412` | `#431407` / `#fdba74` | badge: rate limited, limit reached |

### Contrast ratios

Measured with the WCAG 2.x formula; every text pair meets 4.5:1.

| Pair | Light | Dark |
|---|---|---|
| `ink` on `app` / `panel` | 16.7 / 17.5 | 18.1 / 16.0 |
| `muted` on `app` / `panel` / `sunk` | 7.3 / 7.6 / 7.0 | 13.3 / 11.7 / 10.2 |
| `faint` on `app` / `panel` / `sunk` | 5.5 / 5.7 / 5.3 | 7.8 / 6.9 / 6.0 |
| `faint` on `accent-soft` | 5.2 | 5.9 |
| `accent-text` on `panel` / `accent-soft` | 6.9 / 6.3 | 13.4 / 11.4 |
| `on-accent` on `accent` | 14.5 | 14.5 |
| `danger` on `app` / `danger-soft` | 6.2 / 5.3 | 10.4 / 8.5 |
| `rail-ink` on `rail`; `accent` on `rail` | 11.7; 12.8 | 14.1; 15.4 |
| status `-fg` on `-bg`: running, waiting, queued, completed, failed, limited | 6.6, 6.4, 9.4, 6.5, 6.8, 6.4 | 8.3, 10.4, 10.2, 10.6, 8.5, 9.3 |

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
- Breakpoints: below 1024 px phone layout (bottom bar, one screen at a time); `lg` 1024 px (rail, list and transcript); `xl` 1280 px (details become a column).

## Icons

`icon(Icon::Plus)` renders an inline Lucide SVG (ISC licence, `src/components/LICENSE-Lucide.txt`) with `currentColor` and `aria-hidden`. A control that holds only an icon needs an `aria-label`. Add an icon by copying the inner elements of its Lucide SVG into `Icon::body`.

## Components

All in `df_web::components`; signatures abbreviated.

### Actions

- `ButtonKind::{Primary, Secondary, Ghost}`: filled with the accent, outlined, text only.
- `button(kind, label)`, `submit_button(kind, label)`, `link_button(kind, href, label)`: a `type=button` button, a submit button, a link that looks like a button.
- `action_class(kind)`: the classes, for elements that need their own attributes (`hx-*`, `x-*`).
- `ICON_BUTTON_CLASS`: square 44 px (36 px from 1024 px) icon-only control; needs `aria-label`.

Do: one primary action per view; give icon buttons an `aria-label`. Don't: use `Primary` for navigation or dismissal.

### Badges and chips

- `Tone::{Running, Waiting, Queued, Completed, Failed, Limited}`: color family; `Tone::of_status(TaskStatus)`, `Tone::of_run(RunState)`.
- `status_badge(TaskStatus)`: the status name in its tone. `tone_badge(tone, label)`: any label in a tone.
- `chip(label)`: neutral label, for project keys and similar.
- `filter_chip(label, href, active)`: a filter choice; the current one has `aria-current` and the accent-soft style.

Do: derive the badge from the status. Don't: invent a color for a new status; map it to a tone.

### Forms

- `FormErrors`: validation errors by field; the empty name is a message for the whole form.
- `field(label, name, errors, hint, control)`, `text_field`, `textarea_field`, `select_field`: labelled controls with hint and error (`role=alert`). `INPUT_CLASS` is the shared control style.
- `form_error(errors)`: the form-wide message. `field_error(Option<&str>)`: the error of a field or input group.
- `search_menu(button_label, items)` with `MenuItem { href, label, hint, current }`: a button that opens a list of links with a search field (Alpine; filtering happens in the browser over the rendered entries).

Do: render the submitted values again with the errors (HTTP 422, htmx swaps it). Don't: validate in the browser only.

### Overlay

- `dialog(id, title, body)`: native `<dialog>` that opens itself (`showModal()`: focus trap and Escape), removes itself on close and is a bottom sheet below 1024 px.
- `details_panel(title, body)`: the details of the workspace: bottom sheet on phones, slide-over from 1024 px, permanent column from 1280 px. Expects Alpine state `details` in a parent.

### Lists

- `section(title, body)`: titled block of a panel. `definition_list(&[(label, value)])`: label and value pairs.
- `empty_state(text)`: what an empty list says. `scroll_box(content)`: wide content (tables) scrolls inside it.

### Transcript

`components::transcript`: `user_message`, `agent_message(id, body, markdown)`, `thought`, `pending_message`, `tool_row(&ToolRow)`, `code_block`, `plan`, `turn_end`, `notice`, `unreadable`. Each takes the DOM id that live updates target. All text is escaped; Markdown is rendered without raw HTML (`crate::transcript::markdown`).

### Shell

- `rail(active, compose_href)`: navigation rail from 1024 px with the new-task action, sections and log out; `aria-current="page"` on the current section.
- `bottom_bar(active, compose_href)`: below 1024 px: Tasks, New task, Log out. Configuration pages are not part of it; it is hidden while a transcript is shown (`body[data-view=task]`).
- `page_header(title, actions)`, `avatar(initials)`.
- `layout(title, nav, body)` is the frame of a normal page (max width 64 rem); the task workspace uses the full width.

## Patterns

- **Page shell.** Rail (desktop) or bottom bar (phone) around `main`. Pages outside the navigation (errors) use `Nav::None`.
- **Three-column workspace.** List, transcript, details (`/tasks`, `/tasks/{key}`, one template). `body[data-view=list|task]` says which screen a phone shows. Selecting a task is an `hx-get` of the page with `hx-select="#ws-detail"` and `hx-push-url`: the list and its stream stay untouched, the old detail stream closes with the replaced region.
- **List and detail on phones.** The list is a screen of its own; the transcript has a back link and the details open as a bottom sheet.
- **Sheets and dialogs.** A modal task (composer) is a `dialog`; context that belongs to a view (details) is a `details_panel`.
- **Forms and validation.** `hx-post` on the form with `hx-target="this"` and `outerHTML`; success answers `HX-Redirect`, failure answers 422 with the form.
- **Empty and loading states.** `empty_state("No tasks match.")`; there are no spinners because pages are server-rendered and updates stream in.
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
