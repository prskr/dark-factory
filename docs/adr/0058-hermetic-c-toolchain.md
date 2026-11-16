# ADR-0058: Hermetic C toolchain for Linux exec tools and remote execution

- **Status:** Accepted
- **Date:** 2026-11-14
- **Related:** ADR-0023, [remote-execution.md](../remote-execution.md), TD-27, issue #172

## Context

Bazel linked exec tools (build scripts, proc-macros) with the host's C toolchain (`@local_config_cc`). Builds depended on the developer's `gcc` and `ld` (gold prints a deprecation warning on every link), cache hits were shared only between hosts with the same compiler, and remote execution was blocked because workers would need a matching host toolchain. Plain `cargo` on aarch64 Linux linked with GNU ld, and linking is most of a warm `cargo test` build.

Tests and images are static musl on every Linux target (`toolchains_musl`). Exec tools stay glibc, because proc-macros are shared objects loaded into the glibc `rustc`. The registration order is fixed ([remote-execution.md](../remote-execution.md) 2.1): the musl toolchains come before everything that matches cpu and os only.

A spike on the branch `refactor/hermetic-cc-toolchain` tried two candidates and measured the build, remote execution and CI. Setup: aarch64 NixOS (nix-ld), 8 cores, `build //...` with 9,725 actions, `bazel clean` before each run, no remote cache unless stated. All runs, run links and the cases that were not measured: [the results](0058-hermetic-c-toolchain-results.md).

**Candidates.** `toolchains_llvm` (prebuilt LLVM 18/21 and a Debian bullseye sysroot) does not run on the main development platform: its binaries need host libraries (`libtinfo.so.6`, `libxml2.so.2`, `libz`, `libstdc++`) that nix-ld does not provide. Its download is 1.05-2.0 GB per arch, 11 GB extracted. `hermetic_cc_toolchain` (zig cc, glibc 2.28) is one static binary: 50 MB download, 370 MB extracted, and it ran on aarch64 NixOS, an aarch64 and an amd64 Ubuntu runner and in a Debian container without `gcc`, binutils or gold.

**Build time** (`bazel build //...`, arm64):

| Exec C toolchain | Wall | Note |
|---|---|---|
| host `gcc` | 546 s, 525 s | |
| zig, default configuration | 1166 s | every action rebuilt zig's libc: the actions have no `HOME` and a private `/tmp` |
| zig, fixed cache directory visible to the sandbox | 562 s, 550 s | +5 %, within the 4 % noise of the base |
| one-crate change | 153 s (host `gcc`), 157 s (zig) | |

**Remote execution** (BuildBuddy cloud, amd64 workers, default Ubuntu 24.04 image; `build //...` from the arm64 client, CI job of `ci.yaml` on `ubuntu-latest` with 4 vCPU):

| Case | Remote execution | Before |
|---|---|---|
| arm64 client, cold remote cache | 573 s | 550 s (local) |
| arm64 client, fresh output base, warm remote cache | 66 s | 550 s (local) |
| arm64 client, one-crate change | 111 s | 153 s (local) |
| CI job, cold cache | 10-15.5 min | 43-48 min (remote cache only; two more runs failed after 58 min with test timeouts) |
| CI job, warm cache | 3.5 min | 6-11 min |

Clients that are not amd64 Linux need pinned action keys to share the cache (Decision 7); on macOS a case-insensitive volume still makes about half of the actions miss (1,313 of 2,789 hits; all hit on a case-sensitive volume).

**Plain `cargo`** (aarch64, `cargo test --workspace --no-run`, 32 GB): 440 s with GNU ld, 169 s with the toolchain's `rust-lld` (-62 %); warm `df-core` rebuild 10.0 s against 2.8-3.0 s (-70 %).

**Test recipe** (`just bazel-test-rbe` against `just bazel-test`, arm64 Linux, tests included; the values come from different probe edits, so the number of tests that ran varies, see the results):

| Case | Recipe | `just bazel-test` |
|---|---|---|
| change that relinks the dependents, warm cache, steady state | 157-166 s | 219-231 s |
| fresh output base, remote cache holding only what CI builds | 291 s | 638 s |

## Decision

