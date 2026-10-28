# Review log: split-oversized-test-modules

- BASE: d4d634e37fb992d47e209544a257cb375299719f
- Store: none
- Status: done
- Implementer agent: a007db1cd3d4984fb

## Round 1 (2026-10-28)

- Implementer: 776cbca (groups 1–7: 67cd62b, cc5f1e3, dda6376, 4c2efbe, f0f88f7, 6e7c7ca, 776cbca), tasks 33/36, checks: build/fmt/clippy/machete ok, non-container cargo + Bazel tests ok, lint negative checks ok
- Container tests: skipped (Podman socket died mid-run): df-agent-channel, df-db, df-web, dark-factory lib, df-gateway `control::tests`, their Bazel targets. Tasks 5.1, 5.3, 8.1 open pending these.
- Deviations reported: D1 split `workspace`+`workspace_finalize`, `runner`+`runner_runtime`; D5 test paths gain topic segment; 7.2 grep pattern does not match rustfmt output; 2 new production outliers (`run_handler`, `decide`).
- Reviewed range: d4d634e...776cbca

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1+P1+P2 | Standards+Spec | P1 | 17 production `#[expect(clippy::too_many_lines)]` | Reasons all generic "follow-up of split-oversized-test-modules", don't name the follow-up (AGENTS.md/D7); `run_handler`, `decide` missing from D7 table and Follow-ups | fix-now | fixed (fd64263, 4bacfd6) |
| S2 | Standards | P1 | `crates/dark-factory/src/main.rs:340` | `#[expect]` on whole `impl SandboxBackend` block exempts every method | fix-now | fixed (fd64263) |
| S4+P3 | Standards+Spec | P1 | `crates/factory-agent/src/tests/runner.rs:437-575` | cancelled-by-control-plane scenario keeps 130-line copied setup that `RunScenario::params()` reproduces (task 4.2) | fix-now | fixed (1519fe5) |
| S3 | Standards | P1 | 6 new `tests.rs` in df-agent-channel, df-db, df-k8s | missing `//!` module doc | fix-now | fixed (b66387c) |
| S5 | Standards | P1 | `docs/tech_debt.md` TD-67 | resolved by `RunScenario` but still open | fix-now | fixed (4d69b76) |
| S6 | Standards | P1 | `crates/factory-agent/docs/claude-acp.md:61`, `docs/tech_debt.md` TD-7/68/70 | stale paths to deleted test files | fix-now | fixed (4d69b76) |
| P4 | Spec | P1 | `tests/transcript.rs` `test_tool_origin_detection`, `tests/git.rs` `test_config_key_normalization` | names state a topic, not a behavior (D2) | fix-now | fixed (f6f62b9) |
| P5 | Spec | P1 | `tests/git.rs:544,551`, run ids `run-r2`/`run-r51`/`run-r3`/`run-r41` | review-round references left (D2) | fix-now | fixed (f6f62b9) |
| P6 | Spec | P1 | `design.md` D1 table, `tasks.md` 2.2/7.2 | artifacts don't reflect extra topic files and the 7.2 grep can't match | fix-now | fixed (4bacfd6) |

Gate note: no blocking code finding. Tasks 5.1, 5.3, 8.1 open: container-backed tests not run (socket dead).

User decision: Fix blocking + P1 (all 9 P1). Podman socket back up; container tests to be run in fix round 1.

## Round 2 (2026-10-28)

- Implementer: fixups 1519fe5, fd64263, b66387c, 4d69b76, f6f62b9, 4bacfd6 (HEAD 4bacfd6), tasks 33/36, checks: fmt/clippy/machete ok, Bazel lint+fmt ok, non-container cargo + Bazel tests ok
- Container tests: skipped again (Podman socket died during `cargo test --workspace`): df-agent-channel, df-db, df-web, dark-factory lib, df-gateway `control::tests`, `bazel test //...`. Tasks 5.1, 5.3, 8.1 open.
- Renames added: `test_tool_origin_detection` → `test_only_the_mcp_factory_prefix_marks_factory_origin`; `test_config_key_normalization` → `test_denied_config_keys_are_caught_despite_whitespace_and_command_keys`
- Reviewed range: 776cbca...4bacfd6

