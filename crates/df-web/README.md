# df-web

Server-rendered web UI of the control plane: maud templates, htmx 4 and Alpine (vendored in
`assets/vendor/`), Tailwind (standalone binary, `build.rs` / Bazel genrule). Design: ADR-0004,
ADR-0011, ADR-0035, ADR-0037 and `openspec/changes/iter1-control-plane-web`.

**Design system:** tokens, components, patterns and the review checklist for new views are in
[`docs/design-system.md`](../../docs/design-system.md). Components live in `src/components/`.

## Routes

| Route | Purpose |
| --- | --- |
| `GET /healthz`, `GET /readyz` | Liveness, readiness (database ping). No login. |
| `GET /assets/*` | Embedded assets, content-hashed URLs, `Cache-Control: immutable`. No login. |
| `GET /login`, `GET /auth/callback`, `POST /logout` | OIDC login (`docs/keycloak.md`). |
| `/tasks?status=&project=`, `/tasks/{key}` | Task workspace: list with filters, transcript, details (one template). |
| `GET /tasks/compose?project=`, `POST /tasks` | New-task composer (dialog), creation. |
| `POST /tasks/{key}/title`, `POST /tasks/{key}/messages` | Rename, message input. |
| `PATCH /tasks/{key}/messages/{id}`, `DELETE /tasks/{key}/messages/{id}` | Edit or remove a queued message the agent has not received (409 after forwarding, 422 for an empty text). |
| `GET /tasks/events?status=&project=`, `GET /tasks/{key}/events` | SSE streams for the (filtered) list and the open task. |
| `/projects`, `/personas`, `/credentials` | Configuration lists (`POST` creates). |
| `/egress/allowlist` | Egress rules (`?highlight=host:port` marks rows; instance and project scope; `POST /egress/allowlist/host` and `/service` add, `DELETE /egress/allowlist/{id}` removes). Cluster services are checked against the cluster when saved, but only in the namespaces of `clusterTargets` (the control plane's RBAC); other namespaces are rejected. Desktop navigation only. |
| `/egress/log`, `/egress/log/events` | Network traffic (SSE): connection graph, events table, filters (`q`, `verdict`, `sandbox`, `persona`, `project`, `task`; `denied` and `host` still work) and a request details panel; see `docs/design-system.md`, "Network traffic page". The panel cannot show URL, query or headers: TLS tunnels are not inspected. Needs `gateway.headlessService`. Desktop navigation only. |
| `GET .../new`, `GET .../{key or id}/edit`, `POST .../{key or id}` | Form fragments (modal; a plain request redirects to the list) and update. |
| `GET /projects/forge?repo_url=&issue_tracker_url=` | Forge icons and hint of the project form (htmx partials). |

Everything except health, assets and login requires the role `board-member`. Non-GET requests need
the `HX-Request: true` header (CSRF guard). `base_router(pool)` serves only health and assets;
`router(state)` adds the UI. Other crates reuse `layout()` and `auth::require_board_member`.

## Server-sent events

Each SSE message is a bundle of `<hx-partial hx-target=... hx-swap=...>` elements:

| Message | Partial |
| --- | --- |
| New transcript item | `#transcript`, `beforeend` |
| Item changed (growing agent message, tool call status, turn end) | `#<dom id>`, `outerMorph` |
| Head (title, status), details (status line, PR and issue links, project, persona, branch, collapsed strip), runs, queued message stack | `#task-head`, `#task-details`, `#runs`, `#pending`, `outerMorph` |
| Task list | `#task-rows`, `outerMorph` |

Transcript messages carry the id of the last event they contain as the SSE `id`. Agent chunks that
arrive within 150 ms are sent as one update; when a turn ends, its messages are re-rendered as
Markdown. The DOM id of an agent message is `msg-<message_id>`; ids from the runtime are reduced to
`[A-Za-z0-9_-]`.
A `TurnEnded` event completes the turn's agent messages but adds no item; it refreshes the runs and
details regions. Tool rows (`data-tool`) get their separator lines from the stylesheet, so live
appends regroup without re-rendering.

In the run cards `#n` links to the run divider (`#run-n`). Token counts are compact (`9.4k`,
`182k`, `1.2M`) with the exact value as `title`.

The transcript container (`#transcript-scroll`) opens at the bottom and follows new and growing items
while the reader is within 48 px of the bottom (Alpine `x-data`/`x-init` with a `MutationObserver`).
The queued stack shows three rows and scrolls inside itself; the runs list (newest first) is a
`<details>` that starts closed, and its `open` state survives live updates (`morphIgnore`).
From `md` the details do not scroll as a whole: the sections above Runs keep their height and only the run list scrolls (minimum area 17 rem when open); the description is cut after three lines. The phone sheet scrolls as a whole.

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

## Assets and styles

`assets/app.css` is the Tailwind output committed as a fallback for builds without the `tailwindcss`
binary (`TAILWIND_BIN` or `PATH`); `build.rs` regenerates it when the binary is available and Bazel
always does. Regenerate the committed file after changing classes:

```bash
tailwindcss -i crates/df-web/styles/input.css -o crates/df-web/assets/app.css --minify
```

Alpine attributes with `@` are not valid in maud; use `x-on:click`. Attribute names with `.`
(`x-on:event.window`) are written as strings.

## Tests

Handlers are tested without a browser: `tower::ServiceExt::oneshot` against a PostgreSQL database
from `df-test-postgres` (needs a container socket), a `wiremock` OIDC provider for the login and
`insta` snapshots for the transcript rendering (`src/transcript/snapshots/`).
