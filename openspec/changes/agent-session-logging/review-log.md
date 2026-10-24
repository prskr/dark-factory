# Review log: agent-session-logging

- BASE: 505b3445d6ca01c08be51775b562b561a511f5b8
- Store: none
- Status: done
- Implementer agent: aa133055e3c165605

## Round 1 (2026-10-24)

- Implementer: e62821c, tasks 18/18, checks fmt/clippy/machete ok, tests pass (factory-agent, dark-factory, df-core, df-k8s, helm-unittest), container tests ran (podman)
- Reviewed range: BASE...e62821c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1+P2 | Spec | Blocking | `crates/factory-agent/src/cli.rs:14`, `crates/dark-factory/src/main.rs:56` | clap error doesn't name `DF_LOG_FORMAT` (verified clap_builder format.rs:224); env path untested | fix-now | fixed (9e325c4) |
| P3 | Spec | Blocking | `crates/factory-agent/src/main.rs:131` | "run assigned" event and its fields not tested | fix-now | fixed (aaf42fe) |
| P4 | Spec | Blocking | `crates/factory-agent/src/runner.rs:559`, `main.rs:106` | "prompt received" not asserted in turn scenario | fix-now | fixed (9e325c4) |
| P5 | Spec | Blocking | `crates/*/src/logging.rs` | invalid RUST_LOG: info fallback and WARN not tested | fix-now | fixed (aaf42fe) |
| P6 | Spec | Blocking | `crates/factory-agent/src/logging.rs:36` | default level (no RUST_LOG) INFO-not-DEBUG not tested | fix-now | fixed (aaf42fe) |
| S2 | Standards | P1 | `crates/factory-agent/src/runner.rs:89,92` | chunk text logged uncapped at DEBUG | fix-now | fixed (aaf42fe) |
| S3 | Standards | P1 | `crates/factory-agent/src/runner.rs:134` | `chunk_text` nested in match arm | fix-now | fixed (aaf42fe) |
| S1 | Standards | P2 | `crates/factory-agent/src/runner.rs:95` | tool input at INFO may carry tokens; spec-mandated, accepted risk in design.md:69; needs spec change | tech-debt | tech-debt (TD-52) |
| S4 | Standards | P2 | `crates/{dark-factory,factory-agent}/src/logging.rs` | duplicated LogFormat/filter/subscriber | tech-debt | tech-debt (TD-53) |
| S5 | Standards | P2 | `crates/df-k8s/src/config.rs:46` | `log_format: String` instead of enum | tech-debt | tech-debt (TD-54) |
| S6 | Standards | P3 | `main.rs:106`, `runner.rs:559` | "prompt received" logged at two sites | cosmetic | cosmetic |
| S7 | Standards | P3 | `crates/*/src/logging.rs` | hard-coded "info" in fallback message | cosmetic | cosmetic |
| S8 | Standards | P3 | `crates/factory-agent/src/main.rs:131` | message chain for task_key | cosmetic | cosmetic |
| S9 | Standards | P3 | `crates/factory-agent/src/tests.rs:5200` | `found.unwrap()` after assert | cosmetic | cosmetic |
| S10 | Standards | P3 | e62821c | no separate TDD red phase (process) | cosmetic | cosmetic |

User decision: Fix blocking + P1 (fix P1+P2, P3, P4, P5, P6, S2, S3; S1/S4/S5 to tech debt at finish)

## Round 2 (2026-10-24)

