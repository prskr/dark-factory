# df-web

Server-rendered web UI of the control plane: maud templates, htmx 4 and Alpine (vendored in
`assets/vendor/`), TypeScript (`swc`) and Tailwind (standalone binaries, `build.rs` / Bazel genrules). Design: ADR-0004,
ADR-0011, ADR-0035, ADR-0037 and `openspec/changes/iter1-control-plane-web`.

**Design system:** tokens, components, patterns and the review checklist for new views are in
[`docs/design-system.md`](../../docs/design-system.md). Components live in `src/components/`.

## Routes

| Route | Purpose |
| --- | --- |
| `GET /healthz`, `GET /readyz` | Liveness, readiness (database ping). No login. |
| `GET /assets/*` | Embedded assets, content-hashed URLs, `Cache-Control: immutable`. No login. |
| `GET /lifecycle-events/public-key` | Public key of the lifecycle event signatures (`whpk_<base64>`, `text/plain`); `404` without `lifecycleEvents.signingKeyFile`. No login. |
| `GET /login`, `GET /auth/callback`, `POST /logout` | OIDC login (`docs/content/authentication/_index.md`). |
| `/tasks?status=&project=`, `/tasks/{key}` | Task workspace: list with filters, transcript, details (one template). |
| `GET /tasks/compose?project=`, `POST /tasks` | New-task composer (dialog), creation. |
| `POST /tasks/{key}/title`, `POST /tasks/{key}/messages` | Rename, message input. |
| `POST /tasks/{key}/done`, `POST /tasks/{key}/stop`, `POST /tasks/{key}/cancel` | Mark done (the PR is marked ready; answers the details region, 409 if not allowed), stop the active run (204; 409 without an active run), cancel the task (answers the details region, 409 for a `completed` or `cancelled` task). Stop and cancel are audited (`run.stop`, `task.cancel`). |
| `POST /tasks/{key}/reset-workspace` | Delete the task's volume (answers the details region, 409 while the task has a non-terminal run). Holds the task row lock during the deletion (30 s timeout), so no run starts meanwhile; audited as `task.reset_workspace`. |
| `PATCH /tasks/{key}/messages/{id}`, `DELETE /tasks/{key}/messages/{id}` | Edit or remove a queued message the agent has not received (409 after forwarding, 422 for an empty text). |
| `GET /tasks/events?status=&project=`, `GET /tasks/{key}/events` | SSE streams for the (filtered) list and the open task. |
| `/projects`, `/personas`, `/credentials` | Configuration lists (`POST` creates). |
| `/egress/allowlist` | Egress rules (`?highlight=host:port` marks rows; the Scope column shows "Everywhere", "Project <KEY>" or "Task <KEY>"; `POST /egress/allowlist/host` and `/service` add instance and project entries, `DELETE /egress/allowlist/{id}` removes an entry of any scope). Cluster services are checked against the cluster when saved, but only in the namespaces of `clusterTargets` (the control plane's RBAC); other namespaces are rejected. Desktop navigation only. |
| `/egress/log`, `/egress/log/events` | Network traffic (SSE): connection graph, events table, filters (`q`, `verdict`, `sandbox`, `persona`, `project`, `task`; `denied` and `host` still work) and a request details panel; see `docs/design-system.md`, "Network traffic page". The panel cannot show URL, query or headers: TLS tunnels are not inspected. Needs `gateway.headlessService`. Desktop navigation only. |
| `GET /egress/log/allow?host=&port=&task=&project=`, `POST /egress/log/allow` | "Allow egress" dialog of a blocked row or graph node (modal fragment; a plain request redirects to the log) and the entry it creates. The form posts `host`, `port` and `scope` (`task`, `project` or `instance`) with the `task` and `project` keys. The handler checks the scope itself (the task or project must exist), accepts only an exact host (no wildcard) and creates the entry like the allowlist form: validated, audited, announced to the gateways, and pending `network-access` requests it covers are approved. Answers 422 with the dialog and its error otherwise. |
| `GET .../new`, `GET .../{key or id}/edit`, `POST .../{key or id}` | Form fragments (modal; a plain request redirects to the list) and update. |
| `GET /projects/forge?repo_url=&issue_tracker_url=` | Forge icons and hint of the project form (htmx partials). |
| `/projects/{key}/subscriptions` | Lifecycle event subscriptions of the project (role `admin` as well; others get `403` naming it). The page shows the instance public key with a link to `/lifecycle-events/public-key` and per subscription the last delivery (time, HTTP status or error). `GET .../new`, `GET .../{id}/edit` (modal), `POST` (create), `POST .../{id}` (update) and `DELETE .../{id}`; each change is audited as `event_subscription.create`, `.update` or `.delete` with URL and event types. The form accepts only `https` URLs whose host is not a literal loopback, private, link-local, unique-local or unspecified address, and known event types (none checked: all). The project list links the page for admins only. |

Everything except health, assets, login and the public key requires the role `board-member`;
the project subscription routes require `admin` in addition (`auth::require_admin`). Non-GET
requests need the `HX-Request: true` header (CSRF guard). `base_router(pool,
lifecycle_public_key)` serves health, assets and the unauthenticated
`GET /lifecycle-events/public-key` (the instance public key as text, `404` while lifecycle
events are off); `router(state)` adds the UI. Other crates reuse `layout()` and
`auth::require_board_member`.

## How to stop a run, cancel a task, reopen it and reset its workspace

1. **Stop a run**: press "Stop" on the run card (details column, Runs), or press the round button of the message box while the task is `running` and the box is empty. A connected run (`preparing`, `running`, `awaiting_input`) is `finalizing` at once, the agent is told to stop and the task becomes `waiting` when it has; a run that is not connected yet (`pending`, `provisioning`, `connecting`) ends `cancelled` directly. A `finalizing` run is not stopped again. Queued messages stay queued but do not start a new run on their own. If the runtime has not reported after 120 s, the sandbox is deleted and the run ends `cancelled` (detail `stop-timeout`); the task volume is kept.
2. **Cancel a task**: Status menu, "Cancel task…", confirm. The run is stopped, queued messages are discarded and pending requests closed. The pull request and the issue stay as they are. A `completed` or `cancelled` task cannot be cancelled.
3. **Reopen a cancelled task**: send a message. The task becomes `queued` and a new run starts as soon as the stopped run has ended.
4. **Reset the workspace**: Status menu, "Reset workspace…", confirm with "Reset workspace". The task's volume (clone and build caches) is deleted; the next run clones the repository again and restores the WIP snapshot if there is one. Commits that were not pushed and uncommitted changes without a WIP snapshot are lost. While a run is active the item is disabled ("Stop the run first"): stop the run, wait until it has ended, then reset. Any task offers it, also a `completed` or `cancelled` one; unused volumes are deleted automatically after `volumes.retainAfterCloseSeconds` and `volumes.idleTtlSeconds` (`crates/df-agent-channel/docs/configuration.md`).

## Server-sent events

Each SSE message is a bundle of `<hx-partial hx-target=... hx-swap=...>` elements:

| Message | Partial |
| --- | --- |
| New transcript item | `#transcript`, `beforeend` |
| Item changed (growing agent message, tool call status, turn end) | `#<dom id>`, `outerMorph` |
| Head (title, status), details (status line, PR and issue links, project, persona, branch, collapsed strip, status menu with the cancel and reset dialogs), runs, queued message stack, Send/Stop button of the message box | `#task-head`, `#task-details`, `#runs`, `#pending`, `#composer-action`, `outerMorph` |
| Task list | `#task-rows`, `outerMorph` |

Transcript messages carry the id of the last event they contain as the SSE `id`. Agent chunks that
arrive within 150 ms are sent as one update; when a turn ends, its messages are re-rendered as
Markdown. Content blocks marked `hidden` are dropped from user messages unless `web.showHiddenBlocks` is set (`Folder::with_hidden_blocks`); a message without a shown block produces no item. The DOM id of an agent message is `msg-<message_id>`; ids from the runtime are reduced to
`[A-Za-z0-9_-]`.
A `TurnEnded` event completes the turn's agent messages but adds no item; it refreshes the runs and
details regions. Tool and notice rows (`data-tool`, `data-system`) get their separator lines from the stylesheet, so live
appends regroup without re-rendering.

In the run cards `#n` links to the run divider (`#run-n`). Token counts are compact (`9.4k`,
`182k`, `1.2M`) with the exact value as `title`.

The transcript container (`#transcript-scroll`) opens at the bottom and follows new and growing items
while the reader is within 48 px of the bottom (Alpine `x-data`/`x-init` with a `MutationObserver`).
The queued stack shows three rows and scrolls inside itself; the runs list (newest first) is a
`<details>` that starts closed, and its `open` state survives live updates (`morphIgnore`).
From `md` the details do not scroll as a whole: the sections above Runs keep their height and only the run list scrolls (minimum area 17 rem when open); the description is cut after three lines. The phone sheet scrolls as a whole.

## Markdown, code themes and diagrams

`markdown::render` is the one renderer for agent-written Markdown (the transcript, the request
cards; artifacts will reuse it). comrak runs with raw HTML escaped, images rewritten to links and
`rel="noopener noreferrer nofollow"` on links.

- **Highlighting**: fenced blocks with a language token that syntect's bundled syntaxes know by
  name or extension get `hl-` classes (no inline styles). No fallback to first-line detection;
  blocks without a language, with an unknown one or above 64 KiB (`MAX_HIGHLIGHT_BYTES`) are
  escaped plain text. Every fenced block is `<pre class="hl-code">`.
- **Themes**: `web.codeTheme.light|dark` (syntect theme names, validated at startup by
  `markdown::CodeTheme::new`) become one stylesheet, `GET /assets/code-theme.css?v=<hash>`
  (immutable cache), linked after `app.css`. The dark theme sits in
  `@media (prefers-color-scheme: dark)`. `pre.hl-code` takes the theme's background and color,
  also inside agent bubbles. `values.schema.json` lists the same theme names; a test compares the
  two.
- **Diagrams**: a `mermaid` fence is `<pre class="hl-code" x-data="mermaidDiagram">` with the
  escaped source. `scripts/markdown.ts` (served as `markdown.js`) loads `assets/vendor/mermaid.min.js` (URL from
  `data-mermaid-src` on `<body>`) on the first diagram of a page and draws with
  `securityLevel: 'sandbox'` (iframe; kept in Mermaid's `secure` list so `%%{init}%%` cannot lower
  it). It redraws when `prefers-color-scheme` changes. A parse error leaves the source and adds a
  one-line error.

Vendored Mermaid: version 11.17.2, `dist/mermaid.min.js` (IIFE) from the npm package,
SHA-256 `581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8`.

## Spike: htmx 4 SSE (task 1.1, htmx 4.0.0, `hx-sse.js`)

Result from reading the extension source (`dist/ext/hx-sse.js`):

- **`Last-Event-ID` is sent on reconnect.** The extension remembers the last `id:` of the stream and
  sets the `Last-Event-ID` header when it re-fetches after a dropped connection or after the tab
  was hidden (`pauseOnBackground` cancels the reader and reconnects with backoff when the tab is
  visible again). It is only sent for reconnects of an existing stream; a new page load starts
  without it.
- **Decision: header with `?after=` as the initial value.** The page is rendered with the transcript
  up to event id N and connects to `/tasks/{key}/events?after=N`. A reconnect carries
  `Last-Event-ID`, which wins over `?after=`. Events between page render and first connect are
  replayed because the stream starts at N.
- Unnamed messages are swapped like a normal response. With `hx-swap="none"` on the connecting
  element, the partials in the message do the swapping; empty swaps are suppressed.
- **Morph of a stable id.** `outerMorph` on `#msg-<message_id>` patches the element in place.
  Morph syncs attributes, so a user-opened `<details>` (tool call) would be closed again;
  `htmx-config` sets `morphIgnore` to include `open`.
- htmx 4 swaps every response status except 204 and 304, so validation errors are returned as
  `422` with the form HTML.
- Not verified in a real browser in this environment (no phone/desktop browser session); see the
  manual checks in `openspec/changes/iter1-control-plane-web/tasks.md` (5.1).

Added after the spike:

- **Cleanup of a swapped-out stream.** The extension's `htmx_before_cleanup` aborts the request and
  then cancels the reader of the aborted stream without handling the rejection: an uncaught
  `AbortError` on every swap of `#ws-detail`. `scripts/sse-cleanup.ts` registers a hook that runs
  first (loaded between htmx and `hx-sse`; htmx calls hooks in registration order) and cancels
  the reader of the still open stream. The e2e test
  `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab`
  (`crates/df-e2e/tests/ui.rs`) checks it.

## Assets and styles

The stylesheet (`styles/input.css`) and the component scripts (`scripts/*.ts`) are compiled, not
checked in: Bazel builds them with the pinned `tailwindcss` and `swc`; plain `cargo` runs the tools
from `PATH` (`TAILWIND_BIN` / `SWC_BIN`) in `build.rs` and warns when one is missing. How and what
the tests do then: `docs/development.md`, "Web assets".

Alpine attributes with `@` are not valid in maud; use `x-on:click`. Attribute names with `.`
(`x-on:event.window`) are written as strings.

## Tests

Handlers are tested without a browser: `tower::ServiceExt::oneshot` against a PostgreSQL database
from `df-test-postgres` (needs a container socket), a `wiremock` OIDC provider for the login and
`insta` snapshots for the transcript rendering (`src/transcript/snapshots/`).
