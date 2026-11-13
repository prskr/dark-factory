# Design

## Context

- Toolchains today (`MODULE.bazel`): Rust musl toolchains and `@musl_toolchains_hub` (`toolchains_musl` 0.2.0, GCC 11, `ld.bfd`) are registered first for the `//build/platforms:*_musl` targets; `@rust_toolchains//:all` follows. No C toolchain is registered for the exec platform, so Bazel autodetects `@local_config_cc` from the host.
- Target platforms: `host_musl` locally on Linux, `linux_{amd64,arm64}_musl` for images and CI, native macOS on macOS hosts, `wasm32_wasip2` for plugins. The exec platform is always the plain host (glibc on Linux).
- Proc-macros are `*-unknown-linux-gnu` shared objects loaded by the exec `rustc`; build scripts are exec binaries. Both link against glibc, so the exec toolchain needs a glibc sysroot no newer than the oldest host glibc that runs them (CI `ubuntu-latest`, Debian/Ubuntu dev machines, aarch64 NixOS through `nix-ld`, BuildBuddy executors).
- History (`docs/remote-execution.md`): `toolchains_llvm` with Debian sysroots ran on BuildBuddy RBE before. Its problems were the glibc-tests/musl-images split and the registration order (fixed since), not the exec toolchain itself. That setup needed explicit `-L` sysroot link flags for rust-lld.
- `.bazelrc` already has `build:cache` (BuildBuddy cache, BES, credential helper). `.bazelrc.agents` has no remote config.
- `cargo` builds on `aarch64-unknown-linux-gnu` link with the system `cc` (GNU ld); `x86_64-unknown-linux-gnu` already uses the bundled `rust-lld` by default.

## Goals / Non-Goals

**Goals:**
- Answer the six questions of #172 with measurements, recorded in an ADR, before changing defaults.
- Linux exec builds independent of the host's C toolchain, for amd64 and arm64.
- A `--config=rbe` that builds `//...` from Linux and macOS clients.
- CI and release workflows on `--config=rbe` in this change, if the gate of D6 passes.
- A faster linker for plain `cargo` only if the numbers justify it.

**Non-Goals:**
- Container-backed tests on RBE workers (they run on the client, D4): backlog (`roadmap.md`, new row "Container-backed tests on RBE workers", linked to TD-27).
- Hermetic exec toolchain on macOS hosts: rejected (see proposal).
- Replacing `toolchains_musl` as a goal in itself: only if the spike shows it simplifies `MODULE.bazel` (D3); otherwise rejected.
- Changing the Rust toolchain, `rules_rust` version or target platforms: rejected, not needed.

## Decisions

### D1. Spike before switching, results in an ADR

