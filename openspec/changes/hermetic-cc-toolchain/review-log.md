# Review log: hermetic-cc-toolchain

- BASE: b7a5d9f3
- Store: none
- Status: in-progress
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