1. **zig cc is the C toolchain of the Linux exec platforms** (amd64 and arm64, glibc 2.28), registered after `@musl_toolchains_hub` and before the Rust toolchains. macOS hosts keep Xcode's clang for local exec tools. `toolchains_llvm` is rejected.
2. **A patch and a fixed cache.** `build/patches/hermetic_cc_toolchain-rustc-linker-args.patch` drops two arguments of `rustc` that zig's linker driver rejects or complains about (`-Wl,--fix-cortex-a53-843419`, uber/hermetic_cc_toolchain#269, obsolete with rustc 1.101; `-Wl,-O1`). `.bazelrc` fixes zig's cache at `/var/tmp/zig-cache` and mounts `/var/tmp` into the sandbox.
3. **`toolchains_musl` stays** for the musl targets. It needs no change, and replacing it for one dependency less would change the toolchain of every shipped artifact.
4. **`--config=rbe` on BuildBuddy**, targeting `linux_amd64_musl` on workers with the default executor image. `--config=rbe-cross` serves clients that are not amd64 Linux. Tests that start containers or need the client's tools carry the tag `no-remote-exec` and run on the client; `build/check_no_remote_exec.sh` fails CI for a database test without it. `just bazel-test-rbe` runs everything from one command. It is faster than `just bazel-test` on arm64 Linux in both measured cases (about 30 % with a warm cache, 54 % from a fresh checkout), so it stays. Limit: on a fresh checkout its local half (the tagged tests built for arm64) is always cold, because CI uploads only amd64 outputs.
5. **Plain `cargo` links with `rust-lld` on aarch64 Linux** through `.cargo/cc-lld.sh`; it falls back to plain `cc` without a `rust-lld` and with a Nix `cc`, whose `ld` wrapper adds the RUNPATH of the store libraries. `x86_64-unknown-linux-gnu` already uses it. A pinned `ld.lld` in `bazel_env` is not needed.
6. **CI and releases use `--config=rbe`**: the `bazel` job of `ci.yaml` and the two image jobs of `release.yaml`. The gate passed (see Context and the results). `docs.yaml`, `harness-images.yaml`, `e2e.yaml` and the chart and docs jobs keep `--config=cache`.
7. **Every `rbe` client pins its cpu name and the `layering_check` features** (`--cpu=k8`, `--features`, `--host_features`), because both are part of every action key.

## Consequences

- A Linux host needs no `gcc`, `clang`, binutils or system library for Bazel, and no `nix-ld` libraries for the C toolchain. Tests still call the host's `file` (`//deploy:images_structure_test`, the harness `structure_test` targets) and `readelf` (the two harness `structure_test` targets); in the container without a compiler the former passed because `file` was installed and the latter failed. `//crates/factory-agent:sandbox_test` needs Landlock ABI 6 and fails where it is missing.
- First fetch is 50 MB (370 MB on disk) instead of 1-2 GB (up to 11 GB).
- The build depends on a patched third-party toolchain whose wrapper filters linker arguments, and on a fixed cache path. Both are documented where they are set.
- macOS clients need a case-sensitive volume for the output base to share the cache with Linux clients.
- `@zig_sdk//toolchain/linux-amd64` exists only on hosts that are not amd64 Linux, so `//build/platforms:zig_cc_exec_linux_amd64` references the private repository instead; it is registered before the host's own toolchains, so that all clients share the exec actions.
- The container-backed tests still run on the client; running them on workers stays TD-27.
- During the spike one test, `df_test_postgres_test::the_sweep_drops_released_databases_and_keeps_those_with_an_open_pool`, failed in 3 of 6 cold CI runs with remote execution. It asserted a drop right after closing a pool, before the server had ended the connection; it now waits for that and passed 30 of 30 local runs, also under CPU load. After the fix all 4 cold CI runs with the steps of `ci.yaml` (checks, `Test`, Clippy, Rustfmt) passed, in 10-15.5 min; two of them ran before the amd64 exec toolchain was registered first.
- Rollback: replace `--config=rbe` with `--config=cache` in `BAZEL_ARGS` (CI, releases). Removing the zig registration from `MODULE.bazel` restores the autodetected host compiler, which is a prerequisite again.
- Not measured: the baseline first-fetch size and an amd64 baseline of the Bazel and `cargo` builds; `toolchains_llvm` on amd64, in CI and for the musl targets; BuildBuddy minutes; the cache hit rate between two amd64 hosts; macOS developer machines (only the `macos-latest` runner).

## Alternatives considered

- **`toolchains_llvm`:** see Context; fails on the main development platform and its download is 20-40 times larger.
- **A checked-in `cc_toolchain` for a fixed executor image:** fixes remote execution only, not local Linux hosts.
- **`mold` for `cargo`:** one more pinned tool; the toolchain's `rust-lld` is already there.
- **Replacing `toolchains_musl`:** no benefit beyond one dependency less.
- **Running the container-backed tests on workers** (`dockerd` in the executor): backlog.
