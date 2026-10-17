# Proposal

## Why

The demo starts and ends in the browser: a board member logs in, configures a project and persona, creates `DF-1`, watches the live transcript and sends a follow-up — partly on a phone (ADR-0035). This change delivers the human side of the control plane (milestones 3 and 8 of `product/iterations/iteration-1.md`).

## What Changes

- axum HTTP server in `serve` with health endpoints.
- OIDC login against Keycloak (server-side code flow with PKCE, HTTP-only session cookie, CSRF header), `board-member` role check from a flat `roles` claim, user records.
- Server-rendered UI (maud, vendored htmx 4 and Alpine, Tailwind via standalone binary) with a shared layout and mobile-first navigation.
- Desktop forms: projects, personas, credential references (Secret name + key only).
- Phone-ready task pages: task list with status, task creation, task detail with live transcript over SSE (tool calls collapsed, chunk coalescing, `Last-Event-ID` replay), message input, runs with state/outcome/raw token counts, PR and issue links.
- Spike: htmx 4 SSE `Last-Event-ID` behaviour.

## Capabilities

### New Capabilities
- `human-auth`: OIDC login, sessions, CSRF protection and the iteration-1 role check.
- `configuration-management`: creating and editing projects, personas and credential references.
- `task-ui`: task list, task creation, task detail with live transcript, message queueing and run overview.

### Modified Capabilities
None.

## Impact

- New crate `df-web`; `dark-factory serve` mounts its router. Session store table added by its own migration.
- Uses `df-db` task-management operations and notifications (`iter1-foundation`). Reads transcript rows written by `iter1-run-orchestration`; renders all transcript v1 event kinds, so it can be built and tested with fixture rows before orchestration exists.
- Provides the layout and navigation that `iter1-egress-gateway` uses for its desktop-only egress pages.
- New dependencies: axum, maud, tower-sessions (+ Postgres store), openidconnect, rust-embed, comrak, syntect.
