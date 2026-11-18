# Review log: state-predicates

- BASE: da00b7da728f26f92dea851bda682c64ef3dae13
- Store: none
- Status: done
- Implementer agent: aa50dee4c573fa2f4

## Round 1 (2026-11-17)

- Implementer: f56b7278, tasks 8/8, checks fmt/clippy/machete/tests/bazel/docs-build ok, container tests ran (docker socket)
- Reviewed range: BASE...f56b7278
- Implementer note: `held_back` includes `discarded_at IS NULL` (design says only delivered/forwarded), to keep the behavior of the 3 replaced copies.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1+P1 | Standards+Spec | Blocking | `crates/df-db/src/volumes.rs:72` | `due_for_retention` keeps literal `status IN ('completed','cancelled')`; task 2.2 check and new rule fail | fix-now | open |
| S2 | Standards | P1 | `crates/df-core/src/domain.rs:166` | `group()` uses `_ => Waiting`; new variant no longer forces a compile error | fix-now | open |
| S3 | Standards | P1 | `crates/df-core/src/domain.rs:742` | test name `the_status_constants_hold_the_statuses_of_their_meaning` is vague and checks two constants | fix-now | open |
| P2 | Spec | P1 | `openspec/changes/state-predicates/design.md`, `proposal.md` | `held_back` text lacks `discarded_at IS NULL`; code is right, artifacts outdated | fix-now | open |
| P3 | Spec | P1 | `openspec/changes/state-predicates/proposal.md` (Impact) | says no published docs change; `git-proxy.md` got a paragraph | fix-now | open |
| S4 | Standards | P3 | `crates/df-db/src/task_views.rs:594`, `runs.rs:261` | `status_list` / `state_list` helpers in two modules | none | cosmetic |
| S5 | Standards | P3 | `crates/df-db/src/index_predicates.rs` | `index_predicates_` test-name prefix repeats module path | none | cosmetic |
| P4 | Spec | Rejected | `MODULE.bazel.lock` | lock churn from clap 4.6.8 | — | rejected (catch-up of pre-BASE clap bump; required for `--lockfile_mode=error`) |
| X | Security | — | — | no findings | — | — |

User decision: Fix blocking + P1 (S1+P1, S2, S3, P2, P3)

## Round 2 (2026-11-17)

- Implementer: a9a88062, fixed S1+P1 (`TaskStatus::CLOSED`), S2, S3, P2, P3; checks fmt/clippy/machete/tests ok, container tests ran
- Reviewed range: f56b7278...a9a88062
- Round 1 rows: S1+P1, S2, S3, P2, P3 → fixed (a9a88062)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P2 | Spec | P1 | `crates/df-db/src/volumes.rs:74` | no retention test shows a `failed` task keeps its volume past the close grace period | fix-now | open |
| R2-P1 | Spec | P1 | `proposal.md`, `design.md`, `tasks.md` 2.2 | `TaskStatus::CLOSED` and `volumes.rs` missing from the artifacts | fix-now | open |
| R2-S1 | Standards | P1 | `crates/df-core/src/domain.rs:160-173` | `group()` lists Done twice (early `is_in(DONE)` return + unreachable arm) | fix-now | open |
| R2-S2 | Standards | P1 | `crates/df-core/src/domain.rs:179` | `CLOSED` declared apart from `DONE`/`WAITS_FOR_RESUME` | fix-now | open |
| X | Security | — | — | no findings | — | — |

User decision: Fix blocking + P1 (R2-P2, R2-P1, R2-S1, R2-S2)

## Round 3 (2026-11-18)

- Implementer: adf378bd, fixed R2-P2, R2-P1, R2-S1, R2-S2; checks fmt/clippy/machete/tests ok, container tests ran
- Reviewed range: a9a88062...adf378bd
- Round 2 rows: R2-P2, R2-P1, R2-S1, R2-S2 → fixed (adf378bd)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-P1 | Spec | P1 | `design.md` Decision 2, `tasks.md` 2.1 | still say `StatusGroup::Done` is derived from `DONE`; `group()` now names variants and a test pins `DONE` | fix-now | fixed (58648de1) |
| X | Security | — | — | no findings | — | — |
| S | Standards | — | — | no findings | — | — |

User decision: Fix blocking + P1 (R3-P1)

## Finish (2026-11-18)

- Fix round 3: 58648de1 (text only, verified by the orchestrator; `openspec validate` ok)
- Container tests: ran in every code round (docker socket), none skipped
- Bazel build: no `Cargo.toml`/`Cargo.lock` change; `bazel test` of the touched crates passed in round 1
- Docs: `just docs-build` ok; changed page `docs/content/security/git-proxy.md` ("Feedback messages")
- Tech debt: none (no P2 findings). Roadmap: no deferred work.
- Cosmetic: S4, S5. Rejected: P4.

