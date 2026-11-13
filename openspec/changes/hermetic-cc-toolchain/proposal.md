# Proposal

## Why

Bazel links exec tools (`build.rs` scripts, proc-macros) with whatever C toolchain the host has (`@local_config_cc`). Builds therefore depend on the developer's `gcc`/`ld` (a gold linker prints a deprecation warning on every link), cache hits are only shared between hosts with the same compiler, and remote execution is blocked because workers would need a matching host toolchain ([#172](https://github.com/prskr/dark-factory/issues/172), `docs/remote-execution.md` §2.2, TD-27). Separately, plain `cargo` builds on aarch64 link with GNU ld, and agents spend most of a `cargo test` loop compiling and linking.

An earlier setup used `toolchains_llvm` only for RBE with glibc test builds and was removed because tests and images ended up as two builds (`docs/remote-execution.md` §1). Since then all Linux targets are static musl and the toolchain registration order is fixed (§2.1), so the hermetic exec toolchain can be retried without that split.

## What Changes

- Spike first, then decide (ADR): `toolchains_llvm` (pinned clang + lld, glibc sysroot) as the C toolchain of the Linux exec platforms (amd64, arm64) with `rules_rust` 0.73; `hermetic_cc_toolchain` (zig cc) only if `toolchains_llvm` falls short. The spike answers the six questions of #172 and records measurements.
- On Linux hosts, Bazel no longer uses the host's C compiler or linker for exec tools. `gcc`/binutils are no longer a prerequisite for Bazel builds on Linux. macOS hosts keep Xcode clang for local exec tools.
- `toolchains_musl` stays the C toolchain of the static musl target platforms unless the spike shows `toolchains_llvm` covers them with less configuration (decided in the ADR, not assumed).
- New `--config=rbe` in `.bazelrc`: remote execution on BuildBuddy (`grpcs://remote.buildbuddy.io`) with a Linux execution platform that carries the executor image in `exec_properties`. A custom executor image is built under `deploy/images` only if the default image is not enough. Local builds stay local unless a developer passes the flag.
- CI switches to RBE in this change if the spike passes the gate (design D6): the Bazel jobs in `.github/workflows/ci.yaml` and `.github/workflows/release.yaml` use `--config=rbe` instead of `--config=cache`. Tests that need a container runtime (everything on `df-test-postgres`) keep running on the GitHub runner; all other actions, tests included, run on BuildBuddy. If the gate fails, CI stays on `--config=cache` and the ADR records why.
- Plain `cargo`, gated by a measurement: if linking is a relevant share of `cargo test --workspace --no-run --timings`, `bazel run //:bazel_env` also puts the pinned `clang`/`ld.lld` on `PATH` and `.cargo/config.toml` links the `*-unknown-linux-gnu` host targets with them, falling back to the default linker when they are not on `PATH`. Otherwise this part is dropped and the ADR records the numbers.
- Docs: new ADR, `docs/development.md` (toolchains, host prerequisites, `--config=rbe`), `docs/remote-execution.md` (status, §3 updated to what was built), `docs/tech_debt.md` (TD-27).

Out of scope:
- Running container-backed tests (`df-test-postgres`) on RBE workers (`dockerd` in the executor), so that they no longer need the GitHub runner: backlog (`roadmap.md`, new row "Container-backed tests on RBE workers"), together with TD-27.
- RBE for agent sandboxes (`.bazelrc.agents`): rejected for now; BuildBuddy is unreachable from the sandboxes (ALPN errors).
- A hermetic exec toolchain for macOS hosts (Apple SDK sysroot): rejected; under RBE the exec platform is the Linux worker, and local macOS builds work with Xcode.
- Less debug info in the dev profile, `cargo nextest`: stays in the backlog row of `e2e-suite-and-seed` ("Measure compile time; dev-profile debug info, nextest").

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `build-pipeline`: adds requirements for a hermetic C toolchain of the Linux exec platforms, a remote execution configuration, CI and release builds on remote execution, and the optional pinned linker for plain `cargo`.

## Impact

- `MODULE.bazel` (`toolchains_llvm` or `hermetic_cc_toolchain`, sysroots, `register_toolchains` order), `MODULE.bazel.lock`, `build/platforms/BUILD.bazel` (RBE execution platform), `.bazelrc` (`build:rbe`), `BUILD.bazel` (`bazel_env` tools), `.cargo/config.toml` (conditional).
- `.github/workflows/ci.yaml` and `.github/workflows/release.yaml` (`BAZEL_ARGS`), `BUILD.bazel` files of the container-backed test targets (`no-remote-exec` tag).
- Possibly `deploy/images/rbe-executor` and a CI workflow that publishes it, only if the default BuildBuddy image is not enough.
- No Rust code, no runtime behavior, no published docs: contributor docs only (`docs/adr/`, `docs/development.md`, `docs/remote-execution.md`, `docs/tech_debt.md`, `.agents/skills/rust/SKILL.md` if host prerequisites change).
- First fetch downloads a pinned LLVM distribution and sysroot per host arch (size measured in the spike).
