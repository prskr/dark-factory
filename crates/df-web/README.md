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
| `GET /tasks/events?status=&project=`, `GET /tasks/{key}/events` | SSE streams for the (filtered) list and the open task. |
| `/projects`, `/personas`, `/credentials` (`/new`, `/{key or id}`) | Configuration forms. |

Everything except health, assets and login requires the role `board-member`. Non-GET requests need
the `HX-Request: true` header (CSRF guard). `base_router(pool)` serves only health and assets;
`router(state)` adds the UI. Other crates reuse `layout()` and `auth::require_board_member`.

## Server-sent events

Each SSE message is a bundle of `<hx-partial hx-target=... hx-swap=...>` elements:

| Message | Partial |
| --- | --- |
| New transcript item | `#transcript`, `beforeend` |
| Item changed (growing agent message, tool call status, turn end) | `#<dom id>`, `outerMorph` |
| Head (title, status), details (status line, PR and issue links, project, persona, branch), runs, pending messages | `#task-head`, `#task-details`, `#runs`, `#pending`, `outerMorph` |
| Task list | `#task-rows`, `outerMorph` |

Transcript messages carry the id of the last event they contain as the SSE `id`. Agent chunks that
arrive within 150 ms are sent as one update; when a turn ends, its messages are re-rendered as
Markdown. The DOM id of an agent message is `msg-<message_id>`; ids from the runtime are reduced to
`[A-Za-z0-9_-]`.

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
