# Spike results: hermetic C toolchain

Measurements behind [ADR-0058](0058-hermetic-c-toolchain.md), run by run. Setup unless stated otherwise: aarch64 NixOS (glibc 2.42, GNU ld 2.46, nix-ld), 8 cores, `build --jobs=8`, no remote cache, `bazel clean` (not `--expunge`) before each timed run so that downloads are not timed, repository cache warm, `build //...` (205 targets, 9,725 actions). RAM was 15 GB for the Bazel runs and 32 GB for the `cargo` runs. The CI numbers come from a temporary workflow that ran the steps of `ci.yaml` with an isolated remote cache per run; the runs are linked below.

## Candidates

| | `toolchains_llvm` 1.13 (LLVM 18.1.8 / 21.1.8, Debian bullseye sysroot) | `hermetic_cc_toolchain` 4.3.0 (zig 0.15.2) |
|---|---|---|
| libc of the exec tools | glibc 2.31 (Chromium sysroot, 18-19 MB) | glibc 2.28 (zig's stubs, selectable down to 2.17) |
| Download, one arch | 1.05 GB (18.1.8, both arches); 1.87 GB arm64, 2.02 GB amd64 (21.1.8); 11 GB extracted (21.1.8, arm64) | 49.5 MB arm64, 53.7 MB amd64; 368 MB extracted |
| Runs on aarch64 NixOS | no. 18.1.8 `clang` needs `libtinfo.so.6` (exit 127 in `cc_wrapper.sh`), 21.1.8 `ld.lld` needs `libxml2.so.2` (nix-ld provides `.so.16`); 19.1.7 and 20.1.8 `ld.lld` need `libxml2.so.2` too. The loader (nix-ld) works. | yes, one static binary |
| Runs on amd64 Linux (`ubuntu-latest`) | not tried | yes |
| Needs host shared libraries (`libz`, `libstdc++`, `libtinfo`, `libxml2`) | yes | no |
| Needed changes | none found before it was dropped | a patch to the linker wrapper and a cache directory |

LLVM was not measured on amd64, in CI, for the musl targets or on the glibc-2.31 sysroot link: it stopped working at the first link.

## Build time (aarch64 NixOS, `bazel build //...`)

| Run | Wall | Critical path | Note |
|---|---|---|---|
| base (host `gcc`, GNU ld) | 546 s, 525 s (repeat) | 221 s, 204 s | the first, uncontrolled run took 593 s |
| zig, default configuration | 1166 s, 1158 s | 345 s, 380 s | a warm `~/.cache/zig` did not help |
| zig, cache directory fixed and mounted into the sandbox | 562 s, 550 s (final `.bazelrc`) | 211 s, 216 s | +5 % against the base, within the base's own noise of 4 % |
| one-crate change (`df-core`), `build //...` | base 153 s (158 s earlier), zig 157 s | 98 s, 110 s | |
| no-op | 2 s | | |

Cause of the 2x: the actions have no `HOME` and the sandbox has a private `/tmp`, so zig's libc and compiler_rt build products (`ZIG_GLOBAL_CACHE_DIR`) were rebuilt by every action that linked. In the profile the average duration of proc-macro links went from 2.0 s to 20.1 s and of build script links from 0.5 s to 17.7-18.5 s, while rlib compiles stayed at 1.85 s vs 1.88 s. The cache grows to ~90 MB.

## Host without a C compiler, gold

Debian bookworm-slim container (arm64, no `gcc`, `cc`, `clang`, `ld`, `ar`; `file` installed, no `readelf`; Bazel 9.3.2, 8 jobs, cold output base): `bazel test -k --test_tag_filters=-no-remote-exec //...` ran 42 tests, 39 passed in 654 s. The failures: `//crates/factory-agent:sandbox_test` needs Landlock ABI 6 and the container has none (the test prints that requirement), and the two `//deploy/images/*-harness:structure_test` call `readelf` (not installed). `//deploy:images_structure_test`, which calls only `file`, passed. A second container with binutils where `/usr/bin/ld` is gold built `//crates/df-core/... //crates/df-telemetry/... //crates/df-db/...` in 103 s without any "gold" or deprecation message; zig's lld printed `ignoring deprecated linker optimization setting '1'` for the first links, which the patch removes. On aarch64 NixOS with `nix-ld`, `bazel test //...` with the Docker socket passes 52 of 53 tests; `sandbox_test` fails on this VM as in the container.

## Remote execution (BuildBuddy cloud, `--config=rbe`)

| Scenario | Result |
|---|---|
| arm64 Linux client, cold remote cache, `build //...` without the container-backed and `buf` targets | 573 s (local build: 550 s), critical path 320 s, 4,950 remote actions |
| same client, `bazel clean`ed output base, warm remote cache | 66 s |
| one-crate change (`df-core`), `build //...` | 111 s (local 153 s) |
| `test --config=rbe-cross //...`, warm | 27 s, 42 tests pass |
| local run of the tests tagged `no-remote-exec` (arm64, `host_musl`, Docker) | 11 of 11 pass, 580 s from a cold output base |
| `ubuntu-24.04-arm` runner, `rbe-cross`, build, test, build again | all pass; the second `build` is served from the cache in 1 s |
| `macos-latest` runner, `rbe-cross`, same | all pass, 5-7 min cold |

The default executor image (`gcr.io/flame-public/rbe-ubuntu24-04`, pinned by index digest) is enough. BuildBuddy cloud also has arm64 executors (a one-action probe with `Arch=arm64` ran on `aarch64`); they are not wired into the build.

## Test recipe: `just bazel-test-rbe` against `just bazel-test` (arm64 Linux, plain `bazel`, repo `.bazelrc` and `~/.bazelrc`, Docker socket)

The recipe on arm64 Linux is `bazel test --config=rbe-cross //...` followed by `bazel test --config=cache --build_tests_only --test_tag_filters=no-remote-exec //...` with the socket flags. Without `--build_tests_only` the second half built all of `//...`, images included, locally (580 s from a cold output base). `just bazel-test` has one failing test here (`sandbox_test`, no Landlock); the recipe runs it on a worker, where it passes. `~/.bazelrc` only holds the API key, so `just bazel-test` is purely local.

**Realistic cases.**

(a) A code change that changes the binaries: a no-op arithmetic expression with a different constant in `df_core::title_too_long`, which the dependents call. Dependents relink and their tests rerun. Warm local cache; one run of the mode first (steady state), then the measured one. Sixty-odd tests are affected differently per probe value, so the numbers vary with how many tests had to run.

(b) A fresh checkout whose remote cache holds only what CI puts there: an isolated `--remote_instance_name` (set in `.bazelrc.user`) filled by one run of the `ci.yaml` Bazel steps with `--config=rbe` on amd64 (`ubuntu-latest`), then `bazel clean` and the recipe. Its local half runs cold: the tagged tests built for arm64 `host_musl`, nothing of them is in the remote cache.

| Case | `just bazel-test-rbe` | `just bazel-test` |
|---|---|---|
| (a) real change, warm, steady state | 157 s (8 tests ran), 166 s (33 tests ran, other probe value) | 231 s (22 tests ran); 219 s in the first measurement |
| (b) fresh output base, remote cache from CI only | 291 s: 5,513 of 5,513 remote actions from the cache, local half cold | 638 s |
| (b) same, before the amd64 exec toolchain was registered first (`zig_config` of the amd64 host and `zig_config-linux-amd64` differ in every command line) | 593 s: 1,650 of 5,513 from the cache | 638 s |

Earlier, favorable setups (a comment appended to `df-core`, so that the rlib stays identical and dependents hit the action cache; a cache holding the recipe's own host uploads) gave 166 s against 219 s and 163 s against 638 s.

## CI gate

Spike workflow on `ubuntu-latest` (4 vCPU), one job per mode with the steps of the `bazel` job of `ci.yaml`. "Cold" runs use an isolated cache (`--remote_instance_name` per run), "warm" runs the shared one after earlier runs of the same code. Single runs; all that completed are listed.

| Mode | Cold: job / `Test` step | Warm: job / `Test` step |
|---|---|---|
| `--config=rbe` | 12 min 32 s / 618 s; 12 min 54 s / 657 s; 10 min 32 s; three more cold runs took 11-12 min and failed in `Test` on the flaky test below | 3 min 32 s / 85 s; 3 min 17 s |
| `--config=cache` (before the change) | 43 min 13 s / 2458 s; 48 min 18 s / 2758 s; two cold runs failed after 58 min because three or four tests hit their 300 s timeout (`df_web_test`, `dark_factory_test`, the `df_plugins` tests) when everything executes on the 4 vCPU runner | 5 min 56 s / 231 s; 11 min 20 s |

The failing step in the three failed `rbe` runs was one test, `df_test_postgres_test::the_sweep_drops_released_databases_and_keeps_those_with_an_open_pool`: it asserted a drop right after `pool.close()` but the server had not yet ended the backend. It failed in 3 of 6 cold `rbe` runs, in none of the cold `cache` runs that passed and in none of the local runs; under 16 busy loops it failed in 1 of 30 runs. It is fixed (the test waits for the server to drop the connection); with the fix it passed 30 of 30 unloaded and 30 of 30 under load. The release dry run (structure tests, building the push targets and harness digests, no login, no push) passed in 7-10 min cold and 3 min warm.

Runs: https://github.com/prskr/dark-factory/actions/runs/43646953872 (cold, both modes), 43647063710 (cold), 43646380052 (warm), 43643889657 (warm), 43648403615, 43649968399, 43651278016 (failed on the flaky test), 43652501577; the first runs 43633913440 and 43631340593 failed before the fixes below.

Runs with the sweep-test fix (only the `rbe` job of the `ci.yaml` steps, isolated cache per run; every step passed): 43696058100 (14 min 23 s, `Test` 708 s), 43697817539 (10 min 29 s, 511 s), 43699377285 (15 min 27 s, after registering the amd64 exec toolchain first), 43701151809 (14 min 7 s, final configuration). Before the fix 3 of 6 cold runs failed.

Findings that changed the configuration:

- The zig toolchain of the amd64 workers is not declared on an amd64 Linux host, so a plain registration failed on the runner; `//build/platforms:zig_cc_exec_linux_amd64` references the private repository instead. It is registered before the host's toolchains: otherwise an amd64 client runs different zig paths than the other clients and shares only 30 % of the remote actions with them.
- `--extra_toolchains` put the zig toolchain before `toolchains_musl` ("duplicate symbol: _start").
- The cpu name and the `layering_check` features of the client are part of every action key, so arm64 Linux, macOS and amd64 clients shared nothing until `build:rbe` pinned them (`--cpu=k8`, `--host_cpu=k8`, `--features`, `--host_features`).

## Cache sharing between clients

Cold isolated cache, same commit: the `ubuntu-24.04-arm` runner executed 2,789 remote actions; the `macos-latest` runner afterwards got 1,313 of them from the cache on the default case-insensitive volume and all 2,789 with the output base on a case-sensitive APFS volume (the extracted `toolchains_musl` tree loses headers whose names differ only in case, and the toolchain files are inputs of every Rust action). A second invocation on the same client is a cache hit (1-2 s). A runner of `ubuntu-24.04-arm` (Ubuntu's `gcc` and GNU ld) got 1,118 of 1,118 non-internal actions of `build //crates/df-core/... //crates/df-telemetry/...` from the cache that the aarch64 NixOS machine had filled, without remote execution (`--config=cache`).

## Plain `cargo` (aarch64 Linux, `cargo test --workspace --no-run`, 8 jobs, 32 GB)

| Linker | Cold | `touch crates/df-core/src/lib.rs` + `cargo test -p df-core --no-run` (warm) |
|---|---|---|
| GNU ld (default) | 445 s, 439 s | 10.0 s |
| `rust-lld` | 169 s | 2.8-3.0 s |

A first attempt with 8 jobs on 15 GB was killed by the kernel (peak 18 GB resident while test binaries link). `-Clinker-features=+lld -Clink-self-contained=+linker` needs `-Zunstable-options` on 1.99.1; `-Clink-arg=-fuse-ld=lld` needs the directory of the toolchain's `gcc-ld` wrapper (a sysroot path that `.cargo/config.toml` cannot express), and that wrapper is broken in the Nix packaging of rustup. A driver script that links `rust-lld` as `ld.lld` into a temporary `-B` directory works with `gcc` and `clang` drivers (`.cargo/cc-lld.sh`).

## Negative case of the tag check

`build/check_no_remote_exec.sh` with the tag removed from `//crates/df-db:df_db_test`: exit code 1, message `These tests depend on //crates/df-test-postgres but lack the tag "no-remote-exec": //crates/df-db:df_db_test`. With the tags in place: exit code 0.

## Not measured

- Baseline first-fetch size of the host-toolchain build (no new downloads for the base) and an amd64 baseline of `bazel build //...` and `cargo test --workspace --no-run`.
- `toolchains_llvm` on amd64, in CI, for musl targets.
- BuildBuddy minutes (not visible with the organization key).
- The cache hit rate between two amd64 Linux hosts, and between hosts with different system compilers beyond the one small target set above.
- macOS: only the `macos-latest` runner (arm64), no developer machine.
