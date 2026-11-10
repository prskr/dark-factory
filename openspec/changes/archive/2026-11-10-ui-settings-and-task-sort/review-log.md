# Review log: ui-settings-and-task-sort

- BASE: 9e01101305e4d8452c2e7cb7a2276fa1f2db458d
- Store: none
- Status: untested (e2e tests in crates/df-e2e not run; CI runs them)
- Implementer agent: a0acf376efe57a97b (iteration 1 started 2026-11-10)
- DOCKER_HOST: unix:///var/run/docker.sock
- Step: 7 (finished)

## Design (2026-11-10)

- 1.1 PenPot: `Settings nav` set has Projects first and a new state `Active=Projects`; `Rail` set lost its Projects link and its `Active=Projects` variant (rails switched to `Active=Settings`); "Settings · personas" rail and sidebar are now instances; "Projects · table with task counters" renamed "Settings · projects" with the sidebar.
- 1.2 PenPot: user chose **horizontal tabs** (tabbed_form gets a layout parameter; the model provider modal stays vertical). Boards: "Projects · settings page, general", "Projects · settings page, saved", "Phone · project settings page, general" (phones: scrolling tab row, Save only, no bottom bar).
- 1.1 + 1.2 approved by the user. Agreed: the remaining "Projects · edit modal, …" boards are converted to the page layout in 1.4/1.5 and the modal boards deleted.
- 1.3 PenPot: "Flows (filter, new task)" / "Desktop · task list sort" (closed, tooltip) and "Desktop · task list sort open"; "Tasks - Phone" / "Phone · task list sort" and "Phone · task list sort open". Icon button with Lucide arrow-down-up left of the project menu (phone: left of the avatar); menu of radio items without search.
- 1.3 approved.
- 1.4 PenPot (base layout approved): "Projects · settings page, integrations" plus states secret revealed, secret copied, copying needs HTTPS, no forge integration, no webhook support, no publicUrl; lifecycle variants new subscription, invalid URL, delete subscription, empty, feature off, unrestricted URLs; "Phone · project settings page, integrations". The 7 "edit modal, lifecycle events" boards and the phone sheet are deleted. User asked to update the General tab too (task 1.5).
- 1.4 approved. Push checks, invalid glob and Context boards converted to "Projects · settings page, …"; all remaining modal boards deleted ("Projects · edit modal" became "Projects · new project").
- 1.5: user chose variant A (Project / Repository / Sandbox / Forge events, wake stays on General). Applied to the desktop page, saved state, phone page and the new-project modal. Spec "Grouped project form" made exact; design D7, D8 and Open Questions record the decisions.
- 1.6: mapping rows in `docs/design-workflow.md` renamed to the page boards; board names verified against PenPot.
- Design approved by the user; committed as `wip(ui-settings-and-task-sort): design` (cdaa0e1).

## Round 1 (2026-11-10)

