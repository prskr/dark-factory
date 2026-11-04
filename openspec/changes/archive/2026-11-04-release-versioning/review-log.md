# Review log: release-versioning

- BASE: e7b6843bbe72742c812c9c66cb607a30fb9d00c8
- Store: none
- Status: done (open by design, need GitHub: 5.1 branch run, 6.1 command check, 6.2 first release, 5.2 PR check)
- Implementer agent: ac82d9b71c0771a36
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock

## Design (2026-11-03)

- Task 4.1 (Penpot rail board): approved, commit 7bcb115; Rail component avatar, `Profile menu` component with version footer, board `Tasks / Desktop · profile menu` (menu above the avatar, user request; D6 adjusted)

## Round 1 (2026-11-03)

- Implementer: b17ef60 (tasks 1-3), 4eee4a5 (tasks 4-6), tasks 14/18, checks fmt/clippy/machete/bazel lockfile build ok, xtask/df-telemetry/df-web/helm/bazel version_test pass; container tests ran (podman socket)
- Open by design (need GitHub): 5.1 branch run, 6.1 command check, 6.2 first release. 4.3 browser check not done: `just serve-local` ports 5432/5556 held by another worktree's compose stack.
- Implementer note: some tests written together with the implementation (no separate red phase).
- Reviewed range: e7b6843...4eee4a5

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-telemetry/BUILD.bazel:116`, `tests/version.rs` | No test builds df-telemetry with a real version; CI only ever checks `dev` (scenario "Release binary", task 2.3) | | open |
| P3 | Spec | Blocking | `crates/df-telemetry/src/otel.rs:677` | Scenario "Operator override" (`service.version=custom`) has no test | | open |
| P2 | Spec | P1 | `crates/df-telemetry/src/otel.rs:673` | Test hardcodes `dev`; fails under `--//build:version=…` | | open |
| S1 | Standards | P1 | `crates/df-web/src/components/shell.rs:259,285` | Data clump: `rail`/`profile_menu` take name+version instead of `&Shell` | | open |
| S2 | Standards | P1 | `config_pages.rs:428`, `task_pages/view.rs:1020` | Duplicated phone profile-menu wrapper | | open |
| S3 | Standards | P1 | `crates/df-web/src/layout.rs:50-73` | `HTMX_CONFIG` const below public types (file structure rule) | | open |
| S4 | Standards | P1 | `docs/design-system.md:447` | `layout(...)` signature stale in docs | | open |
| S7 | Standards | P1 | `crates/df-web/src/egress/tests/rules.rs:461` | Egress test counts the profile-menu avatar's `bg-accent-soft` | | open |
| S9 | Standards | P1 | `tools/xtask/src/next_version.rs:1254` | Test name `compares_numerically` doesn't state the behavior | | open |
| P4 | Spec | P2 | `.github/workflows/release.yaml:228` | Harness digest read back via `imagetools inspect` after push, not from the push; matters when the tag is pushed by hand during a release | | open |
| S5 | Standards | P3 | `config_pages.rs:1111` | `save_project` builds `Shell::new` instead of extractor | | open |
| S6 | Standards | P3 | `crates/df-web/src/layout.rs:643` | `Shell` name vs page-frame meaning | | open |
| S8 | Standards | P3 | `crates/df-telemetry/tests/version.rs` | Test mirrors `version_or_dev` logic | | open |
| S10 | Standards | P3 | `tools/xtask/src/next_version.rs:1216` | Shells out to `date -u` | | open |
| X- | Security | — | — | No findings | | — |

User decision: Fix blocking + P1 + P4 + S10 (P1, P3, P2, S1, S2, S3, S4, S7, S9, P4, S10); S5, S6, S8 cosmetic. 4.3 browser check in this round (user frees ports 5432/5556).

## Round 2 (2026-11-03)

