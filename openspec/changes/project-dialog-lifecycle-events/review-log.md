# Review log: project-dialog-lifecycle-events

- BASE: 2a3ba54c3f7241e2acacdd069cf09406fc122ff5
- Store: none
- Status: done
- Implementer agent: a2706d319315ae042

Notes:
- Prerequisite `lifecycle-events` is not archived (manual tasks 5.3, 5.4 open). User chose to implement anyway; archiving this change waits for it.
- Design: all boards exist in PenPot ("Config - Projects"); no orchestrator design work needed. Task 4.2 (mapping row) is left to the implementer.
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock

## Round 1 (2026-11-04)

- Implementer: 4d035911, tasks 10/10, checks fmt/clippy/machete ok, `cargo test -p df-web` 588 pass, `just bazel-test` 46/46, docs-build ok, openspec validate ok; container tests ran (e2e `project_subscription` filter only)
- Reviewed range: BASE...4d035911

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P5 | Spec | Blocking | `crates/df-web/src/project_subscriptions.rs:171-188,335` | "New" and row edit can be open together: duplicate `#subscription-editor`/`#url`, both editors bound to `subscription-form` (submit posts both), 422 swaps the wrong editor (verified) | fix-now | fixed (bcfc0f16) |
| P1 | Spec | Blocking | `project_subscriptions/tests.rs` | Scenario "Unsaved subscription form" (Save project with stray `url`) has no test | fix-now | fixed (bcfc0f16) |
| P2 | Spec | Blocking | `project_subscriptions/tests.rs` | Scenario "Feature off" does not check that subscriptions are still listed | fix-now | fixed (bcfc0f16) |
| P3 | Spec | P1 | `project_subscriptions/tests.rs` | Create response not checked for "No delivery yet" | fix-now | fixed (bcfc0f16) |
| P4 | Spec | P1 | `crates/df-e2e/tests/lifecycle_events.rs:115` | e2e submits only with Enter; design risk says Enter and button | fix-now | fixed (bcfc0f16) |
| S1 | Standards | P1 | `config_pages/tests/lifecycle_tab.rs:1` | Missing `//!` module doc | fix-now | fixed (bcfc0f16) |
| S2 | Standards | P1 | `config_pages.rs:1083-1088` | Admin check decided twice (tab vs. form anchor) | fix-now | fixed (bcfc0f16) |
| S3 | Standards | P1 | `project_subscriptions.rs:~178` | URL input duplicates `components::forms::text_field`, already drifted | fix-now | fixed (bcfc0f16) |
| S10 | Standards | P1 | `assets/tabs.js:6-7` | Header does not list `reveal()` | fix-now | fixed (bcfc0f16) |
| P6 | Spec | P2 | `push_policy/tests.rs` | 390 px tab visibility only checked manually; matters when the Context tab lands (4 tabs) | tech-debt | tech-debt (TD-104) |
| S4 | Standards | P3 | `project_subscriptions.rs:526` | `retargeted` duplicates `config_pages::saved` shape | cosmetic | cosmetic |
| S5 | Standards | P3 | `project_subscriptions.rs` | Repeated element ids as string literals | cosmetic | cosmetic |
| S6 | Standards | P3 | `project_subscriptions.rs:141` | `EditorTarget::new` naming | cosmetic | cosmetic |
| S7 | Standards | P3 | `config_pages.rs:707` | `Editing::No` naming | cosmetic | cosmetic |
| S8 | Standards | P3 | `push_policy/tests.rs:360` | Tab row test lives in push policy tests | cosmetic | cosmetic |
| S9 | Standards | P3 | `project_subscriptions/tests.rs:~366` | Edit-dialog structure test outside `lifecycle_tab.rs` | cosmetic | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1 (P5, P1, P2, P3, P4, S1, S2, S3, S10). P6 → tech debt, S4–S9 cosmetic.

## Round 2 (2026-11-04)

