# Design

## Context

ADR-0004 (maud, htmx 4, Alpine, SSE), ADR-0011 (OIDC, roles), ADR-0035 (form factors), ADR-0037 (Tailwind standalone). Shared crates, schema, notifications and ports: `iter1-foundation` design. Transcript rows (`transcript_events.payload` = canonical protobuf JSON of `transcript.v1.Event`) are written by `iter1-run-orchestration`.

## Goals / Non-Goals

**Goals:**
- Every page testable without a browser: handlers return HTML asserted in `axum` tests (`tower::ServiceExt::oneshot`) against a `#[sqlx::test]` database.
- One SSE mechanism reused by task list, task detail and (later) the egress log.

**Non-Goals:**
- Integration API (utoipa, JWT bearer) — later.
- Permission model beyond `board-member`, audit view, cancel button, artifacts, search.

## Decisions

### D1 — Crate and routing
`df-web` exposes `fn router(state: WebState) -> axum::Router` and `fn layout(title, nav, body) -> Markup`. `dark-factory serve` merges it with other routers (the egress pages from `iter1-egress-gateway` reuse `layout` and the auth middleware). Routes: `/`, `/login`, `/auth/callback`, `/logout`, `/projects[/new|/{key}]`, `/personas[...]`, `/credentials[...]`, `/tasks`, `/tasks/new`, `/tasks/{key}`, `/tasks/{key}/messages` (POST), `/tasks/{key}/events` (SSE), `/tasks/events` (SSE, list), `/assets/*`.

### D2 — Auth
`openidconnect` (discovery, code flow + PKCE, refresh), `tower-sessions` with the Postgres store; its table comes from a `df-web`-owned migration (the app role has no DDL). Session holds user id, roles, refresh token, token expiry; refresh happens lazily on the first request after expiry. CSRF: non-GET requests require `HX-Request: true`; every form is submitted by htmx. Claim path for roles is configurable (`auth.rolesClaim`, default `roles`).
*Alternative:* synchronizer tokens — unnecessary given `SameSite=Lax` plus a custom header (ADR-0011).

### D3 — Transcript rendering
Events are read in `id` order and folded into view items: user message, agent message (chunks with the same `message_id` concatenated), thought (collapsed), tool call (`ToolCall` + later `ToolCallUpdate`s merged), plan, turn boundary with usage, notice. Rendering is a pure function `render_item(&Item) -> Markup` — snapshot-tested with fixtures for every event kind. Markdown: comrak with raw HTML disabled; code highlighting via syntect classes.

### D4 — SSE
Per-request stream: on connect, replay rows with `id > Last-Event-ID` (header, or `?after=` if the spike shows htmx 4 doesn't send it), then subscribe to `transcript_event` notifications for the task and read new rows. Agent chunks are coalesced per `message_id` for 150 ms, emitted as a morph swap of the message element (`id="msg-<message_id>"`); on `TurnEnded` the message is re-rendered as Markdown. Task-list and detail header updates use `task_changed`/`run_changed` notifications and out-of-band swaps.

### D5 — Styling and assets
htmx 4 and Alpine vendored under `crates/df-web/assets/vendor/`, embedded with `rust-embed`, served with content-hashed URLs and long cache headers. `build.rs` runs Tailwind if `TAILWIND_BIN` or `tailwindcss` on `PATH` is available, otherwise emits a cargo warning and an empty stylesheet (Bazel always provides it). Alpine long-form attributes (`x-on:click`) because `@` is maud syntax.

### D6 — Token counts
Per run: sum of `TurnEnded.usage` grouped by `model_id`, computed by a SQL query over `transcript_events` (JSONB). No cost calculation in iteration 1.

## Risks / Trade-offs

- [htmx 4 is new; SSE extension semantics may differ from docs] → spike first (task 1.1); fallback `?after=` parameter.
- [Large transcripts make the detail page slow] → acceptable in iteration 1; paging/virtualisation later.
- [Phone sleeping kills SSE] → replay by id (D4); the message draft is kept in Alpine state outside the swapped region.