- Implementer: aaf42fe, tasks 18/18, checks fmt/clippy/machete ok, tests pass (factory-agent 103, dark-factory 15), container tests ran (podman)
- Reviewed range: e62821c...aaf42fe
- Note: P1+P2 test uses `--log-format` flag, not env var (`set_var` is unsafe, forbidden); P1+P2 fix had no separate red phase

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1+P2 (r2) | Spec | Blocking | `crates/*` CLI tests | env path `DF_LOG_FORMAT=logfmt` still untested; subprocess test via `CARGO_BIN_EXE_*` is safe | fix-now | fixed (9e325c4) |
| P4+P8 | Spec | Blocking | `crates/factory-agent/src/main.rs:115`, `tests.rs:5147` | order "prompt received → turn started" (and "run assigned" first) untested; initial-prompt log site not exercised e2e | fix-now | fixed (9e325c4) |
| P7+S12 | Spec+Standards | P1 | `crates/factory-agent/src/runner.rs:151` | chunk text cut silently, no `text_truncated` flag; spec only bounds `input` | fix-now | fixed (9e325c4) |
| S11+S13 | Standards | P1 | `crates/factory-agent/src/runner.rs:71`, tests | `LOG_INPUT_MAX_BYTES` now also for chunks; test hard-codes 512 | fix-now | fixed (9e325c4) |
| S14 | Standards | P1 | `crates/factory-agent/src/main.rs:36` | private `log_assigned` above `main` | fix-now | fixed (9e325c4) |
| S4 (r2) | Standards | P2 | both `logging.rs` | duplication grew: `build()` and invalid-level test copied; dark-factory test buffer setup duplicated | tech-debt | tech-debt (TD-53) |

User decision: Fix blocking + P1 (fix P1+P2 r2, P4+P8, P7+S12, S11+S13, S14; S4 stays tech debt)

## Round 3 (2026-10-24)

- Implementer: 9e325c4, tasks 18/18, checks fmt/clippy/machete ok, tests pass (factory-agent 103+1, dark-factory 15+1, Bazel cli_test 2/2), container tests ran (podman)
- Reviewed range: aaf42fe...9e325c4
- Note: "run assigned" first-in-order not asserted e2e (main not reachable); covered by field test + comment

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P9+S16 | Spec+Standards | Blocking | `crates/factory-agent/src/tests.rs:5277` | `Option<usize>` order compare: `None < Some` passes when "prompt received" missing (verified) | fix-now | fixed (87daa2da513fdad7e95d14f3da22da5ef22e444f) |
| S18 | Standards | P1 | `crates/dark-factory/tests/cli.rs:15` | subprocess `serve` could hang if parse regressed and default config exists; pass `-c` to a nonexistent path | fix-now | fixed (87daa2da513fdad7e95d14f3da22da5ef22e444f) |
| S15 | Standards | P1 | `crates/factory-agent/src/runner.rs:94,97` | `chunk_text` called twice with `.0`/`.1`; bind `let (text, cut)` | fix-now | fixed (87daa2da513fdad7e95d14f3da22da5ef22e444f) |
| S17 | Standards | P1 | `crates/factory-agent/src/tests.rs:5278` | two order asserts duplicate the ordered loop above | fix-now | fixed (87daa2da513fdad7e95d14f3da22da5ef22e444f) |
| P10 | Spec | P3 | `specs/logging/spec.md:93,104` | spec lacks chunk limit + `text_truncated`; follow-up `/opsx:update` before archive | follow-up | fixed (spec update) |

User decision: Fix blocking + P1 (fix P9+S16, S18, S15, S17; P10 via /opsx:update before archive)

## Round 4 (2026-10-24)

- Implementer: 87daa2da513fdad7e95d14f3da22da5ef22e444f, tasks 18/18, checks fmt/clippy/machete ok, tests pass (factory-agent 103+1, dark-factory 15+1), container tests ran (podman); P9 red check done
- Reviewed range: 9e325c4...87daa2da513fdad7e95d14f3da22da5ef22e444f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| — | — | — | — | no new findings; S15–S18, P9 verified fixed | — | — |

## Result

- Status: done (no blocking finding open)
- Tech debt: TD-52 (S1), TD-53 (S4), TD-54 (S5)
- P10: spec updated via /opsx:update (chunk-text limit and `text_truncated`)
