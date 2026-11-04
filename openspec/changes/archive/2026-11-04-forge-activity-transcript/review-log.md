# Review log: forge-activity-transcript

- BASE: e42d4da438ec8ddbe5d7bc82403f3e0a7a0add2d (originally 272b24b4 on `fix/release-cache`; commits moved to `feat/forge-activity-transcript` by rebase onto `main`, conflict in `crates/df-web/src/components/icons.rs` resolved as union of `ExternalLink` and the new icons)
- Store: none
- Status: done
- Implementer agent: a89f7291890064252

## Round 1 (2026-11-04)

- Implementer: c0c8a94a (code 22ec59d6), rebased as a30c3da5 (code ce8289df); tasks 18/18, checks fmt/clippy/machete/xtask/bazel build+test ok before rebase, container tests ran (podman socket); 5.2 browser check used SQL-inserted rows, no real gateway push. Post-rebase build not yet verified (ENOSPC during check; `cargo clean` freed 31 GiB).
- Reviewed range: 272b24b4...c0c8a94a (same diff as e42d4da4...a30c3da5 apart from the icons.rs merge)
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-db/src/git_pushes.rs:203` | `record_push` updates or locks the task row (`react_to_push`) before `append_control_event` locks run then task: task→run order, deadlock risk against runtime batch writer / `end_run` (design D3). Verified. | fix-now | fixed (ec0fdc45) |
| P2 | Spec | Blocking | `crates/df-db/src/git_pushes/tests.rs` | Scenario "Forge unreachable" has no `git_pushes` test asserting `UPSTREAM_FAILED` in the event | fix-now | fixed (ec0fdc45) |
| P3 | Spec | Blocking | `crates/df-agent-channel/src/pr.rs` tests | Scenario "PR marked ready" in run 2 untested; no two-run test of `pr-ready` per run (design D2) | fix-now | fixed (ec0fdc45) |
| S1 | Standards | P1 | `crates/df-db/src/transcript.rs:68`, `crates/df-agent-channel/src/session.rs:722` | Run-then-task lock order implemented twice | fix-now | fixed (ec0fdc45) |
| S3 | Standards | P1 | `crates/df-agent-channel/src/pr.rs:103-190` | PR `(i64, String)` clump, `unwrap_or_default` on URL in three places | fix-now | fixed (ec0fdc45) |
| S5 | Standards | P1 | `crates/df-web/src/transcript/mod.rs:866` | Doc says "empty" on `failure: Option<String>`; it is `None` | fix-now | fixed (ec0fdc45) |
| S6 | Standards | P1 | `crates/df-db/src/transcript.rs:580` | `i32::try_from(schema_version).unwrap_or(1)` silently rewrites invalid data | fix-now | fixed (ec0fdc45) |
| P4 | Spec | P1 | `tasks.md` 5.2 | Browser check bypassed gateway → `record_push` → `pr.rs`; record the deviation in tasks.md | fix-now | fixed (ec0fdc45) |
| S2 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:211` | Run/task rows locked twice in one transaction (harmless) | fix-now | fixed (ec0fdc45) |
| S4 | Standards | P3 | `crates/df-web/src/components/transcript.rs:789` | `forge_row(…, danger: bool, …, None, "")` positional flags | cosmetic | cosmetic |
| S7 | Standards | P3 | `crates/df-gateway/src/git/push.rs:635,724` | `vec![verdict; n]` built in two places | cosmetic | cosmetic |

User decision: fix blocking + P1 + S2 (P1, P2, P3, S1, S3, S5, S6, P4, S2); S4, S7 cosmetic. Fix round 1 also verifies the rebased build and tests.

## Round 2 (2026-11-04)