- Implementer: 87e6838 (tasks 2-6), 9cdb78b (tasks 7-10) = HEAD_1; tasks 41/42 (7.2 skipped: no browser, Playwright MCP unreachable); checks fmt, clippy, machete, file-length, test-names, check-crates, docs-build, openspec validate, helm-unittest, web-typecheck all OK; cargo tests df-core/df-db/df-web pass; Bazel `test //...` pass (one flaky df-agent-channel test passed on rerun); container tests ran (docker.sock). Not run: 7.1 e2e tests (`just e2e-local`, needs kind cluster).
- 9.1: npm packages byte-identical (mermaid 11.17.2, htmx.org 4.0.0, alpinejs 3.17.5).
- Reviewed range: BASE...9cdb78b
- Security: 0 findings (nothing to verify).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `docs/content/using/_index.md:22-24` | Rail still lists Projects; settings sidebar lacks Projects | fix-now | fixed (181d3ad) |
| SP2 | Spec | Blocking | `deploy/helm/dark-factory/README.md:373` | Public key "in the tab Lifecycle events of a project's edit dialog" (old UI) | fix-now | fixed (181d3ad) |
| SP3 | Spec | Blocking | `docs/content/integrations/forges/webhooks.md` | No reference entry for `GET /projects/{key}/webhook-secret` (admin, no-store, 404) | fix-now | fixed (181d3ad) |
| SP4 | Spec | Blocking | `crates/df-web/src/sse/tests.rs:627` | Scenario "Live update keeps the sort" (`sort=created`, position kept) untested | fix-now | fixed (181d3ad) |
| SP5 | Spec | Blocking | `crates/df-e2e/tests/ui.rs:159` | Scenario "Sort on a phone" (390 px, order, no h-scroll) untested | fix-now | fixed (181d3ad) |
| SP6 | Spec | Blocking | `crates/dark-factory/src/forge.rs:362-374` | Real `ForgeWebhooks::setup`/`secret` mapping (Unsupported, NoSecret) untested | fix-now | fixed (181d3ad) |
| S1 | Standards | P1 | `crates/df-web/src/project_webhook.rs:61-116` | Private `Endpoint`/`endpoint` before `pub(crate) fn group` | fix-now | fixed (181d3ad) |
| S3 | Standards | P1 | `df-web/src/webhooks.rs`, `dark-factory/src/forge.rs`, `df-plugins/src/forge.rs` | `has_endpoint`/`accepts_webhooks` duplicate the new `setup`/`secret` accessors | fix-now | fixed (181d3ad) |
| S4 | Standards | P1 | `crates/df-core/src/config.rs:121,150-154` | `public_url: Option<String>` parsed twice; use `Option<Url>` | fix-now | fixed (181d3ad) |
| S5 | Standards | P1 | `crates/df-db/src/task_views.rs:196-200` | SQL CASE keyed on URL strings of `TaskSort::as_query` | fix-now | fixed (181d3ad) |
| S9 | Standards | P1 | `crates/df-web/src/components/shell.rs:937,945` | `Nav::ProjectSettings` → Projects mapping repeated | fix-now | fixed (181d3ad) |
| S2 | Standards | P3 | `crates/df-web/src/config_pages.rs` `tabbed_form` | Five branches on `page` bool | cosmetic | cosmetic |
| S6 | Standards | P3 | `crates/df-core/src/domain.rs:225` | UI labels for `TaskSort` in domain crate | cosmetic | cosmetic |
| S7 | Standards | P1 | `project_webhook.rs:88`, `secret-field.ts:16` | Mask string duplicated in Rust and TS | fix-now | fixed (181d3ad) |
| S8 | Standards | P3 | `project_webhook.rs:261` | Async block merges two errors | cosmetic | cosmetic |

User decision: Fix blocking + P1, and S7 moved P3 → P1 (fix-now). S2, S6, S8 stay cosmetic.

## Round 2 (2026-11-10)

- Implementer: 181d3ad = HEAD_2 (fix round 1: SP1-SP6, S1, S3, S4, S5, S7, S9); checks OK; container tests ran (df-db, df-web, dark-factory forge, Bazel df_web_test/dark_factory_test); e2e not run. S3 added `WebhookSetup::NotLoaded` (unknown plugin previously reported Ready). SP6 Unsupported tested via pure helper `webhook_setup`.
- Reviewed range: 9cdb78b...181d3ad
- Security: 0 findings. Round 1 items verified fixed by both reviewers. OIDC redirect URI unchanged by the `publicUrl` trailing slash (`oidc.rs:102` trims it).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-SP1 | Spec | Blocking | `docs/content/integrations/forges/webhooks.md:122` | Audit row says "details: user, project, plugin"; the user is the actor, details hold only project and plugin | fix-now | fixed (a142b81) |
| R2-SP2 | Spec | Blocking | `crates/df-web/src/project_webhook.rs:90` | New `WebhookSetup::NotLoaded` arm (bug fix: unknown plugin was Ready) has no test; the "not loaded" case goes through `ForgeServing::NotLoaded` | fix-now | fixed (a142b81) |
| R2-SP3 | Spec | P1 | `crates/df-e2e/tests/ui.rs:357-427` | Phone sort e2e test does not assert the menu is in the title row | fix-now | fixed (a142b81) |
| R2-S1 | Standards | P1 | `df-plugins/src/router.rs:67`, `df-plugins/src/forge.rs:82` | `has_webhook`/`accepts_webhooks` now test-only and repeat the setup rule | fix-now | fixed (a142b81) |
| R2-S3 | Standards | P1 | `crates/df-db/src/task_views.rs` (tests) | Single-variant asserts repeat the exhaustive `TaskSort::ALL` loop | fix-now | fixed (a142b81) |
| R2-S5 | Standards | P1 | `crates/dark-factory/src/forge.rs:61` | Test name "…has_no_setup_and_no_secret" is inaccurate (setup is NoSecret) and also asserts the absent-plugin case | fix-now | fixed (a142b81) |
| R2-SP4 | Spec | P3 | `crates/df-plugins/src/forge.rs:93-97` | "secret None when unsupported" untested by construction (no fixture without the export) | accepted | cosmetic |
| R2-S2 | Standards | P3 | `crates/df-core/src/config.rs:150-177` | Private serde helpers between public items (precedent at :450) | cosmetic | cosmetic |
| R2-S4 | Standards | P3 | `crates/df-web/src/sse/tests.rs:741` | `let position = row_position;` alias | cosmetic | cosmetic |
| R2-S6 | Standards | P3 | `crates/df-e2e/tests/ui.rs:392` | `let (_, newer) = (&keys[0], &keys[1]);` | cosmetic | cosmetic |

