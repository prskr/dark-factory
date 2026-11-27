# Review log: web-htmx-robustness

- BASE: 5931fa9297d97ec558346ee80d2aa1c0ae8921ea
- Store: none
- Status: done

## Round 1 (2026-11-27)

- Implementer: 4aeaa60d, tasks 25/25, checks fmt/clippy/machete/tsc/openspec validate/docs-build ok, df-web nextest 1168 passed (PostgreSQL via docker socket), container tests skipped: all e2e (`just e2e-local` failed at cluster setup: no DNS for code.forgejo.org, test-project image pull failed) incl. egress_rules, transcript, opening_tasks_from_the_list… (6.4), a_message_post_without_an_answer_shows_the_network_toast_and_keeps_the_text (4.2); browser checks of 2.2 (390 px offset, screen reader) not run
- Reviewed range: BASE...4aeaa60d

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-e2e/tests/ui.rs:838,903` | e2e tests for network toast (4.2) and focus (6.4) never ran | run e2e | open |
| P2 | Spec | Blocking | `crates/df-e2e/tests/ui.rs` | failed-selection test doesn't assert open task stays shown; stub lacks Content-Type | fix-now | open |
| P3 | Spec | Blocking | `crates/df-web/scripts/composer.ts:46-56` | composer memory scenarios only source-string tests; `$nextTick` preselect unrun | fix-now | open |
| P4 | Spec | Blocking | `crates/df-web/src/layout.rs:674`, tasks.md 2.2 | 2.2 ticked, 390 px offset and screen-reader checks not done | fix-now | open |
| P5 | Spec | Blocking | — | "Live updates reconnect → no toast", "Live update does not move focus" untested | fix-now | open |
| P6, S2 | Spec+Standards | Blocking | `crates/df-web/src/layout.rs:270` | `id="composer-slot"` literal instead of `Region::ComposerSlot.id()` | fix-now | open |
| P7, S3 | Spec+Standards | Blocking | `docs/content/using/tasks.md` | task-head focus not documented | fix-now | open |
| S1 | Standards | P1 | `crates/df-e2e/tests/ui.rs:833` | focus checks appended to unrelated e2e test (one behavior per test) | fix-now | open |
| S6 | Standards | P1 | `task_pages/view.rs:648`, `chat_pages/view.rs:198` | focus head markup and tests duplicated | fix-now | open |
| S7 | Standards | P1 | `layout.rs:579`, `assets.rs:242` | `COMPONENT_SCRIPTS` doc stale; plain-script list hard-coded in test | fix-now | open |
| S4 | Standards | P1 | `crates/df-web/src/htmx.rs:95` | private helper before public items | fix-now | open |
| S9 | Standards | P1 | `crates/df-web/scripts/composer.ts:89` | three statements on one line | fix-now | open |
| S10 | Standards | P1 | `_typos.toml:16` | stale comment above `IIFEs?` | fix-now | open |
| S8 | Standards | P1 | 6 test files | toast-partial literal repeated in tests | fix-now (user moved from P3) | open |
| S5 | Standards | P2 | `workspace.ts:53`, `chatLayout.ts` | `select(row)` duplicated | tech-debt | open |

Security: 0 findings.

User decision: Fix blocking + P1, plus S8 (moved P3 → P1).

## Round 2 (2026-11-27)

- Implementer: 3530143f, tasks 25/25, checks fmt/clippy/machete/tsc/openspec validate/docs-build ok, df-web 1169 passed (PostgreSQL), e2e full suite ran (`just e2e-local`): all passed; the 3 composer tests passed in ui.rs and were then moved to tests/composer.rs (only compiled after the move). Screen-reader check of 2.2 manual, open.
- Reviewed range: 4aeaa60d...3530143f

Round 1 rows: P2, P4, P5, P6/S2, P7/S3, S1, S4, S6, S7, S8, S9, S10 fixed (3530143f). P1 partly fixed: full e2e passed, but the moved composer tests (tests/composer.rs) never ran as their own binary; P3 partly fixed (see R2-P2, R2-P3).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking (carried) | `crates/df-e2e/tests/composer.rs` | moved composer e2e tests not run | run e2e | open |
| R2-P2 | Spec | Blocking | `crates/df-e2e/tests/composer.rs:35-55` | assignee memory not asserted | fix-now | open |
| R2-P3 | Spec | Blocking | `crates/df-e2e/tests/composer.rs` | "Storage blocked" scenario has no behavioral test | fix-now | open |
| R2-S1 | Standards | Blocking | `crates/df-e2e/tests/ui.rs:1070,1076` | `.first()` without the comment the e2e skill requires | fix-now | open |
| R2-P1 | Spec | P1 | `crates/df-e2e/tests/ui.rs:313-358` | failed selection uses real 404, scenario names 500 | accept 404, note in tasks.md 6.4 + test comment | open |
| R2-S2 | Standards | P1 | `ui.rs:1064`, `composer.rs:112` | create button located by CSS, not role/name | fix-now | open |
| R2-S3 | Standards | P1 | `crates/df-web/src/htmx/tests.rs:128` | assert_toast lost URI context | fix-now | open |
| R2-S4, R2-P4 | Standards+Spec | P1 | `docs/content/using/tasks.md:34` | focus sentence inside the columns list | fix-now | open |

Security: 0 findings.

User decision: Fix blocking + P1.

## Round 3 (2026-11-27)

- Implementer: 60cd334d, tasks 25/25, checks fmt/clippy/machete/tsc/openspec validate/docs-build ok, df-web 1169 passed (PostgreSQL), full `just e2e-local` green, 0 skipped (composer as own binary: 4 passed)
- Reviewed range: 3530143f...60cd334d

Round 2 rows: P1, R2-P2, R2-P3, R2-S1, R2-P1, R2-S2, R2-S3, R2-S4/P4 fixed (60cd334d).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-P1 | Spec | Blocking | `crates/df-e2e/tests/composer.rs:58-100` | blocked-storage test passes even without composer.ts try/catch (no page-error check, default assignee not asserted) | fix-now | open |
| R3-S1 | Standards | Blocking | `crates/df-e2e/tests/composer.rs:77` | `.first()` without comment | fix-now | open |
| R3-S2 | Standards | Blocking | `crates/df-e2e/tests/composer.rs:72-87,154-168` | default-project check duplicated | fix-now | open |
| R3-P2 | Spec | P3 → fix (user) | `crates/df-e2e/src/browser.rs:446` | `compose_task` stale comment, CSS locator for create button | fix-now | open |
| R3-S3 | Standards | P3 | `crates/df-web/src/htmx/tests.rs:128` | extra prefix assert for URI context | accepted | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1 + R3-P2.

## Round 4 (2026-11-27)

- Implementer: aeef62b0, tasks 25/25, checks fmt/clippy/machete ok, full `just e2e-local` green (20 binaries, 0 skipped); guard removal in composer.ts made the blocked-storage test fail, guard restored
- Reviewed range: 60cd334d...aeef62b0 (e2e only: crates/df-e2e/src/browser.rs, crates/df-e2e/tests/composer.rs)

Round 3 rows: R3-P1, R3-S1, R3-S2, R3-P2 fixed (aeef62b0).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-P1 | Spec | Blocking | `crates/df-e2e/tests/composer.rs:92` | missing guard in `remember()` caught only via timing-dependent rethrown pageerror; Alpine's `console.warn` "Alpine Expression Error" not checked | fix-now | open |

Standards: 0 findings. Security: 0 findings.

User decision: fix-round limit reached; user allowed one more fix round (4) for R4-P1 only.

## Round 5 (2026-11-27)

- Implementer: ba3455f2b259022bef7d2157f58d99ff898a4c37, composer e2e 4/4 passed; guard removed in `remember()` made the blocked-storage test fail ("[warning] Alpine Expression Error: blocked"), guard restored
- Reviewed range: aeef62b0...ba3455f2b259022bef7d2157f58d99ff898a4c37 (3-line test assertion change), checked by the orchestrator against R4-P1; no reviewer round spawned
- R4-P1: fixed (ba3455f2b259022bef7d2157f58d99ff898a4c37)

## Finish

- Container tests: ran (df-web with PostgreSQL; full `just e2e-local` in rounds 2–4). Open manual check: screen-reader announcement of an error toast (tasks.md 2.2).
- Bazel build: skipped, no Cargo.toml/Cargo.lock/BUILD changes.
- Docs: `just docs-build` ok. Pages changed: docs/content/using/tasks.md, docs/content/using/chats.md, docs/content/using/network-access.md.
- Tech debt: TD-132 (S5).
- Roadmap: backlog row "Toasts for axum extractor rejections".
- Note: during fix round 4 the implementer freed disk with `docker system prune -f` and `docker image prune -af` on the host socket.
