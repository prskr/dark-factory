# Design

## Context

The web UI from `ui-redesign` (maud components in `df-web::components`, htmx 4, Alpine, Tailwind tokens, SSE regions) is the base; `docs/design-system.md` documents it. The look of every view is fixed on the design canvas "Dark Factory UI redesign" (artboards in the Desktop, Phone, Project filter and new task, Configuration and Empty states rows; the "Later" row is out of scope).

Two backend facts shape this change:

- **Queued messages are forwarded immediately.** `df-agent-channel::Session::send_new_prompts` sends every undelivered `task_messages` row to the runtime as soon as it is queued; the runtime holds it in its own queue until the current turn ends. A message counts as delivered only when its `UserMessage` transcript event is persisted (`delivered_at`). The transcript already records `TurnStarted` and `TurnEnded`.
- **There is one forge integration per instance.** `dark-factory` loads the `forgejo` plugin from `plugins.forgejo`, and every project with a forge credential is served by it (`PluginForgeClient::resolve`). There is no per-host mapping.

## Goals / Non-Goals

**Goals:**
- Queued messages stay editable until the agent really receives them, without changing the runtime or the wire contract.
- The forge indicator shows what the system will actually do with a project, not a guess from the host name.
- Every new view element is a documented component; the code matches the mockup without one-off styling.

**Non-Goals:**
- Per-host forge routing or a second forge plugin (the indicator is ready for more integrations, the routing is not).
- Editing a message after the runtime received it (would need a retract message in the agent protocol).
- Client-side rendering of the transcript; grouping stays server-rendered plus CSS.

## Decisions

### D1 — Hold queued messages back until the runtime is between turns
New column `task_messages.forwarded_at TIMESTAMPTZ NULL`. The session keeps a `busy` flag per stream: set by a persisted `TurnStarted` of its run, cleared by `TurnEnded`; on session start it is derived from the run's last turn event (a `TurnStarted` without a later `TurnEnded` means busy). `send_new_prompts` then:

1. re-sends every undelivered message that is already forwarded and not yet sent on this stream (reconnect case, unchanged dedup by prompt id), and
2. only when not busy, claims the held-back ones with `UPDATE task_messages SET forwarded_at = now() WHERE task_id = $1 AND delivered_at IS NULL AND forwarded_at IS NULL RETURNING …` (queue order) and sends them.

It runs on the existing triggers (queue notification, session start) and additionally after a batch containing `TurnEnded`. The run's first prompt (assignment) is sent at session start as today, because the run is not busy then.

*Alternatives:* a `RetractPrompt` control message in the agent protocol, so forwarding stays immediate (touches the proto contract, `factory-agent` and its queue, and edit becomes "retract and hope it was not started"); allowing edits only while no run is active (rarely useful, because a queued message starts a run within seconds). Rejected in favour of one source of truth in the database. Cost: a message reaches the agent at the end of the current turn instead of being queued in the runtime earlier, which is when the agent would have read it anyway.