- Implementer: bcfc0f16, fixed P5, P1, P2, P3, P4, S1, S2, S3, S10; clippy/machete ok, `cargo test -p df-web` 590 pass, `bazel test //...` 46/46, docs-build ok; e2e `project_subscription` ran (after ENOSPC the implementer deleted a kind cluster, `target/debug/incremental`, `target/docs-site`)
- Reviewed range: 4d035911...bcfc0f16

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P7 | Spec | P1 | `design.md` D3 / `project_subscriptions.rs:330-355,425-459` | D3 still says `new`/`edit` return an editor fragment into a slot; code now returns the whole region with one open editor | fix-now | fixed (7dcbf317) |
| S11 | Standards | P1 | `project_subscriptions.rs:415,433,452,531` | `region_markup` match-into-response copied 4×, two copies added this round | fix-now | fixed (fdfab3fa) |
| S12 | Standards | P1 | `crates/df-e2e/tests/lifecycle_events.rs:81-84,134` | `subscribe()` now also runs the edit step; doc is stale | fix-now | fixed (fdfab3fa) |
| S13 | Standards | P1 | `project_subscriptions/tests.rs:474-475` | New-editor case asserts `contains("")`, always true | fix-now | fixed (fdfab3fa) |
| O1 | Spec | P1 | `project_subscriptions.rs` editor URL field | URL input lost `font-mono` (design draws it mono) after S3 | fix-now | fixed (fdfab3fa, test 5d9d34a2) |
| S14 | Standards | P3 | `config_pages/tests/lifecycle_tab.rs:84-90` | `unwrap` instead of `Result<()>`; matches existing module style | | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1 (P7 by orchestrator in design.md; S11, S12, S13, O1 by implementer). S14 cosmetic.

## Round 3 (2026-11-04)

- Orchestrator: 7dcbf317 (design.md D3, P7)
- Implementer: fdfab3fa, fixed S11, S12, S13, O1; clippy/machete ok, `cargo test -p df-web` 590 pass, `bazel test //...` 46/46; e2e `project_subscription` ran
- Reviewed range: 7dcbf317...fdfab3fa

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8/S15 | Spec+Standards | Blocking | `crates/df-web/src/project_subscriptions/tests.rs:501` | `contains("font-mono")` checks the whole tab, which always has `font-mono` (key panel, row URLs, event labels): the O1 test passes without the fix (verified, 5 uses in source) | fix-now | fixed (5d9d34a2) |
| S16 | Standards | P1 | `project_subscriptions.rs:176`, `components/forms.rs:121` | Bare `mono: bool` positional argument, one caller always `true`; repo pattern is class concatenation | fix-now | fixed (5d9d34a2) |

Security: 0 findings.

User decision: Fix blocking + P1 (P8/S15, S16).

## Round 4 (2026-11-04)

- Implementer: 5d9d34a2, fixed P8/S15 (mutation-checked), S16; clippy/machete ok, `cargo test -p df-web` 590 pass, `bazel test //...` 46/46; e2e not rerun (no e2e/htmx change)
- Reviewed range: fdfab3fa...5d9d34a2

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17 | Standards | P1 | `project_subscriptions/tests.rs:12-21` | `url_input` inserted between `tab`'s doc line and `async fn tab`: wrong doc on `url_input`, none on `tab` | fix-now | fixed (11df6b5a) |
| S18 | Standards | P1 | `project_subscriptions/tests.rs:14-18` | `url_input` duplicates `testkit::tag_around`, already imported | fix-now | fixed (11df6b5a) |

Security: 0 findings. Spec: 0 findings.

User decision: Fix blocking + P1 (S17, S18).

## Round 5 (2026-11-04)

- Implementer: 11df6b5a, fixed S17, S18 (tests only); clippy ok, `cargo test -p df-web project_subscriptions` 15 pass
- Reviewed range: 5d9d34a2...11df6b5a, checked by the orchestrator (test-only diff); no findings

## Finish

- Container tests: ran (e2e `project_subscription` in rounds 1–2, df_test_postgres tests every round); none skipped
- Bazel build: not needed (no Cargo.toml/Cargo.lock change); `bazel test //...` 46/46 at 5d9d34a2
- Docs: `just docs-build` ok. Pages: integrations/lifecycle-events.md, authentication/{keycloak,dex,_index}.md, using/setup.md; also docs/development.md, docs/design-workflow.md, deploy/helm/dark-factory/README.md
- Tech debt: TD-104 (P6)
- Roadmap: nothing new ("Context" tab already in `iter3-project-context`)
- Archive blocked until `lifecycle-events` is archived