Round-1 ledger re-check: all 9 items confirmed fixed by both reviewers, no regressions. Test count unchanged (1030).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P1 | `crates/factory-agent/src/tests/runner.rs` cancel test | `let _ = inbound_tx.send(Cancel)` silently drops a send the test depends on (carried over, line touched) | fix-now | fixed (1a37f69) |
| S7 | Standards | P1 | `tests/runner_runtime.rs:915,922` | run id `run-resume` in a cancel-before-assign test that never resumes | fix-now | fixed (a289fb4) |
| S8+S9 | Standards | P1 | `tests/runner_runtime.rs:927-928,1004,1035,1040-1041` | `run-second` meaningless; branches `DF-51`/`DF-2` keep round numbers | fix-now | fixed (a289fb4) |
| S10 | Standards | P1 | `tests/runner.rs` cancel test | comment claims `params` borrows the scenario (it doesn't), placed above wrong call | fix-now | fixed (1a37f69) |
| P7 | Spec | P1 | `tests/git.rs:541` | new name implies command keys are a hiding trick; they are a second denied class | fix-now | fixed (a289fb4) |

User decision: Fix blocking + P1 (all 5). Podman socket back; container tests to run per crate with limited parallelism.

## Round 3 (2026-10-28)

- Implementer: fixups 1a37f69, a289fb4, 2b41d13 (HEAD 2b41d13), tasks all done, checks: fmt/clippy ok, factory-agent tests ok
- Container tests: ran per crate (-j2, --test-threads=2): df-db 67, df-agent-channel 147, df-web 375, dark-factory 14+3, df-gateway 87+2, all passed. Skipped: `bazel test //...` for container targets.
- Final rename-table additions: `test_config_key_normalization` → `test_config_denylist_sees_through_whitespace_and_blocks_command_keys`; `test_tool_origin_detection` → `test_only_the_mcp_factory_prefix_marks_factory_origin`
- Reviewed range: 4bacfd6...2b41d13

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8 | Spec | P1 | `tasks.md` 8.1 | 8.1 ticked, but `bazel test //...` for container targets skipped and no total-vs-baseline comparison recorded | resolved by ledger note | fixed (review-log 8.1 note) |
| S12 | Standards | P3 | `crates/factory-agent/src/tests/runner.rs:466` | `.expect` inside an aborted, never-awaited task can't fail the test; the `Cancelled` outcome assertion already catches a lost Cancel | leave | cosmetic |

Task 8.1 baseline note (for P8): test count at BASE and HEAD is 1030 (`#[test]`/`#[tokio::test]`, spec review round 2). Executed counts match the baseline per crate: df-db 67, df-agent-channel 147, df-web 375, factory-agent 116 (114 + 1 other binary + 1 ignored/manual), df-k8s 65 (62 run). `bazel test //...` for container targets (df-agent-channel, df-db, df-web, dark-factory, df-gateway) deferred to CI: the local Podman VM died twice under full parallel load.

User decision: Finish (P8 resolved by ledger note, S12 cosmetic).

## Finish (2026-10-28)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| F1 | Build | Blocking | `MODULE.bazel.lock` | `bazel build --lockfile_mode=error //crates/... //plugins/...` fails: lock stale after `Cargo.toml` lint changes (implementer reverted lock updates) | fix-now | fixed (15b07d8) |

- F1 fix 15b07d8: lockfile diff is the 3 `FILE:@@//…Cargo.toml` hashes only, no version bumps (verified by orchestrator); `--lockfile_mode=error` build passes. No further review round for a hash-only change.
- Tech debt: none created (no P2 findings). TD-67 resolved and removed by this change.
- Container tests not run locally: `bazel test //...` for df-agent-channel, df-db, df-web, dark-factory, df-gateway targets (CI).