The spike runs on a branch and fills a results table (questions 1–6 of #172): works yes/no per arch, sysroot glibc version, first-fetch download size, `bazel build //...` wall time cold and warm against today, remote cache hit rate between two different Linux hosts, RBE wall time. The ADR (`docs/adr/0058-hermetic-c-toolchain.md`) records the table and the decision; the implementation tasks after the spike follow it. If both candidates fail on a question, the ADR says so and the change shrinks to what works (for example exec toolchain without RBE).

Alternative: switch straight away. Rejected: the previous attempt was removed after it had shipped; this time the evidence comes first.

### D2. `toolchains_llvm` for the Linux exec platforms, zig cc as fallback

`toolchains_llvm` (`llvm.toolchain` with a pinned LLVM release per host arch, `sysroot` per arch) registered with `exec_compatible_with`/`target_compatible_with` set to Linux + cpu and **without** the `:musl` constraint, after `@musl_toolchains_hub//:all`. Musl target platforms keep resolving to `toolchains_musl` (registration order, `docs/remote-execution.md` §2.1); the plain Linux platform (exec) resolves to LLVM.

Sysroot: a pinned glibc sysroot per arch with an old glibc (target: ≤ 2.31, e.g. a Chromium or Debian bullseye sysroot), so the produced exec tools also load on older hosts and executors. The spike confirms the version against the oldest host in use.

Rust side: `rules_rust` links exec binaries and proc-macros through the resolved `cc_toolchain`, so no `rust.toolchain` change is expected; if rust-lld needs the sysroot library paths (as before), they go into `extra_exec_rustc_flags` per exec triple.

`hermetic_cc_toolchain` (zig cc) is only evaluated if `toolchains_llvm` fails a question. It would cover glibc exec and musl targets with one toolchain, but is untested with `rules_rust` here and zig's glibc version selection differs.

Alternative: a checked-in `cc_toolchain` for a fixed executor image (`docs/remote-execution.md` §3.2 A). Rejected: it fixes RBE only, not local Linux hosts.

### D3. Keep `toolchains_musl` for the musl targets unless LLVM is simpler

Default: keep. It works, ships `ld.bfd`, and the musl runtime of the harness images comes from Bootlin anyway. Replace it only if the spike shows `toolchains_llvm` builds the static musl targets (with a musl sysroot) with the same or less configuration and `//deploy:images_structure_test` passes. One toolchain dependency less is the only benefit.

### D4. `--config=rbe` on BuildBuddy

```
build:rbe --config=cache
build:rbe --remote_executor=grpcs://remote.buildbuddy.io
build:rbe --extra_execution_platforms=//build/platforms:linux_amd64_rbe
build:rbe --host_platform=//build/platforms:linux_amd64_rbe
build:rbe --jobs=50
```

No `--platforms` in `build:rbe`: `ci.yaml` passes `--config=linux-amd64-musl` itself, and the release jobs deliberately set no target platform because the image targets transition to amd64 and arm64.

- `linux_amd64_rbe` (and `linux_arm64_rbe` if BuildBuddy's cloud offers arm64 executors; otherwise arm64 stays cross-compiled as today): `@platforms//os:linux`, cpu, `exec_properties` with `OSFamily`, `Arch` and `container-image` pinned by digest.
- Executor image: with a hermetic toolchain and sysroot, BuildBuddy's default image should suffice (it needs `bash`, coreutils, `tar`, `file` for test scripts). Only if a test or repository rule needs more, a minimal image goes to `deploy/images/rbe-executor`, built with `rules_img` like the other images and published to `ghcr.io/prskr/dark-factory/rbe-executor` by the existing image workflow.
- Container-backed test targets (`df_db_test`, `df_web_test`, ... — everything that depends on `df-test-postgres`) get Bazel's built-in `no-remote-exec` tag. Under `--config=rbe` they run on the client (the GitHub runner in CI, which has Docker), with their binaries taken from the remote cache; every other action, tests included, runs on BuildBuddy. Without `--config=rbe` the tag changes nothing. A repository check (the existing `cargo xtask check-crates` or a `bazel query` in CI) fails when a test target depends on `//crates/df-test-postgres` without the tag, so a new database test cannot silently land on a worker without Docker.
- Alternative: run all tests on the client (`--strategy=TestRunner=local`). Simpler (one flag, no tags), but test parallelism drops to the runner's cores and every test binary is downloaded. Kept as the fallback if the executor image turns out to be a problem for the image structure tests.

### D5. Plain `cargo`: measure, then the least config that works

Measure on an aarch64 Linux host: `cargo test --workspace --no-run --timings` with the default linker vs. `RUSTFLAGS="-C link-arg=-fuse-ld=lld"`. Threshold: if the faster linker saves less than 10 % of a warm incremental `cargo test -p df-core --no-run` and of a cold `--workspace` run, drop the cargo part and remove the third requirement from the spec delta.

If it pays off, pick the first option that works on stable Rust 1.99:
1. Rust's bundled `rust-lld` (`-Clinker-features=+lld -Clink-self-contained=+linker` for `aarch64-unknown-linux-gnu`) in `.cargo/config.toml`, if stable for that target. Needs no extra tool and no fallback.
2. Otherwise the pinned `ld.lld` from D2 in `bazel_env`, and the system `cc` as linker driver with `-fuse-ld=lld -B<dir of ld.lld>`. The host's crt files and glibc stay in use, so plain `cargo` keeps working on NixOS. The flags are set by `.envrc` (`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS`) only when `.bazel_env/bin/ld.lld` exists, which is the fallback: `.cargo/config.toml` cannot express "if present".

Alternative: `mold`. Rejected: one more pinned tool, and lld comes with D2 for free.

### D6. CI and releases switch to `--config=rbe` behind a gate

Gate, measured in the spike on the change branch:
1. The `bazel` job of `ci.yaml` passes every step (testcontainers check, Test, Clippy, Rustfmt) with `--config=rbe`, the database tests running on the runner.
2. Its wall time is not higher than with `--config=cache`, on a cold remote cache and on a warm one (same commit run twice; two runs each, median).
3. The build and test steps of both release jobs (`images`, harness images) pass with `--config=rbe`, run on the branch from a manual workflow run that stops before any push (release builds are `workflow_dispatch` only and push for real).

If all three hold, `BAZEL_ARGS` in `ci.yaml` and both release jobs replace `--config=cache` with `--config=rbe` (which includes it). BuildBuddy minutes are recorded in the ADR but do not decide. If one fails, both workflows stay on `--config=cache`, the spec's "Remote execution rejected" scenario applies, and the ADR records the numbers.

`bazel run //deploy:push_images` runs the push tool on the runner; with remote execution its runfiles (image layers) are downloaded first (`--remote_download_outputs=toplevel`, Bazel's default). The release check in step 3 covers this up to the push.

Alternative: switch only `ci.yaml` and keep releases on the cache. Rejected: one configuration for both keeps the release images built by the same actions that CI tested.

## Risks / Trade-offs

- [Large first fetch (LLVM ~1 GB per arch, sysroot)] → Measured in D1; CI repository cache keeps it; agents fetch once per sandbox. If it dominates sandbox build time, the ADR may restrict the hermetic toolchain to CI and RBE via a config instead of the default.
- [Sysroot glibc newer than a host] → Pick an old sysroot (D2); the spike runs the exec tools on the oldest host in use.
- [aarch64 NixOS: prebuilt LLVM binaries need a dynamic loader] → `nix-ld` provides it; verified in the spike. If it fails, NixOS users keep `--extra_toolchains=@local_config_cc//:all` in `.bazelrc.user` (documented).
- [Toolchain resolution picks LLVM for musl targets] → Registration order after `@musl_toolchains_hub` and `//deploy:images_structure_test` (static ELF check) catch it.
- [BuildBuddy cloud has no arm64 executors] → arm64 stays cross-compiled under RBE, as it is today without RBE.
- [Cost of RBE minutes] → recorded in the ADR; local builds stay local.
- [BuildBuddy outage or quota blocks CI and releases] → Rollback is replacing `--config=rbe` with `--config=cache` in `BAZEL_ARGS`; the comment next to it says so.
- [A new database test target without `no-remote-exec` fails on the workers] → The repository check of D4.

## Migration Plan

Contributor-only change. Developers on Linux run `bazel fetch` once (new toolchain repositories). Rollback: revert the `MODULE.bazel` registration; `@local_config_cc` is autodetected again. CI and releases go back to `--config=cache` by changing `BAZEL_ARGS`.

## Open Questions

- Exact LLVM release and sysroot source (Chromium vs. Debian bullseye): picked in the spike by availability for both arches and checksum pinning; does not change the approach.
- Whether BuildBuddy's default executor image is enough (D4): only decides whether task 4.3 produces an image.
