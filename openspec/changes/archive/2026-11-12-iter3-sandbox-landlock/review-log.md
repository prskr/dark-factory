# Review log: iter3-sandbox-landlock

- BASE: c0726fdd13b334347c30fe6a7178767555747f4e
- Store: none
- Status: untested (kernel-backed tests and e2e not run: no Landlock in the sandbox kernel; P6 open)
- Implementer agent: a5f308fe158cbf729

## Round 1 (2026-11-12)

- Implementer: 4f2a2a9, tasks 12/15 (open: 5.1 e2e not executed — kind node out of disk; 5.2 manual homelab; 5.3 full test run not green), checks: fmt/clippy/machete/xtask/openspec/docs-build ok; cargo deny fails on pre-existing RUSTSEC-2026-0486; container tests ran (docker.sock) except `just e2e-local`; kernel-backed: 10/12 `factory-agent/tests/sandbox.rs` not runnable (Landlock ENOSYS on 6.12.115 here)
- Reviewed range: c0726fd...4f2a2a9

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/factory-agent/src/runner.rs:764` | With `disabled`, harness inherits all `DF_*` (spec requirement is unconditional); config doc claims removal | fix-now | fixed (cd3c148) |
| P2 | Spec | Blocking | `crates/factory-agent/tests/sandbox.rs` | Scenario "Runtime memory" (`/proc/<ppid>/environ`, `mem`) untested | fix-now | fixed (cd3c148) |
| P3 | Spec | Blocking | `crates/factory-agent/tests/sandbox.rs` | Scenario "Toolchain under /opt" untested end-to-end (fake-root exec) | fix-now | fixed (cd3c148) |
| P5 | Spec | Blocking | `crates/factory-agent/src/tests/confinement.rs` | Wrapper exit 125 → run `FAILED` has no runner test | fix-now | fixed (cd3c148) |
| P6 | Spec | Blocking | `crates/df-e2e/tests/landlock.rs` | "Factory tools still work" / key unreadable in pod only in e2e, never executed (needs ABI 6 kernel) | run e2e on ABI-6 kernel | open |
| X1 | Security→Spec | P1 | `crates/factory-agent/src/sandbox/aliases.rs:26` | Symlinked `<data>/runtime` (or symlinks inside) from an unrestricted run redirect outbox into writable `/data/home`; D2 handles hard links/writable-root symlinks but not this. Security verifier: confidence 4 (pre-existing sink, narrow precondition) | fix-now | fixed (cd3c148) |
| S8 | Standards | P1 | `crates/factory-agent/tests/sandbox.rs:186` | Fixed `/tmp/df-landlock-test-$$` path | fix-now | fixed (cd3c148) |
| P7 | Spec | P1 | `docs/content/security/sandbox.md` | No runnable kernel check command (proposal requires one) | fix-now | fixed (85cfc81, via P9) |
| P8 | Spec | P1 | `docs/content/security/sandbox.md` | "trace the runtime" in opt-out depends on Yama/caps, not Landlock | fix-now | fixed (cd3c148) |
| S1 | Standards | P1 | `crates/factory-agent/src/config.rs:78` | `LandlockMode` spellings duplicated; add `FromStr` in df-core | fix-now | fixed (cd3c148) |
| S2 | Standards | P1 | `crates/factory-agent/src/config.rs:22` | `RuntimeConfig` dirs duplicate `Layout`; hold `layout: Layout` | fix-now | fixed (cd3c148) |
| S3 | Standards | P1 | `crates/factory-agent/src/runner.rs:768`, `cli.rs:18` | `DF_*` literals instead of new constants | fix-now | fixed (cd3c148) |
| S4 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:333` | Writable paths duplicated across crates | fix-now | fixed (cd3c148) |
| S7 | Standards | P1 | `crates/factory-agent/src/sandbox.rs` | `Result<(), String>` instead of anyhow + context | fix-now | fixed (cd3c148) |
| S9 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:523` | Inline test module ~730 lines; move to `assign/tests.rs` | fix-now | fixed (cd3c148) |
| P4 | Spec | P3 | `crates/factory-agent/src/sandbox.rs:548` | Non-`FullyEnforced` → 125 not testable on ABI≥6 | note only | cosmetic |
| S5 | Standards | P3 | `crates/factory-agent/src/runner.rs:1051` | `mcp_command` now also the sandbox exe; rename `runtime_exe` | — | cosmetic |
| S6 | Standards | P3 | `crates/factory-agent/src/runner.rs:960` | `Confinement::disabled()` carries unused probe | — | cosmetic |
| S10 | Standards | P3 | `crates/df-core/src/config/tests.rs:369` | Test also asserts `as_str` | — | cosmetic |

User decision: Fix blocking + P1, plus S5 and S6 (moved from P3 to fix-now). P6 stays open until the e2e runs on an ABI-6 kernel.

## Round 2 (2026-11-12)

- Implementer: cd3c148, tasks 12/15, checks: fmt/clippy/machete/xtask/openspec/docs-build ok; cargo + Bazel tests pass except `//crates/factory-agent:sandbox_test` (all 12 kernel tests fail here: no Landlock); container tests ran (df-agent-channel); `just e2e-local` not retried (disk)
- Fixed per implementer: P1, P2, P3, P5, X1, S8, P7, P8, S1–S7, S9
- Reviewed range: 4f2a2a9...cd3c148
- Round-1 rows verified: P1, P2, P3, P5, X1, P8, S1–S9 fixed (cd3c148); P7 not fixed (see P9). Security: 0 new findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P9 | Spec | Blocking | `docs/content/security/sandbox.md:134` | Kata check pod is rejected by the `restricted` PodSecurity of the sandbox namespace; hard-codes `kata` instead of `sandboxNamespace.runtimeClassName` (verified) | fix-now | fixed (85cfc81) |
| P10 | Spec | Blocking | `crates/factory-agent/src/tests/confinement.rs:829` | Disabled-mode env test passes without the fix under plain cargo (no `DF_*` in test env) | fix-now | fixed (85cfc81) |
| P11 | Spec | P1 | `openspec/.../specs/sandbox-filesystem-isolation/spec.md`, `design.md` D2 | X1 behavior (refuse symlinked runtime dir, remove symlinks inside) not in spec/design; docs and spec disagree | `/opsx:update` (artifact change) | open |
| S11 | Standards | P1 | `crates/df-core/src/config.rs:452` | `FromStr for LandlockMode` has `Err = String`; library code needs a `thiserror` type | fix-now | fixed (85cfc81) |
| S12 | Standards | P1 | `crates/factory-agent/src/main.rs:181` | Tuple/match chain hard to read; name collides with `aliases::secure_outbox_dir` | fix-now | fixed (85cfc81) |
| S13 | Standards | P1 | `crates/factory-agent/src/sandbox/aliases.rs:612` | Real-dir check duplicated with `verify_writable_roots` | fix-now | fixed (85cfc81) |
| S14 | Standards | P1 | `crates/factory-agent/src/sandbox/aliases.rs:630` | `read_dir`/`remove_file` errors without path context | fix-now | fixed (85cfc81) |
| S15 | Standards | P1 | `crates/factory-agent/src/tests/runner_runtime.rs` | `mcp_bin` locals not renamed | fix-now | fixed (85cfc81) |
| S16 | Standards | P1 | `crates/factory-agent/src/cli.rs:39` | Runtime socket default still a literal | fix-now | fixed (85cfc81) |
| P12 | Spec | P3 | `crates/factory-agent/src/main.rs:284` | No test of startup ordering for the outbox-dir check | — | cosmetic |
| S17 | Standards | P3 | `crates/factory-agent/src/tests/connection.rs` | Repeated `Layout { … }` in tests | — | cosmetic |

