# Review log: egress-rule-editing

- BASE: 628b546f35cca77c1f960ed70b9963d359d99d3c
- Store: none
- Status: done
- Implementer agent: a496273aa0d98a997 (iteration 1 started 2026-11-20)
- DOCKER_HOST: unix:///var/run/docker.sock

## Design (task 1.1)

- Status: approved by the user (2026-11-20), commit a9ded70abb7df79f050f2ffaa9620232e5179e5d
- Boards: Network (edit rule, edit rule task, edit deny rule, edit cluster service, edit rule already exists); Config - Projects (settings page egress, instance rules open, empty, make global; phone egress). Mapping rows in `docs/design-workflow.md`.
- Decisions: edit dialog without "Delete rule"; effect toggle shown disabled in the host and deny rule dialogs (the cluster-service dialog has none, like its add dialog); "Make global" confirms inline in the row (spec updated), not via `hx-confirm`.
- Known design-file defect (not from this change): project settings boards keep a white panel in the Dark theme preview.
- Extra item from the user (2026-11-20): on `/egress/allowlist` below 1440 px the header actions (Live traffic, Add cluster service, New egress rule) wrap and become left-aligned; they must stay right-aligned like on other pages. Handed to the implementer with this change.

## Round 1 (2026-11-20)

- Implementer: HEAD_1 `4a9a0df7`, tasks 18/18, checks: fmt, clippy, machete, file length, docs-build ok; nextest df-db, df-web, df-agent-channel, df-orchestrator 1882 passed; 4 new e2e tests pass (also repeated). Container tests ran.
- Not run: full e2e suite (aborted at `chats`, cluster database reset while the disk was full); Bazel test of task 6.2 (`//crates/df-web/... //crates/df-db/...`).
- Deviations reported: Egress tab is a lazy panel inside the project `<form>` (D5 says outside); "Save project" hidden by CSS on that tab; `task_id` added to `EntryRecord`; deny rule create/delete/edit now publish `egress_policy`.
- Reviewed range: BASE...4a9a0df7
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P2 | Spec | Blocking | `crates/df-web/src/egress/project_tab.rs:359`, `egress/allowlist.rs:174` | Instance cluster-service rows link to `?q=ns/svc`, not a highlighted row; `is_highlighted` excludes cluster services | fix-now | fixed (09c777ec) |
| C1 | Process | Blocking | task 6.2 | Bazel test `//crates/df-web/... //crates/df-db/...` and the full e2e suite not run | fix-now | fixed (09c777ec) |
| S1 | Standards | P1 | `egress/allowlist/dialogs.rs`, `egress/project_tab.rs`, `df-db/src/egress/update.rs` | Private helpers before public items | fix-now | fixed (09c777ec) |
| S2 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:84` | `no_sideways_scrolling` duplicates the check in `tests/ui.rs`; belongs in `src/browser.rs` | fix-now | fixed (09c777ec) |
| S3 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:31` | Tests add rules to seeded shared projects, cleanup only at the end of the body | fix-now | fixed (09c777ec) |
| S4 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:200` | Wait without context | fix-now | fixed (09c777ec) |
| S5 | Standards | P1 | `egress/allowlist/dialogs.rs:283-389` | `answer`/`refreshed` repeat the `ReturnTo` match; `new_host`/`new_service` near-identical | fix-now | fixed (09c777ec) |
| P3 | Spec | P1 | `design.md` D5, Risks | Design still says tab outside the form, `task_project_key` only, connections not cut | fix-now | fixed (09c777ec) |
| P4 | Spec | P1 | `docs/content/using/network-access.md` | "this deny rule already exists" missing | fix-now | fixed (09c777ec) |
| P5 | Spec | P1 | `docs/content/security/egress-gateway.md:53` | Egress tab mention does not link to `setup.md#rules-of-one-project` | fix-now | fixed (09c777ec) |
| S9 | Standards | P1 (user, was P2) | `egress/allowlist/dialogs.rs:328` | `find_entry` loads the whole allowlist to find one id | fix-now | fixed (09c777ec) |
| P6 | Spec | P1 (user, was P2) | `egress/project_tab.rs` | Tab inside the project `<form>`: htmx sends the form fields with delete/make-global requests | fix-now | fixed (09c777ec) |
| S8 | Standards | P1 (user, was P3) | `egress/allowlist/dialogs.rs:391` | `actor_of` clones roles; two Actor patterns | fix-now | fixed (09c777ec) |
| S7 | Standards | P2 | `egress/allowlist/dialogs.rs`, `project_tab.rs:291` | `task:<KEY>` / `project:<KEY>` encoded by string ops in several places; matters when another scope kind is added | tech-debt | tech-debt (TD-128) |
| S6 | Standards | P3 | `egress/project_tab.rs:87,146` | Instance predicate and tab URL written twice | cosmetic | cosmetic |
| P1 | Spec | Rejected | `egress/allowlist/dialogs.rs:155` | No effect toggle in the cluster-service edit dialog | reject | rejected (approved board "edit cluster service" has no effect toggle, matching the add dialog) |