User decision: Fix blocking + P1 (R2-SP1, R2-SP2, R2-SP3, R2-S1, R2-S3, R2-S5). R2-S2, R2-S4, R2-S6 cosmetic.

## Round 3 (2026-11-10)

- Implementer: a142b81 = HEAD_3 (fix round 2: R2-SP1, R2-SP2, R2-SP3, R2-S1, R2-S3, R2-S5); checks OK; container tests ran (df-web project_webhook, df-db task_views, Bazel dark-factory/df-plugins); e2e not run.
- Reviewed range: 181d3ad...a142b81
- Security: 0 findings. Spec: 0 findings; R2-SP1, R2-SP2, R2-SP3 verified fixed. Standards: R2-S1, R2-S3, R2-S5 verified fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-S1 | Standards | P1 | `crates/df-db/src/task_views.rs:742` | Stale comment about the DF-3/DF-2 tie above an assert that only checks the default sort | fix-now | fixed (6e7e372) |
| R3-S2 | Standards | P1 | `crates/df-db/src/task_views.rs:755` | `keys(Updated) == keys(Updated)` compares two calls; a stable wrong order passes | fix-now | fixed (6e7e372) |

User decision: Fix blocking + P1 (R3-S1, R3-S2); fix round 3 of 3.

## Finish checks (2026-11-10)

- Fix round 3: 6e7e372 (R3-S1, R3-S2), checked by the orchestrator (test-only diff, no full review).
- Bazel `build --lockfile_mode=error //crates/... //plugins/...`: OK. `just docs-build`: OK.
- User decisions: e2e tests (`crates/df-e2e`) left to CI (status `untested`); roadmap row for webhook secret rotation skipped; 7.2 visual check run with Playwright.
- 7.2: `just serve-local` (plugins built with Bazel, dev cert via rcgen since no openssl/rustup in the sandbox) at 1440 and 390 px against the PenPot boards. Matches: settings sidebar, rail, new-project modal, project settings page header/tabs/`?tab=`, Integrations groups (no forge integration state), phone pages and sort menu without horizontal scroll. Not checked: Incoming webhook Ready state (no forge credential or webhook secret in dev config).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V1 | Visual | Blocking | `crates/df-web/src/components/forms.rs` `icon_menu` | At 1440 px the sort menu (`right-0`, w-56) opens leftwards and is clipped under the rail | fix-now | fixed (4a91216) |
| V2 | Visual | P3 | `crates/df-web/src/components/forms.rs` `icon_menu` | Check mark right of the label; board has it left | cosmetic | cosmetic |
| V3 | Visual | P3 | `crates/df-web/src/config_pages/project_page.rs` | Content full width; board uses a ~640 px column | cosmetic | cosmetic |

User decision: one extra fix round (beyond the limit of 3) for V1.

## Result (2026-11-10)

- Fix round 4 (extra, user-approved): 4a91216, V1 fixed. Re-checked in the browser: menu at x 231-455 at 1440 px (list pane ends at 456), x 97-321 at 390 px, no horizontal scroll.
- 7.2 ticked. Tasks 42/42.
- Status `untested`: no blocking finding open, but the df-e2e tests (`crates/df-e2e/tests/ui.rs`, `lifecycle_events.rs`, `webhooks.rs`) never ran (need the e2e kind cluster); left to CI by the user.
- Tech debt: none (no P2 findings). Roadmap: no row (user skipped the webhook secret rotation row).
- Cosmetic, not tracked: S2, S6, S8, R2-S2, R2-S4, R2-S6, R2-SP4, V2, V3.
