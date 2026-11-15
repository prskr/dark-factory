# Review log: hermetic-cc-toolchain

- BASE: b7a5d9f3
- Store: none
- Status: done
- Implementer agent: a0178cad173df7ac4

## Preflight (2026-11-14)

- Host: aarch64 NixOS (glibc 2.42, GNU ld 2.46), Docker at `unix:///var/run/docker.sock` (aarch64).
- BuildBuddy key in `~/.bazelrc` (`--remote_header`); BES reachable. Pass `--bazelrc=$HOME/.bazelrc` after `.bazelrc.agents` to use it.
- User approved: push this branch, temporary push-triggered spike workflow (ubuntu-latest, ubuntu-24.04-arm, macos-latest) using `BUILDBUDDY_ORG_API_KEY`, removed before finishing; no release push, no PR. `bazel clean --expunge` allowed.

## Paused (2026-11-14 ~12:20, user needed the machine)

- Resume point: step 2 (Implement), spike group 1 in progress. Implementer stopped; resume it with SendMessage or spawn a fresh one pointing at `/workspace/spike-notes/RESUME.md`.
- Commits: 90ef90b1 (toolchains_llvm prototype), 5d2e9f19 (hermetic_cc_toolchain/zig prototype, current).
- Spike data: `/workspace/spike-notes/` (outside the repo; `RESUME.md` has the timing table and open points).
- Finding: zig is about as fast as the baseline (562 s vs 547 s) once its cache is persistent and mounted into the sandbox; without that it is 2x slower.
- User preferences: musl for all tested/shipped Linux artifacts (already true); no gold warnings; same linker everywhere not required.
- Resumed twice (after the pause, after an OOM restart; sandbox now 32 GB RAM).

## Round 1 (2026-11-14)

- Implementer: baeaad83 (HEAD_1), tasks 25/26 (5.2 open: `cargo deny` fails on RUSTSEC-2026-0486 rustix, Cargo.lock untouched), checks fmt/clippy/machete/cargo build/bazel build lockfile_mode=error/openspec/docs-build ok; `bazel test //...` 52/53 (`//crates/factory-agent:sandbox_test` fails: no Landlock ABI 6 on this VM), container tests ran (11 `no-remote-exec` targets pass).
- Decision (ADR 0058): hermetic_cc_toolchain (zig, glibc 2.28) + patch + `/var/tmp/zig-cache` sandbox mount; toolchains_musl kept; `--config=rbe` + `rbe-cross`, `just bazel-test-rbe`; CI and release on rbe; cargo aarch64 links with rust-lld (`.cargo/cc-lld.sh`).
- Reviewed range: b7a5d9f3...baeaad83

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `Justfile:140`, ADR-0058 Decision 4 | Local half of `bazel-test-rbe` (arm64/macOS) builds all of `//...` (no `--build_tests_only`); D4 speed gate was measured on the `rbe-cross` half only | | open |
| P2 | Spec | Blocking | `crates/df-test-postgres/src/lib.rs:675` | Sweep test asserts drop right after `pool.close()`; server may still list the backend, so CI on rbe failed 3/6 cold runs; ADR declares the gate passed anyway | | open |
| P4 | Spec | P1 | spec "Linux host without a C compiler", `docs/development.md`, ADR | `//deploy:images_structure_test` also needs host `file`; docs/ADR name only `readelf` and the harness tests | | open |
| P7 | Spec | P1 | `docs/remote-execution.md` §3.3–3.4, `.bazelrc:39` | Config block lacks cpu/features pins; macOS run was `rbe-cross` not `rbe`; `.bazelrc` comment names the filter instead of `rbe-cross` | | open |
| P3 | Spec | P1 | spec "Container-backed tests on the client" | Harness structure tests and `buf_lint_test` also run on the client; scenario names only df-test-postgres | | open |
| S7 | Standards | P1 | `build/check_no_remote_exec.sh` | `2>/dev/null` hides query errors in CI | | open |
| S5 | Standards | P1 | `.bazelrc:26` vs rust SKILL.md:25 | Override location differs (`.bazelrc.user` vs `.bazelrc.agents`) | | open |
| P8 | Spec | P1 | `build/check_no_remote_exec.sh` | Negative case (target without tag fails) not recorded | | open |
| P5 | Spec | P1 | ADR Results | Baseline fetch size / amd64 baseline not recorded under "Not measured" | | open |
| S1 | Standards | P1 | ADR-0058 | Extra `Results`/`Not measured` sections; README requires template sections | | open |
| S2+S4 | Standards | P1 | ADR-0058, `docs/development.md:521` | Long run-by-run prose; 280-word bullet → short reference + how-to | | open |
| S3 | Standards | P1 | ADR-0058 | Task/D-numbers only resolve in the archived change | | open |
| S6+P6 | Standards+Spec | P3 | `.cargo/cc-lld.sh:7` | `rustc` from PATH may be Bazel's; verified: Bazel's 1.99.1 sysroot ships the same `rust-lld`, no fallback; comment only | | open |
| S8 | Standards | P3 | workflows, docs, ADR | Rollback sentence repeated 5× | | open |
| X* | Security | Rejected | — | No findings ≥ 8 (pinning, secrets, /var/tmp cache, scripts checked) | | rejected (no findings) |

