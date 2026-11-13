# Tasks

No published docs change: this alters contributor tooling only (Bazel toolchains, `.bazelrc`, `.cargo`/`.envrc`); nothing a user or operator sees or configures. Contributor docs are updated in the groups below.

## 1. Spike and decision (design D1)

- [ ] 1.1 Baseline on the current `main`: record cold and warm `bazel build //...` wall time, first-fetch download size (`bazel clean --expunge` + `--experimental_repository_cache` stats), and `cargo test --workspace --no-run --timings` on aarch64 and amd64 Linux; verify the numbers are in a results table in the spike notes.
- [ ] 1.2 Prototype `toolchains_llvm` for the Linux exec platforms (D2) on the spike branch; verify `bazel build //...` and `bazel test //...` (container-backed tests excluded) pass on amd64 and arm64 Linux, record the sysroot glibc version and that a build script and a proc-macro link with the pinned `ld.lld` (`--subcommands`).
- [ ] 1.3 Check aarch64 NixOS (`nix-ld`) and a host whose default `ld` is gold; verify the build passes and no gold deprecation warning appears.
- [ ] 1.4 Try `toolchains_llvm` for the musl targets (D3); record whether `//deploy:images_structure_test` passes and how `MODULE.bazel` changes compared to keeping `toolchains_musl`.
- [ ] 1.5 Prototype `--config=rbe` against BuildBuddy (D4) from a Linux and a macOS client; record whether the default executor image suffices, whether arm64 executors exist, the RBE wall time and the cache hit rate of the second client.
- [ ] 1.6 Measure the cargo link share (D5) on aarch64 Linux with the default linker vs. lld, and check whether Rust's bundled `rust-lld` is usable on stable for `aarch64-unknown-linux-gnu`; record the numbers against the 10 % threshold.
- [ ] 1.7 Only if 1.2–1.5 show `toolchains_llvm` falls short: repeat the failing checks with `hermetic_cc_toolchain` and record the comparison.
- [ ] 1.8 Write `docs/adr/0058-hermetic-c-toolchain.md` (template `docs/adr/template.md`) with the results table, the answers to the six questions of #172 and the decisions on D3, D4 and D5; add it to `docs/adr/README.md`; verify `openspec validate hermetic-cc-toolchain --strict` still passes after adjusting the spec delta to the decisions (remove the cargo requirement if D5 says drop).

## 2. Hermetic exec toolchain (spec: Hermetic C toolchain for Linux exec tools)

- [ ] 2.1 Add the chosen toolchain to `MODULE.bazel` (pinned LLVM release and sysroots per arch, checksums), registered after `@musl_toolchains_hub//:all` and before `@rust_toolchains//:all`, with a comment on the order; update `MODULE.bazel.lock`; verify `bazel --bazelrc=.bazelrc.agents build //...` passes.
- [ ] 2.2 Verify toolchain resolution: `bazel cquery --toolchain_resolution_debug='@bazel_tools//tools/cpp:toolchain_type'` shows LLVM for the exec platform and `toolchains_musl` for `linux_*_musl`; `//deploy:images_structure_test` passes.
- [ ] 2.3 Verify the "Linux host without a C compiler" scenario: `bazel test //...` in a container image without `gcc`/`clang`/binutils (container-backed tests excluded) passes.
- [ ] 2.4 If D3 decided to replace `toolchains_musl`: remove it and its registration, verify 2.2 and the static ELF checks again.
- [ ] 2.5 Update `docs/development.md` ("Bazel: platforms, cache and images": toolchains bullet, host prerequisites, cache sharing bullet, NixOS fallback via `.bazelrc.user`) and `.agents/skills/rust/SKILL.md` if host prerequisites for agents change; verify the documented commands run as written.

## 3. Pinned linker for plain cargo (spec: Pinned linker for plain cargo; only if D5 says go)

- [ ] 3.1 Implement the option chosen in D5 (`.cargo/config.toml` with bundled `rust-lld`, or `ld.lld` in the `bazel_env` tools of `BUILD.bazel` plus the conditional flags in `.envrc`); verify `cargo test --workspace --no-run` links with lld when `.bazel_env/bin` is on `PATH` (`-Clink-arg=-Wl,--version` or `readelf -p .comment`).
- [ ] 3.2 Verify the fallback: `cargo build --workspace` passes with `.bazel_env/bin` off `PATH`, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] 3.3 Document the linker in `docs/development.md` (local setup and `bazel_env` tool list); verify the tool list matches `BUILD.bazel`.

## 4. Opt-in remote execution (spec: Opt-in remote execution)

- [ ] 4.1 Add the RBE execution platform(s) to `build/platforms/BUILD.bazel` with `exec_properties` (image pinned by digest) and `build:rbe` to `.bazelrc` (D4); verify `bazel build --config=rbe //...` passes from a Linux client and `bazel build //...` without it sends no remote action (BuildBuddy invocation shows local strategy only).
- [ ] 4.2 Tag container-backed test targets `requires-container` and filter them under `build:rbe`; verify `bazel test --config=rbe //...` passes and `bazel query 'attr(tags, requires-container, tests(//...))'` lists every target depending on `//crates/df-test-postgres`.
- [ ] 4.3 Only if 1.5 showed the default executor image is not enough: add `deploy/images/rbe-executor` and publish it to `ghcr.io/prskr/dark-factory/rbe-executor` from the image workflow; verify its structure test passes in CI.
- [ ] 4.4 Verify the cross-client scenario: build the same commit with `--config=rbe` from Linux, then from macOS; the second invocation is fully cached.
- [ ] 4.5 Update `docs/remote-execution.md` (status, §2.2 and §3 to what was built, §4 limitations) and `docs/development.md` (`--config=rbe`, API key); update TD-27 in `docs/tech_debt.md` to the remaining part (container-backed tests on RBE, agent sandboxes); verify links with `just docs-build`.

## 5. Integration checks

- [ ] 5.1 Run the CI workflow on the branch with the unchanged `--config=cache --config=linux-amd64-musl` arguments; verify all jobs pass and the remote cache hit rate on a re-run is not lower than on `main`.
- [ ] 5.2 Run the pre-push hooks (`cargo clippy --all-targets --all-features -- -D warnings`, `cargo deny check`) and `cargo-machete`; verify no findings.

## Workflow follow-up

- On archive: add the backlog row "Container-backed tests on RBE workers (`dockerd` in the executor); then `--config=rbe` as CI default" to `product/iterations/roadmap.md`, referencing TD-27 and the archived change.
- Comment the ADR and the results table on issue #172 when the user asks for it.