### D2 — Edit and remove are conditional writes
`df-db::edit_queued_message(task, id, content, actor)` and `remove_queued_message(task, id, actor)` run one statement each with `WHERE id = $1 AND task_id = $2 AND delivered_at IS NULL AND forwarded_at IS NULL AND author->>'kind' = 'human'` (the exact author encoding follows `Principal`'s serde form). Zero rows means "already received" (or not editable), which the handler maps to 409 with "The agent has already received this message.". The claim in D1 and these writes contend on the same row, so the database decides the race. Removal and the status rule (`queued` with no undelivered message left → `waiting`) run in one transaction. Both write `message.edit` / `message.remove` audit entries and publish a new `task_message_changed` notification, which the task's SSE stream turns into a re-render of the queued stack.

Content stays `Vec<ContentBlock>`; an edit replaces it with one text block, as the message input only produces text today.

A failed or rate-limited task that was requeued by a message and then has that message removed goes to `waiting`, not back to its earlier status; the earlier status is not stored and `waiting` is the honest "nothing to do" state.

### D3 — Queued stack and editing in the message input
`GET /tasks/{key}` renders the stack (`pending_region`) inside the message form area instead of the transcript. `PATCH /tasks/{key}/messages/{id}` (form field `message`) and `DELETE /tasks/{key}/messages/{id}` answer with the re-rendered stack (or 409/422 with the stack and an error). Edit is Alpine state in the form (`editing: {id, text}`): it fills the textarea, swaps Send for Save and shows the "Editing a queued message" bar with Cancel; Save sends `hx-patch`, Cancel restores the previous draft. Messages that are forwarded but undelivered stay in the stack without edit/remove buttons, marked "waiting for delivery"; on a `failed` or `completed` task the mark reads "not delivered, sent with the next run" (they are re-sent when a new run starts, D1).

### D4 — Conversation layout with server-side runs and CSS grouping
Transcript items get `data-author="human|agent"`; tool rows count as `agent`. Each message renders its avatar; CSS hides it when the next sibling has the same author (`[data-author=agent]:has(+ [data-author=agent]) .avatar { visibility: hidden }`), so live appends regroup without re-rendering earlier items. When a turn currently ends with a tool row, the group shows no avatar until the next message; accepted. Bubbles: human right on `sunk`-like `bubble-me`, agent left on `ink` (inverted in dark), new tokens `bubble-me`, `bubble-agent`, `on-bubble-agent`, `code-on-agent` in `styles/input.css`. Run dividers are rendered where the run of consecutive items changes and for the run-start event in the stream.

*Alternative:* re-rendering the previous item on every append (more SSE payload, more swap logic).

### D5 — Forge kind from the configured integration
`WebState` gets the forge integration the instance loaded (`Option<ForgeKind>`; today `Some(Forgejo)` when `plugins.forgejo` is configured). `ForgeKind` (df-core) maps a plugin name to icon and label (`forgejo`, `github` known; other names show the generic Git icon with the plugin name). A project shows the kind when the integration is configured and the project has a forge credential, otherwise Git. The form's feedback fragment `GET /projects/forge?repo_url=…&issue_tracker_url=…` (htmx, `keyup changed delay:300ms`) checks that the URL is a valid HTTPS repository URL (scheme, host, at least owner and repository path segments, same check as project validation) and answers icon and hint; the issue tracker URL gets the forge icon when it is on the repository's host, otherwise a generic issue icon. With one integration every valid HTTPS repository maps to it; the hint then tells the truth: this is the integration that will be used.

*Alternative:* guessing by host (`github.com` → GitHub) — contradicts the decision that the integration decides, and is wrong for self-hosted instances.

### D6 — Project counts in one grouped query
`df-db::project_status_counts()` returns `(project_id, status, count)` from `SELECT project_id, status, count(*) FROM tasks GROUP BY 1, 2`; `df-web` folds it into the four columns. Links: Queued → `status=queued`, Running → `status=running`, Waiting → `status=waiting`, Done → `status=done`. `queued` and `running` are new URL-only values of the list's status filter (no chip, all chips unselected), defined next to the status groups in `df-core` so the list query and `/tasks/events` share them.

### D7 — Configuration forms as htmx modals
A single `<dialog id="modal">` in the configuration page shell. Create and edit buttons `hx-get` the form fragment (`/projects/new`, `/projects/{key}/edit`, same for personas and credentials) into it and open it; the existing `POST` handlers stay. On 422 they return the form fragment with errors (the modal stays open); on success they return the updated list region with `HX-Retarget: #config-list` and `HX-Trigger: modal-close`. A direct, non-htmx `GET` of a fragment route redirects to the list. Below 768 px the dialog is styled as a full-height sheet (same component as the details sheet). Escape and the backdrop close the dialog.

### D8 — Shell: logo, rail, phone bar, profile menu
`assets/logo.svg` holds the factory mark from the mockup (`currentColor` strokes); the rail inlines it and `layout` links it as `<link rel="icon" type="image/svg+xml">` with the hashed asset URL. The rail renders icon-only links with `aria-label` and `title`, and the active bar as a pseudo-element. The bottom bar takes the create action from the page (`Nav::Projects` → `New project` opening the project modal; otherwise the composer). The profile avatar shows the session user's initials; its menu (Alpine) holds name, role ("board member") and the existing `hx-post="/logout"`.

### D9 — Collapsible details
Alpine state `detailsCollapsed` on the workspace, initialised from `localStorage['df.details.collapsed']` (read in `try/catch`, default expanded) and written on toggle. A class on the workspace switches the `xl` grid track from 20 rem to 4 rem and shows the strip instead of the panel. Below 1280 px the state has no effect.

### D10 — Design system additions
`docs/design-system.md` gains: bubble and run divider, queued stack and editing bar, modal/sheet for forms, empty state, forge icon, profile menu, collapsed details strip; each with its component signature, states and do/don't. New icons (logo, Forgejo, GitHub, Git, issue) are inline SVGs in `components::icons`; Forgejo's logo is CC BY-SA 4.0 (licence note next to the icon), the GitHub mark is used only to identify GitHub as the forge.

### D11 — Tablet tier
The phone/desktop switch moves from 1024 px (`lg:`) to 768 px (`md:`) for the rail, the bottom bar, the profile menu, the list's project menu, form sheets and the list/transcript split, so a browser split screen (720–960 px) keeps the rail and shows list and transcript side by side. From 768 to 1279 px the layout is the one 1024–1279 px has today: the details open as a panel over the transcript. The `xl` (1280 px) details column and its collapse stay as they are.

### D12 — Corrections from the manual check (7.2)
The browser check against the canvas turned up differences; each follows the named artboard:
- Details header (`Main.dc.html`): heading, close and collapse controls as drawn; the collapsed strip (`DesktopCollapsed.dc.html`) shows only the status and the PR/issue links, no heading text.
- Message input (`Main.dc.html`): the send button sits inside the rounded input box.
- Runs (`Main.dc.html`, Runs section): one card per run with `#n`, status badge, start time, a one-line detail (failure reason and error truncated to one line, the full error text in a `<details>` disclosure) and a model / in / out / cache table from the per-model usage `df_db::task_views` already returns. The Links section shows the PR and issue when present.
- New-task composer (`NewTask.dc.html`): a round `+` button on the left opens the linked-issue field (accessible name "Link issue"); the persona picker sits next to the submit button; no "Options" text.
- Projects (`Projects.dc.html`): the table fills the content width (page padding 32/40 px, header with count and New project), columns and counters as drawn.
- Personas and Credentials (`Personas.dc.html`, `Credentials.dc.html`): the same header with count; every page outside the task workspace fills the content width (no max width).
- Projects and Credentials share one table component (card, header row, cell padding, row borders as in `Projects.dc.html` / `Credentials.dc.html`); a wide table scrolls inside its card and never widens the page.
- Phone details sheet (`MobileDetails.dc.html`): 18 px top radius and a 40×5 px grey handle. It opens at about half height; dragging the handle up gives full height, dragging down returns to half height or, past a threshold, closes the sheet; tapping the handle toggles the two heights. Escape, the backdrop and the close button still close it. A small Alpine pointer handler, no library; without pointer events the sheet keeps its fixed height.
- Phone bottom bar: hidden while a transcript is shown (web-ui-shell "Transcript on a phone"); the hiding rule must win over the bar's own display utility.
- Configuration modals (`Projects.dc.html`, `Personas.dc.html`, `Credentials.dc.html` with `modal`): two-column form grid as drawn (project: key and name in one row, key read-only when editing; base branch and forge credential in one row; persona: harness and model in one row); footer at the bottom right with Cancel (closes the dialog) and "Save project" / "Save persona" / "Save credential reference" ("Create …" in the new form).
- Phone project list (`ProjectsPhone.dc.html`): key and name with a "Tasks" link on the right, then the forge icon next to the repository URL (mono, 12 px), then the counts as `<dot> <number> <state>`; the edit button is a separate column on the right below the Tasks link, not in the top corner.
- Empty task list and workspace (`Main.dc.html` with `empty`, `MobileList.dc.html` with `empty`): as drawn.
- Details overview (`Main.dc.html`, overview branch): sections in the drawn order, each with an uppercase faint header: "Now" (status dot and the task's status line, or the status label when the line is empty), "Links" (PR and issue cards; "No pull request or issue yet." when there is none), the definition list (76 px label column, faint 13 px labels, values left-aligned and medium weight, persona with its harness), then Description and Runs. A run card without recorded usage shows the persona's model (or "default model") and "no token usage recorded" instead of the table.
- Task list row (`Main.dc.html:99-111`): 3 px left edge in the accent colour and the accent-soft background for the selected task (like the rail's active bar), title 15 px medium, key/time mono 12 px faint.
- Composer chips (`NewTask.dc.html`): the project chip is a 10 px rounded rectangle; the persona picker has no border; the Create task button uses the mockup's arrow-up icon, optically centred. The textarea shows no accent focus outline; the card signals focus with a stronger border (`:focus-within`). The composer's close button has no background.
- Forge icon: the Forgejo logo as drawn on the canvas (CC BY-SA 4.0 note next to it) replaces the simplified glyph.

## Risks / Trade-offs

- [The `busy` flag is wrong after an unusual event order, so messages are held back forever] → the flag is cleared on `TurnEnded`, on session start without an open turn and on run end; a test covers a run that ends during a turn; held-back messages are re-evaluated on the next session.
- [A message queued during a long turn reaches the agent later than today] → the agent only reads it at the next turn anyway; the stack shows the message as queued, so nothing looks lost.
- [CSS `:has()` grouping in older browsers] → without `:has()` every message shows its avatar; layout stays correct.
- [GitHub mark usage] → shown only as forge identification next to a repository, as GitHub permits; no other brand use.

## Migration Plan

One migration adds `task_messages.forwarded_at` (nullable) and sets it to `now()` for every row that is undelivered at upgrade time: the old control plane may already have forwarded them, so they must not become editable. They are re-sent on the next session (D1 step 1) and the runtime deduplicates by prompt id. Messages queued after the upgrade start held back. Rollback: the old control plane ignores the column.