- Implementer: ec0fdc45 (fix round 1), fixes P1, P2, P3, S1, S2, S3, S5, S6, P4; checks fmt/clippy/machete/xtask/bazel build ok on rebased tree; cargo tests df-db, df-agent-channel, df-gateway, df-web incl. container-backed pass; Bazel tests not re-run.
- Reviewed range: a30c3da5...ec0fdc45
- Security: 0 findings. Round 1 fix-now items all confirmed fixed by Standards and Spec reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P5, S12 | Spec, Standards | Blocking | `crates/df-db/src/git_pushes/tests.rs:~424` | Regression test for P1 waits on `pg_stat_activity` lock waits server-wide; a lock wait in another test database ends the loop early and the test can pass on the old order. Verified. | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S8 | Standards | P1 | `crates/df-agent-channel/src/runs.rs:509` | `lock_task` doc lists `persist_batch` as caller; it now uses `df_db::transcript::lock_run_and_task` | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S9 | Standards | P1 | `crates/df-db/src/transcript.rs:49`, `crates/df-agent-channel/src/session.rs:324` | Doc claims "the one place that defines the order"; `start_finalizing` still locks run then `lock_task` by hand. Verified. | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S10 | Standards | P1 | `crates/df-db/src/transcript.rs:39-47` | `RunTaskLock::run_id`/`task_id` accessors unused | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S11 | Standards | P1 | `crates/df-agent-channel/src/pr.rs:123` | Negative stored `pr_number` silently becomes "no PR" (`.ok()?`); `record_pr` treats it as `InvalidData` | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S13 | Standards | P1 | `crates/df-db/src/git_pushes/tests.rs:380` | Test named "unreachable forge" uses reason `df:forge-refused` | fix-now | fixed (ba27238cf12675a2ce242074811f7306b7865da1) |
| S14 | Standards | P3 | `crates/df-db/src/transcript.rs:17,22` | `SCHEMA_VERSION: i32` with `.unsigned_abs()` | cosmetic | cosmetic |

User decision: fix blocking + P1 (P5/S12, S8, S9, S10, S11, S13); S14 cosmetic.

## Round 3 (2026-11-04)

- Implementer: ba27238cf12675a2ce242074811f7306b7865da1 (fix round 2), fixes P5/S12, S8, S9, S10, S11, S13; checks fmt/clippy/machete/xtask ok; cargo tests df-db, df-agent-channel, df-web incl. container-backed pass; `bazel test //crates/... //plugins/...` 41/41 pass; regression test verified red against the old lock order.
- Reviewed range: ec0fdc45...ba27238cf12675a2ce242074811f7306b7865da1
- Security: 0 findings. Spec: 0 findings. All round 2 fix-now items confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S15 | Standards | P1 | `crates/df-db/src/transcript.rs:39-41`, `crates/df-agent-channel/src/runs.rs:510-512` | Caller list of the lock order kept in two docs; `df-db` names a downstream function. Keep the rule in `lock_run_and_task`, have `lock_task` point to it without caller names | fix-now | fixed (5b9e3752) |
| S16 | Standards | P1 | `crates/df-agent-channel/src/pr.rs:124` | `of_task` doc omits the new `InvalidData` case | fix-now | fixed (5b9e3752) |
| S17 | Standards | P2 | `crates/df-db/src/git_pushes/tests.rs:428`, `crates/df-db/src/transcript/tests.rs:199` (+ 6 pre-existing copies) | `pg_stat_activity` lock-wait polling loop copied across test files; pre-existing copy in `crates/df-agent-channel/src/runs/tests.rs:645` still counts server-wide (can end early). Matters when more lock-order tests are added or that test flakes | tech-debt | tech-debt (TD-101) |

User decision: fix P1 (S15, S16), then finish; S17 to tech debt.

## Round 4 (2026-11-04)

- Implementer: 5b9e3752 (fix round 3), doc-only (S15, S16); fmt and clippy ok. Verified by the orchestrator: only `///` lines changed; no reviewer round.

## Result

- Status: done. No blocking finding open; container-backed tests ran in every round (podman socket); `bazel test //crates/... //plugins/...` 41/41 in round 3; no Cargo.toml/Cargo.lock change.
- Fixed: P1, P2, P3, P4, P5/S12, S1, S2, S3, S5, S6, S8, S9, S10, S11, S13, S15, S16.
- Tech debt: TD-101 (S17).
- Cosmetic: S4, S7, S14. Rejected: none.
- Deferred work already on the roadmap backlog: "Forge activity outside runs", "Commit links in forge activity".
- Open caveat: task 5.2 browser check used SQL-inserted rows; the gateway → `record_push` → `pr.rs` path is covered by `df-db` and `df-agent-channel` tests, not end to end.
