# Tasks

No published docs change: this alters contributor and maintainer tooling only (Bazel toolchains, `.bazelrc`, `.cargo`/`.envrc`, and the CI and release workflows that build the images); the released images, chart and configuration an operator uses stay the same. Contributor and maintainer docs (`docs/development.md`, `docs/releasing.md`, `docs/remote-execution.md`) are updated in the groups below.

## 1. Spike and decision (design D1)

- [x] 1.1 (aarch64 only: no amd64 baseline; ADR 0058) Baseline on the current `main`: record cold and warm `bazel build //...` wall time, first-fetch download size (`bazel clean --expunge` + `--experimental_repository_cache` stats), and `cargo test --workspace --no-run --timings` on aarch64 and amd64 Linux; verify the numbers are in a results table in the spike notes.
- [x] 1.2 (LLVM fails on aarch64 NixOS, not run on amd64; zig passes on both; ADR 0058) Prototype `toolchains_llvm` for the Linux exec platforms (D2) on the spike branch; verify `bazel build //...` and `bazel test //...` (container-backed tests excluded) pass on amd64 and arm64 Linux, record the sysroot glibc version and that a build script and a proc-macro link with the pinned `ld.lld` (`--subcommands`).
- [x] 1.3 (done with zig) Check aarch64 NixOS (`nix-ld`) and a host whose default `ld` is gold; verify the build passes and no gold deprecation warning appears.
- [x] 1.4 (not done: ADR 0058, `toolchains_llvm` was rejected for the exec platforms, so there is nothing to extend to the musl targets) Try `toolchains_llvm` for the musl targets (D3); record whether `//deploy:images_structure_test` passes and how `MODULE.bazel` changes compared to keeping `toolchains_musl`.
- [x] 1.5 Prototype `--config=rbe` against BuildBuddy (D4) from a Linux and a macOS client; record whether the default executor image suffices, whether arm64 executors exist, the RBE wall time and the cache hit rate of the second client.
- [x] 1.6 Measure the CI gate (D6) on the spike branch: the `ci.yaml` `bazel` job with `--config=rbe` vs. `--config=cache`, cold and warm cache, two runs each; and the release jobs' build and test steps with `--config=rbe` from a manual run that stops before pushing. Record pass/fail, median wall times and BuildBuddy minutes in the results table. On an arm64 Linux dev machine, also compare a prototype of `just bazel-test-rbe` (D4) with `just bazel-test`: a one-crate change on a warm cache, and a fresh checkout with a warm remote cache.
- [x] 1.7 Measure the cargo link share (D5) on aarch64 Linux with the default linker vs. lld, and check whether Rust's bundled `rust-lld` is usable on stable for `aarch64-unknown-linux-gnu`; record the numbers against the 10 % threshold.
- [x] 1.8 (LLVM fell short on NixOS and on download size; zig measured instead) Only if 1.2–1.6 show `toolchains_llvm` falls short: repeat the failing checks with `hermetic_cc_toolchain` and record the comparison.
- [x] 1.9 Write `docs/adr/0058-hermetic-c-toolchain.md` (template `docs/adr/template.md`) with the results table, the answers to the six questions of #172 and the decisions on D3, D4, D5 and D6; add it to `docs/adr/README.md`; verify `openspec validate hermetic-cc-toolchain --strict` still passes after adjusting the spec delta to the decisions (remove the cargo requirement if D5 says drop).

## 2. Hermetic exec toolchain (spec: Hermetic C toolchain for Linux exec tools)

