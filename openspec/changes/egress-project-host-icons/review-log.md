# Review log: egress-project-host-icons

- BASE: 4da424d495ddd14dc3ce9f3762b76ea6574abce9
- Store: none
- Status: done
- Implementer agent: a6edc6db9dd0fa636

## Round 1 (2026-10-25)

- Implementer: c3eb885, tasks 9/9, checks fmt/clippy/machete ok, df-web 309 pass, openspec validate ok, container tests ran (podman)
- Reviewed range: BASE...c3eb885

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `crates/df-web/src/egress/tests.rs:1591` | Details-panel check only greps `forge-forgejo` in rows (the table icon already matches); `flow_json` icon untested | fix-now: parse the row's `data-flow` JSON, assert `icon` | fixed (e3e780a) |
| SP2 | Spec | P1 | `crates/df-web/src/egress/tests.rs:1578` | Scenario "Project host before provider monogram" (GitHub instance) only unit-tested, no stream test (task 3.1) | fix-now: stream test with GitHub instance: `github.com` → `forge-github`, `api.github.com` → `GH` | fixed (e3e780a) |
| SP3 | Spec | P1 | `crates/df-web/src/egress/tests.rs:1540` | Scenario "Issue tracker on the repository host" (served) only unit-tested | fix-now: add tracker on repository host to served project in stream test | fixed (e3e780a) |
| S1 | Standards | P1 | `crates/df-web/src/components/project_host.rs:55` | Private `url_host` between public items (AGENTS.md file structure) | fix-now: move below `impl ProjectHosts` | fixed (3da1133) |
| S2 | Standards | P1 | `crates/df-web/src/egress/log/rows.rs:259` | `use` after fn in test module | fix-now: reorder | fixed (e3e780a) |
| S3 | Standards | P1 | `crates/df-web/src/components/project_host.rs:95` | Duplicates `config_pages.rs:758 project_forge_kind` | fix-now: one shared helper | fixed (e3e780a) |
| S5 | Standards | P1 | `crates/df-web/src/components/mod.rs:32` | `project_roles` re-exported but only used in module | fix-now: make private | fixed (e3e780a) |
| S10 | Standards | P1 | `crates/df-web/src/egress/tests.rs:1121` | `POST /credentials` response unchecked; `panic!` closures in helpers | fix-now: assert status; `expect` over panic closures | fixed (e3e780a) |
| S11 | Standards | P1 | `crates/df-web/src/config_pages.rs:29` | Separate `crate::components` import | fix-now: merge | fixed (e3e780a) |
| S4 | Standards | P3 | `project_host.rs:55` vs `config_pages.rs:530` | Two host parsers (`url_host` vs validating `repository_host`); stored projects pass form validation so behaviour matches | fix-now | fixed (e3e780a) |
| S6 | Standards | P3 | `crates/df-web/src/config_pages.rs:453` | `role_icon(&HostRole::Forge(..))` wraps `forge_icon` | fix-now | fixed (e3e780a) |
| S7 | Standards | P3 | `host_icon.rs:116`, `project_host.rs:171` | Duplicate test fixture `project()` | fix-now | fixed (e3e780a) |
| S8 | Standards | P3 | `crates/df-web/src/egress/tests.rs:1490` | `icon_before`/`icon_after` near-duplicates | fix-now | fixed (e3e780a) |
| S9 | Standards | P3 | `crates/df-web/src/egress/tests.rs` | ~20 `Viewer::new(filter, Default::default())` sites | fix-now | fixed (e3e780a) |

User decision: address all findings (blocking, P1 and P3: S4, S6, S7, S8, S9 included)

## Round 2 (2026-10-25)

- Implementer: e3e780a, fix round 1 (all 14 items), checks fmt/clippy/machete ok, df-web 310 pass, container tests ran (podman)
- Reviewed range: c3eb885...e3e780a

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | Blocking (regression of S1) | `crates/df-web/src/components/project_host.rs:70-88` | Private `project_roles` (S5 fix) now sits between public items | move below `impl ProjectHosts` next to `url_host` | fixed (3da1133) |
| R2-S2 | Standards | P1 | `crates/df-web/src/components/project_host.rs:119` | No blank line before `url_host`, no doc comment | add blank line + one-line doc referencing `repository_host` | fixed (3da1133) |
| R2-S3 | Standards | P3 | `crates/df-web/src/egress/tests.rs` (`flow_of`) | `flow_of` repeats attribute scan of `icon_value` and unescape of `rows.rs:386` test | one `attr_value` test helper | cosmetic |

Spec axis: 0 findings; SP1–SP3 confirmed fixed.

User decision: fix blocking + P1 (R2-S1, R2-S2); R2-S3 cosmetic

## Round 3 (2026-10-25)

- Implementer: 3da1133, fix round 2 (R2-S1, R2-S2), checks fmt/clippy/machete ok, df-web 310 pass, container tests ran (podman)
- Reviewed range: e3e780a...3da1133 (pure item move + doc comment in `project_host.rs`; verified by the orchestrator, reviewer agents not re-spawned)

No new findings. R2-S1, R2-S2 fixed.

## Result

- Status: done. No blocking findings open.
- Tech debt: none (no P2 findings).
