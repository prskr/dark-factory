# Tasks

## 1. Spike and skeleton

- [x] 1.1 Spike htmx 4 SSE: does it send `Last-Event-ID` on reconnect, and how do morph swaps of a stable id behave; record the result in `crates/df-web/README.md` and choose header or `?after=` (design D4)
- [x] 1.2 Create `df-web` with `router()`, `layout()`, vendored htmx/Alpine, `rust-embed` assets, Tailwind `build.rs` (design D5) and `/healthz`, `/readyz` (DB ping); mount it in `dark-factory serve`; verify with an axum test that `/healthz` returns 200 and assets are served with cache headers

## 2. Authentication

- [x] 2.1 Add the session-store migration and `tower-sessions` setup; verify a session survives a server restart in a `#[sqlx::test]`
- [x] 2.2 Implement login, callback (state/PKCE check), user upsert, logout and lazy token refresh test-first against a mock OIDC provider (`wiremock` serving discovery, JWKS and token endpoints); verify the scenarios in `specs/human-auth/spec.md`
- [x] 2.3 Implement the auth middleware (`board-member` from the configured claim, 403 page) and the CSRF check (`HX-Request` on non-GET); verify the cross-site POST and missing-role scenarios
- [x] 2.4 Document the Keycloak client setup (confidential client, redirect URI, client-role mapper emitting a flat `roles` claim) in `docs/keycloak.md`; verify against the dev Keycloak that login works as documented

## 3. Configuration pages

- [x] 3.1 Implement credential reference list/create/edit (name, Secret name, key; no value field) with audit entries; verify with handler tests incl. "no value field"
- [x] 3.2 Implement project list/create/edit with validation (key regex, HTTPS repo URL, key immutable once tasks exist) and audit; verify the scenarios in `specs/configuration-management/spec.md`
- [x] 3.3 Implement persona list/create/edit (harness `claude-code`, adapter command, model, harness credential) with audit; verify with handler tests
- [x] 3.4 Implement responsive navigation (configuration links hidden from the mobile menu); verify the 390 px navigation scenario with a rendered-HTML test for the mobile menu

## 4. Task pages

- [x] 4.1 Implement the task list (newest first, status, status line) and task creation form using `df-db` task operations; verify "Create DF-1" and "Missing title" scenarios
- [x] 4.2 Implement transcript folding and rendering (design D3) test-first with fixtures for every `transcript.v1` event kind; verify snapshot tests and the HTML-escaping scenario
- [x] 4.3 Implement the task detail SSE endpoint (replay after id, live via notifications, chunk coalescing, Markdown re-render at turn end); verify with an SSE client test: replay after id yields no gaps or duplicates, and chunks are coalesced
- [x] 4.4 Implement the message input (POST, pending display until delivered, draft kept across reconnects); verify the follow-up message scenario
- [x] 4.5 Implement the runs section (state, outcome, detail, retry time, token counts per model) and PR/issue links with live updates via `run_changed`/`task_changed`; verify with handler tests using fixture runs and transcript rows
- [x] 4.6 Implement live status updates on the task list via SSE; verify the status-change scenario in a test

## 5. Verification

- [x] 5.1 Check task list, creation and detail at 390 px width in a real phone browser against the dev instance (no page-level horizontal scroll, tap to expand tool calls, reconnect after sleep); record the result in the change's PR description
- [x] 5.2 Verify `cargo llvm-cov -p df-web` reports ≥ 70 % line coverage