Note: standards reviewer says the two 125 tests (symlinked home, missing CA dir) don't call `require_landlock` and should still pass without Landlock, contrary to the implementer's note; to be checked in the next fix round.

User decision: Fix blocking + P1 (P9, P10, S11–S16 to the implementer; P11 via `/opsx:update` by the user; P6 stays open).

## Round 3 (2026-11-12)

- Implementer: 85cfc81, tasks 12/15, checks: fmt/clippy/machete/xtask/openspec/docs-build ok; cargo tests pass; Bazel 6/7 (`sandbox_test`: 12/14 fail, no Landlock); e2e not retried
- Fixed per implementer: P9, P10, S11–S16. The two 125 tests pass without Landlock (round-1 claim withdrawn).
- Reviewed range: cd3c148...85cfc81
- Round-2 rows verified: P9, P10, S11–S16 fixed (85cfc81). Security: 0 new findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S18 | Standards | P1 | `crates/factory-agent/src/runner.rs:167` | `runtime_env` on `Confinement` (unrelated to Landlock); enforce-mode test fixture silently uses an empty env | fix-now | fixed (3af8f95) |
| P13 | Spec | P1 | `crates/factory-agent/src/tests/confinement.rs:202` | Disabled-mode env test omits `DF_CONTROL_PLANE_SERVER_NAME` named by the scenario | fix-now | fixed (3af8f95) |
| S19 | Standards | P3 | `crates/factory-agent/src/runner.rs:16` | `use std::ffi::OsString` outside the `std` group | — | cosmetic |
| S20 | Standards | P3 | `crates/factory-agent/src/main.rs:184` | Inconsistent error logging; `break_hard_links` errors lack path | — | cosmetic |
| S21 | Standards | P3 | `crates/factory-agent/src/runner.rs:797` | Identity-looking `.map` | — | cosmetic |

User decision: Fix P1 (S18, P13) in a last fix round, then finish. P6 stays open.

## Fix round 3 (2026-11-12)

- Implementer: 3af8f95 (S18, P13). Orchestrator checked the diff; no further review round (user decision). clippy, fmt, factory-agent cargo tests ok.

## Finish (2026-11-12)

- Bazel build `--lockfile_mode=error //crates/... //plugins/...`: ok (remote cache disabled, BuildBuddy unreachable from sandbox).
- `just docs-build`: ok. Docs added/changed: `docs/content/security/sandbox.md` (new), `docs/content/security/_index.md`, `docs/content/operations/troubleshooting.md`, `docs/content/operations/upgrades.md`, `crates/factory-agent/docs/configuration.md`, df-agent-channel reference, Helm README, `docs/demo.md`.
- Tech debt: none (no P2 findings). Roadmap: no new deferred items.
- Open:
  - P6 (blocking): e2e `crates/df-e2e/tests/landlock.rs` never executed (kernel without Landlock; kind node out of disk).
  - Kernel-backed tests never executed: 12 of 14 in `crates/factory-agent/tests/sandbox.rs` (ruleset, CA rename, runtime/ not writable, signal scope, `/proc/<ppid>` memory, exec outside `/usr`, env stripping in the wrapper, `/proc/self/fd`, top-level symlink to `/`, …). Landlock returns ENOSYS on this sandbox kernel (6.12.115).
  - P11: spec.md / design D2 to be amended via `/opsx:update` (symlinked runtime dir).
  - Tasks 5.1 (e2e), 5.2 (manual homelab check, user), 5.3 (full green test run) unticked.