- [x] 2.1 (zig, glibc 2.28, no sysroot) Add the chosen toolchain to `MODULE.bazel` (pinned LLVM release and sysroots per arch, checksums), registered after `@musl_toolchains_hub//:all` and before `@rust_toolchains//:all`, with a comment on the order; update `MODULE.bazel.lock`; verify `bazel --bazelrc=.bazelrc.agents build //...` passes.
- [x] 2.2 Verify toolchain resolution: `bazel cquery --toolchain_resolution_debug='@bazel_tools//tools/cpp:toolchain_type'` shows LLVM for the exec platform and `toolchains_musl` for `linux_*_musl`; `//deploy:images_structure_test` passes.
- [x] 2.3 (39 of 42 tests: the other three need Landlock and the host's `readelf`, scenario adjusted) Verify the "Linux host without a C compiler" scenario: `bazel test //...` in a container image without `gcc`/`clang`/binutils (container-backed tests excluded) passes.
- [x] 2.4 (not done: ADR 0058, D3 keeps `toolchains_musl`) If D3 decided to replace `toolchains_musl`: remove it and its registration, verify 2.2 and the static ELF checks again.
- [x] 2.5 Update `docs/development.md` ("Bazel: platforms, cache and images": toolchains bullet, host prerequisites, cache sharing bullet, NixOS fallback via `.bazelrc.user`) and `.agents/skills/rust/SKILL.md` if host prerequisites for agents change; verify the documented commands run as written.

## 3. Pinned linker for plain cargo (spec: Pinned linker for plain cargo; only if D5 says go)

- [x] 3.1 (bundled `rust-lld` through `.cargo/cc-lld.sh`, not `bazel_env`) Implement the option chosen in D5 (`.cargo/config.toml` with bundled `rust-lld`, or `ld.lld` in the `bazel_env` tools of `BUILD.bazel` plus the conditional flags in `.envrc`); verify `cargo test --workspace --no-run` links with lld (with `.bazel_env/bin` on `PATH` for the `bazel_env` option) by `readelf -p .comment` on a test binary from `target/debug/deps`.
- [x] 3.2 (no `bazel_env` option: the fallback is the script's own) Verify `cargo build --workspace` and `cargo clippy --all-targets --all-features -- -D warnings` pass; for the `bazel_env` option also with `.bazel_env/bin` off `PATH` (fallback).
- [x] 3.3 (no `bazel_env` tool added) Document the linker in `docs/development.md` (local setup and `bazel_env` tool list); verify the tool list matches `BUILD.bazel`.

## 4. Remote execution (spec: Remote execution configuration; CI and releases on remote execution)

- [x] 4.1 Add the RBE execution platform(s) to `build/platforms/BUILD.bazel` with `exec_properties` (image pinned by digest) and `build:rbe` to `.bazelrc` with `--platforms=//build/platforms:linux_amd64_musl` (D4); verify `bazel build --config=rbe //...` passes from an amd64 and an arm64 Linux client, `bazel cquery --config=rbe 'deps(//deploy:runtime_image)'` still contains both the amd64 and the arm64 `factory-agent`, and `bazel build //...` without it sends no remote action (BuildBuddy invocation shows local strategy only).
- [x] 4.2 (also the two harness image `:test` targets and `buf`; negative case checked: without the tag on `//crates/df-db:df_db_test` the script exits 1 and lists it) Tag every test target that depends on `//crates/df-test-postgres` with `no-remote-exec` and add the repository check of D4; verify `bazel test --config=rbe //...` on an amd64 Linux client with Docker passes with those tests executed locally (`--execution_log_compact_file` or the BuildBuddy invocation), `bazel test --config=rbe --test_tag_filters=-no-remote-exec //...` passes from macOS, and the check fails for a test target without the tag.
- [x] 4.3 If 1.6 showed the recipe is faster on arm64 Linux: add `bazel-test-rbe *TARGETS="//..."` to the `Justfile` (D4: one invocation on amd64 Linux, remote plus local database tests on arm64 Linux and macOS, socket flags of `bazel-test`); verify on an arm64 Linux machine with Docker that it runs every test once, the database tests locally, and fails if either invocation fails. Otherwise record in the ADR that development stays local.
- [x] 4.4 (not done: ADR 0058, the default executor image is enough) Only if 1.5 showed the default executor image is not enough: add `deploy/images/rbe-executor` and publish it to `ghcr.io/prskr/dark-factory/rbe-executor` from the image workflow; verify its structure test passes in CI.
- [x] 4.5 (macOS needs a case-sensitive output volume, ADR 0058) Verify the cross-client scenario: build the same commit with `--config=rbe` from Linux, then from macOS; the second invocation is fully cached.
- [x] 4.6 If the D6 gate passed (1.6): replace `--config=cache` with `--config=rbe` in `BAZEL_ARGS` of `.github/workflows/ci.yaml` and of both Bazel jobs in `.github/workflows/release.yaml`, with a comment pointing to the ADR and naming the rollback; verify the CI run of the pull request uses remote execution (BuildBuddy invocation link) and passes. If the gate failed, leave both workflows unchanged and remove the "CI and releases" scenarios that no longer apply from the spec delta.
- [x] 4.7 Update `docs/remote-execution.md` (status, §2.2 and §3 to what was built, §4 limitations), `docs/development.md` (`--config=rbe`, API key, `no-remote-exec` for database tests, `just bazel-test-rbe` if added, what CI uses) and `docs/releasing.md` if the release workflow switched; update TD-27 in `docs/tech_debt.md` to the remaining part (container-backed tests on RBE workers, agent sandboxes); verify links with `just docs-build`.

## 5. Integration checks

- [x] 5.1 (`ci.yaml` runs for pull requests and `main` only and no PR exists yet: the temporary spike workflow ran its steps with the final `BAZEL_ARGS`; one flaky test, ADR 0058) Run the CI workflow on the branch with its final `BAZEL_ARGS`; verify all jobs pass, and on a re-run of the same commit that the remote cache hit rate is not lower than on `main`.
- [x] 5.2 (`cargo deny check` needed `rustix` 1.1.6 for RUSTSEC-2026-0486, bumped in its own commit) Run the pre-push hooks (`cargo clippy --all-targets --all-features -- -D warnings`, `cargo deny check`) and `cargo-machete`; verify no findings.

## Workflow follow-up

- On archive: add the backlog row "Container-backed tests on RBE workers (`dockerd` in the executor), so they no longer run on the GitHub runner" to `product/iterations/roadmap.md`, referencing TD-27 and the archived change.
- The first real release after the switch is the end-to-end check of the push step under `--config=rbe` (D6 step 3 stops before pushing); watch it.
- Comment the ADR and the results table on issue #172 when the user asks for it.
