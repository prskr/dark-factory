# Bazel: "No matching toolchains found" with `.bazelrc.agents` on macOS

Status: open. Found while implementing `iter1-run-orchestration` (2026-10-19). Cargo builds and tests
are unaffected; only Bazel with the remote config fails.

## Symptom

```bash
bazel --bazelrc=.bazelrc.agents build //crates/df-core:df_core
```

```text
ERROR: .../rules_rust+/ffi/cc/allocator_library/BUILD.bazel:21:11: While resolving toolchains for
target @@rules_rust+//ffi/cc/allocator_library:cc_allocator_library: No matching toolchains found
for types: @@bazel_tools//tools/cpp:toolchain_type
```

Every target fails, including crates untouched by the change. Pre-existing, not caused by
`df-agent-channel`.

## Cause

Toolchain resolution is part of analysis, which runs on the client. The remote executor is never
asked, so the executor (NativeLink worker on the RPi5, `gcc`/`ld` provided through `envfs`) is not
the problem.

`.bazelrc` (`--config=remote`, enabled by `.bazelrc.agents`) sets both
`--host_platform` and `--extra_execution_platforms` to `//build/platforms:linux_arm64_rbe`.
Rust rules need a C/C++ toolchain for the exec platform (`cc_allocator_library`, `process_wrapper`,
protobuf tooling). Candidates:

| Toolchain | Registered in | Matches `linux_arm64_rbe`? |
|-----------|---------------|----------------------------|
| `toolchains_musl` | `MODULE.bazel:29-35`, `extra_target_compatible_with = ["//build/platforms:musl"]` | No: the platform has only `os:linux` and `cpu:aarch64`, not `:musl` |
| `@local_config_cc` (autodetected) | implicit | No: matches the macOS host only |

No glibc/LLVM cc toolchain for linux-aarch64 is registered, so resolution finds nothing.

## Attempt: option 1, add `:musl` to `linux_arm64_rbe`

`build/platforms/BUILD.bazel`: add `":musl"` to the `constraint_values` of `linux_arm64_rbe`.

Result: analysis passes, the build reaches the executor, then fails linking
`rules_rust//util/process_wrapper`:

```text
--target=aarch64-unknown-linux-gnu
--codegen=linker=external/toolchains_musl++.../aarch64-linux-musl-gcc
error: linking with `aarch64-linux-musl-gcc` failed ... libstd-*.rlib ... glibc_version
```

A glibc-targeting Rust `std` is linked with the musl gcc. The error is only moved from
analysis to link time. Change reverted.

Why the glibc toolchain is chosen: all `rust.repository_set(...)` entries in `MODULE.bazel` have a
glibc *exec* triple (`aarch64-unknown-linux-gnu`) but a *musl target*. The default
`rust.toolchain(...)` block (`MODULE.bazel:41`) additionally registers a toolchain whose target is
`-gnu` (compatible with plain linux/aarch64, so also with the RBE platform once it carries `:musl`).
Exec-side tools are built with the exec platform as target, and that gnu-target toolchain was picked.
`--toolchain_resolution_debug='@@rules_rust//rust:toolchain_type'` printed nothing useful, so the exact
candidate order was not confirmed.

## Options

1. **Register a glibc cc toolchain for linux-aarch64** (for example `toolchains_llvm`, or a
   non-hermetic one that uses the worker's `/usr/bin/gcc`). Exec tools then link glibc against glibc.
   Leaves the musl toolchains and production image builds untouched. Needs network access to fetch a
   sysroot if hermetic. Recommended first try.
2. **Make exec-side Rust musl** (option 1 plus ensuring the musl-target toolchains win over the
   default gnu one). Needs restructuring of the `rust.toolchain` block (it also provides
   `wasm32-wasip2`) or toolchain ordering. Larger change, affects image builds.

## Reproduce and investigate

```bash
# fails: baseline
bazel --bazelrc=.bazelrc.agents build //crates/df-core:df_core

# which cc toolchains were considered and why they were rejected
bazel --bazelrc=.bazelrc.agents build \
  --toolchain_resolution_debug='@@bazel_tools//tools/cpp:toolchain_type' //crates/df-core:df_core

# does a local (non-remote) build work on the Mac?
bazel build //crates/df-core:df_core
```

## Impact

Remote builds and tests cannot run until this is fixed. Workaround: run Bazel locally without `.bazelrc.agents` (macOS host toolchain; `#[sqlx::test]` targets need PostgreSQL on `localhost:5432`, see `docs/development.md`):

```bash
bazel test //crates/...
bazel build --config=fmt //crates/... //plugins/...
```

For `iter1-run-orchestration` this passed on 2026-10-19 (9/9 test targets, rustfmt check). It found one BUILD error the Cargo checks could not: a `rust_test` with `crate = ...` does not inherit the library's `crate_features`.