- Implementer: 71dcbc7, fixed P1 P2 P3 P4 (digest from `bazel build //deploy/images/claude-harness:digest`, not verified against a real push) S1 S2 S3 S4 S7 S9 S10; checks ok; container tests ran
- 4.3 browser check not done: Playwright MCP disconnected, obscura has fixed 1280 px viewport
- Reviewed range: 4eee4a5...71dcbc7

Round 1 status updates: P2, P3, P4 fixed (71dcbc7); S1, S2, S3, S4, S7, S9, S10 fixed (71dcbc7); P1 not fixed (see P5).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P5 (P1) | Spec | Blocking | `crates/df-telemetry/BUILD.bazel:127-146` | `stamped_version_test` tests a separate copy with its own env file; removing `_VERSION_ENV` from the production libraries passes all tests | | open |
| S11 | Standards | P1 | `docs/design-system.md:201` | `rail` doc signature stale | | open |
| S14 | Standards | P1 | `tools/xtask/src/next_version.rs:128` | Test name `year_month_of_known_timestamps` is a topic, not a behavior | | open |
| S13 | Standards | P1 | `tools/xtask/src/next_version.rs:84-87` | Silent `unwrap_or` sentinels on by-construction invariants | | open |
| S15 | Standards | P1 | `docs/design-system.md:206` | `layout`/`workspace_layout` prose muddled | | open |
| S12 | Standards | P3 | `tools/xtask/src/next_version.rs:67-88` | Hand-rolled civil date instead of chrono; xtask is a standalone workspace without chrono, so this would add a dependency (reviewer: keeping is defensible) | | cosmetic |
| S16 | Standards | P3 | `crates/df-web/src/egress/tests/rules.rs:461` | Assertion depends on class order | | cosmetic |
| X- | Security | — | — | No findings | | — |

User decision (round 2): Fix blocking + P1 + S12 (P5, S11, S13, S14, S15, S12 via chrono in xtask); S16 cosmetic.

Round 2 status updates: S11, S12, S15 fixed (dc17cb7); S13, S14 removed with S12 (dc17cb7); P5 pending round 3 verification.

## Round 3 (2026-11-03)

- Implementer: dc17cb7, P5 via `build/with_version.bzl` transition wrapper (fails without `_VERSION_ENV`: shown), S12 chrono 0.4.45 in tools/xtask, S11, S15 docs; checks ok; root `cargo deny check` warnings only; tools/xtask advisories RUSTSEC-2026-0381/0382 pre-existing (ureq → rustls-webpki/webpki-roots)
- Reviewed range: 71dcbc7...dc17cb7
- Orchestrator finding O1 (P1): `tools/xtask/Cargo.toml` chrono feature `clock` pulls iana-time-zone, winapi and ~25 crates; `Utc::now()` only needs `now` (= std), verified in chrono 0.4.45 manifest

Round 3 status updates: P5/P1 fixed (dc17cb7); S11, S12, S13, S14, S15 fixed (dc17cb7).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P7 | Spec | Blocking (convergence: in code the last round changed) | `build/with_version.bzl:12-24`, `crates/df-telemetry/tests/version.rs:7-17` | Both sides of the comparison derive from the flag; a broken transition gives `dev == dev` and passes | | open |
| P6 | Spec | P1 | `crates/df-telemetry/BUILD.bazel:63-82` | `df_telemetry_slim` (linked by the factory-agent binary) has no stamped test | | open |
| O1 | Orchestrator | P1 | `tools/xtask/Cargo.toml` | chrono `clock` instead of `now` pulls ~25 extra crates | | open |
| S18 | Standards | P1 | `build/with_version.bzl:1`, `crates/df-telemetry/BUILD.bazel:128` | Docstring says "default configuration" for a transitioned build | | open |
| S17 | Standards | P3 | `build/with_version.bzl:12` | `_impl` vs repo convention `_<rule>_impl` | | open |
| S19 | Standards | P3 | `tools/xtask/Cargo.toml:49` | `chrono` not in alphabetical order | | open |
| X- | Security | — | — | No findings | | — |

User decision (round 3): Fix blocking + P1 (P7, P6, O1, S18); S17, S19 cosmetic.