User decision: fix Blocking + P1, plus S9, P6 and S8 (moved to P1). S7 stays tech debt, S6 cosmetic.

## Round 2 (2026-11-20)

- Implementer: HEAD_2 `09c777ec` (09c777ecd6e90b33717eb7e32cbfd6bc56fd705d), fix round 1. Checks: fmt, clippy, machete, file length, docs-build ok; nextest df-db, df-web, df-e2e, df-agent-channel 1789 passed after fixing the `demo_seed` count for the new seeded projects EG and EO; Bazel `//crates/df-web/... //crates/df-db/...` pass; full `just e2e-local` pass (E2E_MIN_FREE_GIB=8).
- Notes: P6 uses `form="egress-actions"` on the make-global POST (htmx 4 includes the enclosing form only for POST/PUT/PATCH); P5 links to `setup.md#projects` (the anchor named in the fix list does not exist there).
- Reviewed range: 4a9a0df7...09c777ec
- Security: 0 findings. Standards and Spec confirm S1–S5, S8, S9, P2–P6 fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P7 | Spec | Blocking | `crates/df-web/src/egress/allowlist/dialogs.rs:294,358` | Dialogs on the project page use `id="form"` / `hx-target="#form"` like the project form (`config_pages.rs:524`), which comes first in the document: a rejected save from the Egress tab (duplicate, rule gone, validation) replaces the project form and the dialog shows no message. Present since 4a9a0df7; no test | fix-now | fixed (1d827ced) |
| S10 | Standards | P1 | `crates/df-e2e/src/seed.rs:98` | `#[expect(clippy::too_many_lines)]` reason names no follow-up; the table can be split now | fix-now | fixed (1d827ced) |
| S11 | Standards | P1 | `crates/df-db/src/egress/update/tests.rs:142` | `get_entry` test lives in the update tests, not next to `egress.rs` tests | fix-now | fixed (1d827ced) |
| S12 | Standards | P1 | `egress/allowlist/dialogs.rs:194,463` | `respond(.., true)` / `new_dialog(.., false)`: bare boolean flags | fix-now | fixed (1d827ced) |
| S13 | Standards | P3 | `egress/tests/cluster_highlight.rs:9` | `add_service` helper not with the other DB helpers in `egress/tests/mod.rs` | cosmetic | cosmetic |

User decision: Fix blocking + P1 (P7, S10, S11, S12); S13 cosmetic.

## Round 3 (2026-11-20)

- Implementer: HEAD_3 `1d827ced` (1d827cede10e6b74c309bc62b6afa2f868f363ee), fix round 2. Checks: fmt, clippy, machete, file length, docs-build ok; nextest df-db, df-web, df-e2e 1506 passed; e2e `egress_rules` 4 tests pass with E2E_REPEAT=2.
- Notes: dialog forms use `id="rule-form"`; the e2e "No rules for this project yet" assertion was dropped because two parallel tests share project EG (empty state stays covered by df-web tests).
- Reviewed range: 09c777ec...1d827ced
- Security: 0 findings. Spec: 0 findings, P7 confirmed fixed. Standards: S10–S12 confirmed fixed; sharing project EG between parallel e2e tests judged safe (assertions keyed to each test's unique host).
- Status updates: P7, S10, S11, S12 fixed (1d827ced).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S15 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:292` | "Add rule from the Egress tab" sequence repeated in three tests | fix-now | fixed (9eb693a3) |
| S14 | Standards | P1 | `egress/allowlist/dialogs.rs:228` | `DIALOG_FORM` constants far below the imports; id written twice | fix-now | fixed (9eb693a3) |
| S16 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:328` | Scoped button lookup written inline with full paths; `ui.rs:40` has its own copy | fix-now | fixed (9eb693a3) |
| S17 | Standards | P1 | `crates/df-e2e/tests/egress_rules.rs:402` | Label "on the empty phone tab" no longer true | fix-now | fixed (9eb693a3) |

User decision: fix P1 (S14–S17) in a last fix round, then finish.

## Fix round 3 and finish (2026-11-20)

- Implementer: HEAD_4 `9eb693a3` (9eb693a33b6c18b97ab0a85e0c4098fbf40989ef). Checks: fmt, clippy, file length ok; nextest df-web, df-e2e 1050 passed; e2e `egress_rules` (4) and `ui` (12) pass. Also updated `docs/design-workflow.md` (project settings page now has four tabs).
- Review of 1d827ced...9eb693a3 by the orchestrator (refactor of tests and two constants, no behavior change): S14, S15, S16, S17 fixed (9eb693a3). The `dialog_form_id!` macro that writes the id once is more than needed (P3, cosmetic).
- Finish: container tests ran in every round; no `Cargo.toml` / `Cargo.lock` change, so no extra Bazel build (Bazel test of task 6.2 passed in round 2); `just docs-build` ok.
- Tech debt: S7 → TD-128; design-file dark panel → TD-129. No new roadmap rows (the change's rows and "Make-global hint for cluster-service rules" exist).
- Docs pages changed by the change: `docs/content/security/egress-gateway.md`, `docs/content/using/network-access.md`, `docs/content/using/setup.md`.
