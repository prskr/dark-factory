# Review log: iter1-control-plane-web

- BASE: fa902a3feaa103916193a59ee3ac1487b2d5d4c2
- Store: none
- Status: done
- Implementer agent: abcc21153591443c2

## Round 1 (2026-10-22)

- Implementer: de414d2, tasks 16/18 (2.4 and 5.1 left open: manual Keycloak and phone-browser checks), checks fmt/clippy/machete/tests/bazel/helm ok, df-web line coverage 90.7 %, container tests ran (podman socket)
- Reviewed range: BASE...de414d2

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/sse.rs:219-266` | Pending messages stay "Pending" after delivery: `mark_prompt_delivered` sends no notification and the poll timeout does not set `check_regions`. The test hides the race with a conditional second read | fix-now: hide pending messages whose prompt id is in the folded transcript (or refresh regions on timeout); make the test deterministic | fixed (9737c6e) |
| P4 | Spec | Blocking | `crates/df-web/src/transcript/mod.rs:590` | No test for the "300-character code line scrolls only the code block" scenario | fix-now: rendered-HTML assertion | fixed (9737c6e) |
| P5 | Spec | Blocking | `crates/df-web/src/layout.rs:56-62` | Mobile nav links about 36 px high; spec requires touch-sized targets | fix-now: `min-h-11` | fixed (7c9598b) |
| P2 | Spec | P1 | `crates/df-web/src/sse.rs:222` | Poll timeout never re-renders head/runs/pending; a lost `task_changed` leaves the PR link and status stale | fix-now: `check_regions = true` on timeout | fixed (9737c6e, test 7c9598b) |
| P3 | Spec | P1 | `crates/df-web/src/sse/tests.rs` | No test for a `RunChanged` live update (task 4.5) | fix-now | fixed (06d90a0) |
| P6 | Spec | P1 | `crates/df-web/src/auth/tests.rs` | First-login test does not assert first_seen/last_seen | fix-now | fixed (9737c6e) |
| P9 | Spec | P1 | docs | `serve` without `auth` mounts only health/assets; not documented outside the code | fix-now: document in configuration docs / docs/keycloak.md | fixed (9737c6e) |
| S2 | Standards | P1 | `crates/df-web/src/task_pages.rs:225-231` | `with_details` error silently dropped; audit written outside the create transaction | fix-now: log it (or write in the same transaction as catalog.rs does) | fixed (9737c6e) |
| S3 | Standards | P1 | `auth/mod.rs:172`, `auth/oidc.rs:245` | Duplicate `now_unix` | fix-now | fixed (9737c6e) |
| S4 | Standards | P1 | `auth/oidc.rs:108,143,172` | `CoreClient::from_provider_metadata(...)` repeated 3x | fix-now: `async fn client(&self)` | fixed (9737c6e) |
| S5 | Standards | P1 | `assets.rs:40-44,70-74` | Hand-rolled hex twice; `hex` is a workspace dependency | fix-now: `hex::encode` | fixed (9737c6e) |
| S6 | Standards | P1 | `auth/mod.rs:333-337` | Logout duplicates `ui::htmx_redirect` | fix-now | fixed (9737c6e) |
| S7 | Standards | P1 | `auth/mod.rs:33-41`, `auth/oidc.rs:42-62` | Tokens and PKCE verifier in `Debug` structs | fix-now: `SecretString` or redacting `Debug` | fixed (9737c6e) |
| S8 | Standards | P1 | `sse.rs:41-53`, `auth/mod.rs:105-184` | Private helpers before public items (AGENTS.md file structure) | fix-now | fixed (9737c6e) |
| S9 | Standards | P1 | `sse.rs:135,180`, `transcript/mod.rs:216,297,330` | Direct indexing in production path (invariant by construction) | fix-now: `get`/`get_mut` or invariant comment | fixed (9737c6e) |
| S1 | Standards | P2 | `df-db/src/{users,catalog,task_views}.rs`, `auth/store.rs` | New queries are runtime `sqlx::query_as`, not compile-time checked; `select_project!` concatenation blocks a direct switch to macros | tech-debt: matters when a schema change breaks a query (caught only by container tests) | tech-debt (TD-42) |
| P7 | Spec | P2 | `transcript/mod.rs:464`, design D3 | syntect code highlighting skipped; design/proposal still name it | tech-debt: matters when highlighting is wanted; update design via /opsx:update or implement | tech-debt (TD-43) |
| S10 | Standards | P3 | `auth/mod.rs:168-170` | `openidconnect::url::form_urlencoded` instead of direct `url` | cosmetic | cosmetic |
| S11 | Standards | P3 | `Cargo.toml:40` | jsonschema 0.60 bump unrelated to the change (needed by outdated hook) | cosmetic: mention in squashed commit message | cosmetic |
| S12 | Standards | P3 | `auth/mod.rs:187` | `error_page` belongs in `ui.rs` | cosmetic | cosmetic |
| P8 | Spec | P3 | `task_pages.rs` | `task.create` audit not required by spec | cosmetic: keep | cosmetic |

User decision: Fix blocking + P1 (P1, P4, P5, P2, P3, P6, P9, S2-S9). S1 and P7 deferred as tech debt.

## Round 2 (2026-10-22)

- Implementer: 9737c6e + 12d3798 (Bazel lock), tasks 16/18, checks fmt/clippy/machete ok, df-web tests 65 pass, bazel build + df_web_test ok, container tests ran (podman socket)
- Reviewed range: de414d2...12d3798

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P11 | Spec | Blocking | `crates/df-web/src/sse/tests.rs:200-272` | `SseReader::next()` waits 5 s = `POLL`; since P2, the poll timeout re-renders regions, so the RunChanged/TaskChanged live tests pass even without the notification arms | fix-now: `next_within` well below `POLL` after publish | fixed (06d90a0) |
| P10 | Spec | Blocking | `crates/df-web/src/layout.rs:54-63`, `assets/app.css` | Committed fallback `app.css` lacks `.inline-flex`; in a plain cargo build `min-h-11` has no effect on the inline links and `py-2` is gone, so targets shrink | fix-now: regenerate and commit `app.css`; test asserts `.inline-flex` in the embedded CSS | fixed (7c9598b, test 06d90a0) |
| P12 | Spec | Blocking | `crates/df-web/src/sse.rs:220-224` | P2 fix (poll-timeout region refresh) has no regression test | fix-now: test with `tokio::time::pause()` or no publish and a wait beyond `POLL` | fixed (7c9598b) |
| S13 | Standards | P1 | `crates/df-web/src/auth/oidc.rs:43` | Private `DiscoveredClient` alias before public items | fix-now | fixed (7c9598b) |
| S14 | Standards | P1 | `deploy/helm/dark-factory/values.yaml:52-54` | `publicUrl` comment disagrees with the README wording | fix-now | fixed (7c9598b) |
| S15 | Standards | P3 | `crates/df-web/src/sse.rs:72-250` | Private `DetailStream` before public `task_list_events` (pre-existing) | cosmetic | cosmetic |

User decision: Fix blocking + P1 (P11, P10, P12, S13, S14). S15 cosmetic.

## Round 3 (2026-10-22)

- Implementer: 7c9598b, tasks 16/18, checks fmt/clippy/machete ok, df-web tests 66 pass, bazel df_web_test ok, helm lint ok, container tests ran (podman socket)
- Reviewed range: 12d3798...7c9598b

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P13 | Spec | Blocking | `crates/df-web/src/sse/tests.rs:262` | `run_changes_rerender_the_runs_region` still uses `next()` (5 s = `POLL`); dropping the `RunChanged` alternative goes undetected. The implementer report claimed this was changed | fix-now: `next_within` below `POLL`; verify red by dropping only `RunChanged` from the arm | fixed (06d90a0) |
| P14 | Spec | Blocking | `crates/df-web/src/assets.rs:109-123` | The `.inline-flex` assertion promised for P10 is missing; the layout test checks only the HTML class string | fix-now: add `.inline-flex` to the stylesheet test | fixed (06d90a0) |
| S16 | Standards | P1 | `crates/df-web/src/sse/tests.rs:216,287` | Test deadlines restate `POLL` as literals | fix-now: derive from `super::POLL` | fixed (06d90a0) |

User decision: Fix blocking + P1 (P13, P14, S16).

## Round 4 (2026-10-22)

- Implementer: 06d90a0 (test-only), tasks 16/18, checks fmt/clippy/machete ok, df-web tests 66 pass, container tests ran (podman socket)
- Reviewed range: 7c9598b...06d90a0

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17 | Standards | P3 | `crates/df-web/src/sse/tests.rs:220` | Inline comment repeats the `LIVE` doc comment | cosmetic | cosmetic |

User decision: Finish; no blocking findings open. Tech debt: TD-42 (S1), TD-43 (P7).

## Result

- Status: done. Tasks 18/18; 2.4 verified by the user against the dev Keycloak (2026-10-22, via HTTPRoute + Cilium Gateway). 5.1 checked by the user on a phone against the dev instance (2026-10-22) as far as a run gets before the egress proxy stub (503, `iter1-egress-gateway` pending).
- Tech debt created: TD-42, TD-43.