## Round 4 (2026-11-03)

- Implementer: 96f6461, P7 (`DF_EXPECTED_VERSION` via RunEnvironmentInfo; no-op transition makes both stamped tests fail: shown), P6 (`version_slim_test`, `stamped_version_slim_test`; fails without slim `_VERSION_ENV`: shown), O1 (chrono `now`, 22 crates removed from xtask lock), S18; checks ok
- Reviewed range: dc17cb7...96f6461

Round 4 status updates: P7, P6, O1, S18 fixed (96f6461).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S20 | Standards | P3 | `crates/df-telemetry/BUILD.bazel:117-151` | `version_test`/`version_slim_test` (and their wrappers) duplicated; two copies, reviewer: fine as is | | open |
| P8 | Spec | P3 | `crates/df-telemetry/tests/version.rs:9-12` | Fixed check depends on `DF_EXPECTED_VERSION` being set; not a live bug (rule always sets it), only guards a future edit of the rule | | open |
| X- | Security | — | — | No findings | | — |

Note: both are nits in code the last round changed; classified non-blocking because neither is a defect today (orchestrator judgement, user to confirm).

User decision (round 4): fix S20 + P8 in one more small round (beyond the 3-round limit, user's choice), then finish.

## Round 5 (2026-11-03)

- Implementer: e7ec3e3, S20 (list comprehensions), P8 (`DF_REQUIRE_EXPECTED_VERSION` guard; dropping `DF_EXPECTED_VERSION` fails both stamped tests: shown); Bazel df-telemetry 6/6, cargo version test ok
- Reviewed range: 96f6461...e7ec3e3
- Round 4 status updates: S20, P8 fixed (e7ec3e3)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21 | Standards | P3 | `build/with_version.bzl:23-26`, `tests/version.rs:10-14` | Guard only catches a half-edit of the env dict; if the wrapper env is lost entirely, both vars vanish together | | open |
| P9/S23 | Spec/Standards | P3 | `build/with_version.bzl:41` | Rule `doc` doesn't mention `DF_REQUIRE_EXPECTED_VERSION` | | open |
| S22 | Standards | P3 | `crates/df-telemetry/tests/version.rs:4-6` | "both are unset" now ambiguous (three vars); long line | | open |
| X- | Security | — | — | No findings | | — |

User decision (round 5): finish; P9/S23, S22 cosmetic; S21 rejected (if the wrapper env is lost, `DF_VERSION_ENV_FILE` is lost with it, the test expects `dev` and fails against the transitioned `2026.11.0`).

## Finish (2026-11-03)

- Bazel `build --lockfile_mode=error //crates/... //plugins/...`: ok (112 actions)
- Tech debt: no new P2 entries (P4 was fixed); TD-99 added by the implementer (task 6.3; TD-93 before the rebase onto main, which had taken TD-93 to TD-98). Roadmap rows added in task 6.3.
- Cosmetic: S5, S6, S8, S16, S17, S19, S22, P9/S23
- `just bazel-test` (//..., container tests via podman): 44/45 pass; `//crates/df-web:df_web_test` fails in `sse::tests::streams_end_when_the_session_token_is_due_for_refresh` (1 of 3 reruns). Known flaky TD-83 (wall clock), not touched by this change.
- Open blocking findings: none.
- Unverified: 4.3 browser check at 1440/390 px (Playwright MCP disconnected, obscura fixed at 1280 px); 5.1 branch run, 6.1 command check, 6.2 first release, 5.2 PR job check (need GitHub); P4 digest equality against a real push (verified from rules_img sources only).
- TD IDs created: TD-99 (by the implementer, task 6.3).
- Browser check (task 4.3, 2026-11-03, `just serve-local`, Playwright MCP): 1440 px: rail avatar "Account menu of Local Admin", menu opens above the avatar left-aligned, footer `Dark Factory dev`, no log-out button outside the menu, Log out ends the session (redirect to Dex). 390 px: phone menu with the same entries under the top-right avatar. Passed.