User decision (2026-11-15): Fix blocking + P1 (S6/P6 one-line comment, S8 cosmetic). Also bump `rustix` (RUSTSEC-2026-0486) in this change so task 5.2 can close.

## Round 2 (2026-11-15)

- Implementer: 07e82307 (HEAD_2; plus d8b40c15 rustix bump, e73d7541 sweep test fix), tasks 26/26, checks fmt/clippy/machete/deny/cargo build/bazel lockfile build/openspec/docs-build ok; `bazel test -k //...` 52/53 (only `sandbox_test`, no Landlock ABI 6); 11 `no-remote-exec` tests pass. No CI-equivalent run on the branch after the fixes.
- P1 measurements: one-crate warm steady state recipe 166 s vs `bazel-test` 219 s (242 s right after the other mode); fresh output base + warm remote cache 163 s vs 638 s. Recipe kept.
- P2: 30/30 runs pass unloaded and under load; without the fix 1/30 failed under load.
- Reviewed range: baeaad83...07e82307 (round-1 rows verified as part of it)
- Round-1 rows: S1, S2+S4, S3, S5, S6+P6, S7, P3, P4, P5, P7, P8 fixed (07e82307); P1, P2 partially fixed (see P9, P2b); S8 cosmetic.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P9 | Spec | Blocking | ADR-0058 decision 4 | Recipe gate measured with a comment-only change (dependents cache hits) and a remote cache warmed by the recipe's own arm64 uploads; realistic case (real code change; fresh checkout with CI-warmed amd64 cache only) unmeasured, ADR says "faster in both cases" without the limits | | open |
| P2b | Spec | Blocking | ADR-0058 decision 6 | Says CI "passed every step"; true only for the passing runs; the sweep fix was verified locally (30/30), not in a CI-equivalent run | | open |
| P10 | Spec | P1 | `docs/development.md:228` | Still says CI passes `--config=cache` | | open |
| S9 | Standards | P1 | ADR-0058:190 | Remote-execution paragraph packs ~8 measurements and 3 topics | | open |
| S10 | Standards | P1 | ADR-0058:167, `docs/remote-execution.md:373` | Reference to `spike-results.md` breaks once the change is archived | | open |
| X* | Security | Rejected | — | No findings | | rejected (no findings) |

User decision (2026-11-15): Fix blocking + P1 (P9 by measuring realistic cases, drop the recipe if it loses; P2b via 2 cold CI-equivalent rbe runs with a temporary spike workflow; P10, S9, S10).

## Round 3 (2026-11-15)

- Implementer: be4a211e (HEAD_3; plus 9376e72d temporary workflow, b2483174, c8165031 zig exec toolchain registered first, a7a46ef1), tasks 26/26, checks fmt/clippy/cargo build/openspec/docs-build ok, `MODULE.bazel.lock` unchanged; `bazel test -k //...` 52/53 (only `sandbox_test`, no Landlock ABI 6); 11 `no-remote-exec` tests pass. Spike workflow removed.
- P9 measurements: real code change (relinks dependents) 157–166 s vs 219–231 s; fresh output base + CI-only amd64 remote cache 291 s vs 638 s (593 s before c8165031: 1,650/5,513 remote cache hits, after 5,513/5,513). Recipe kept.
- P2b: 4/4 cold CI-style rbe runs pass (43696058100, 43697817539, 43699377285, 43701151809; verified with `gh run view`).
- Reviewed range: 07e82307...be4a211e
- Round-2 rows: P9, P2b, P10, S9, S10 fixed (be4a211e).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P11 | Spec | P1 | `docs/remote-execution.md:65` | §3.2 says `zig_cc_exec_linux_amd64` serves only non-amd64 clients; since c8165031 it is registered first for all clients (shared action keys) | | open |
| S1 | Standards | P1 | `docs/adr/0058-hermetic-c-toolchain-results.md` | First non-decision file in `docs/adr/`; README does not mention companion files | | open |
| S4 | Standards | P1 | `docs/adr/0058-hermetic-c-toolchain-results.md:3` | "the workflow is in the history" goes stale after the squash | | open |
| S2 | Standards | P3 | ADR-0058 decision 6 | CI numbers repeated from Context/Consequences | | open |
| S3 | Standards | P3 | ADR-0058 decision 4 | zig-path rationale repeated 4× | | open |
| P12 | Spec | P3 | ADR-0058:42-47 | "one run each" while the table shows ranges | | open |
| X* | Security | Rejected | — | No findings (no secret committed; workflow push-only, removed) | | rejected (no findings) |

User decision (2026-11-15): Fix P1 + P3 (docs only), no further review round, then finish.

## Finish (2026-11-15)

- Fix round 3: 7224221a; P11, S1, S2, S3, S4, P12 fixed. `just docs-build` and `openspec validate --strict` ok.
- Container tests: ran (round 2, `unix:///var/run/docker.sock`), 11 `no-remote-exec` targets pass. `bazel test -k //...` 52/53; `//crates/factory-agent:sandbox_test` fails on this VM only (no Landlock ABI 6), unrelated to the change.
- Bazel lockfile build (`--lockfile_mode=error`) ok after the rustix bump (round 2).
- Tech debt: none created (no P2 findings); TD-27 updated by the change.
- Roadmap backlog: "Container-backed tests on RBE workers", "Rust 1.100/1.101 upgrade (drop zig patch argument)".
- Open cosmetic: S8 (rollback sentence repeated).
