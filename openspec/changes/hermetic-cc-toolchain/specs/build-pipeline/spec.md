# Spec Delta

## ADDED Requirements

### Requirement: Hermetic C toolchain for Linux exec tools
On Linux amd64 and arm64 hosts, Bazel SHALL compile and link exec tools (build scripts, proc-macros) with a C toolchain that the module pins by version and checksum, that brings its own libc and that needs no shared library of the host (ADR-0058). The compiler, linker and libraries of the host SHALL NOT be used for them. macOS hosts MAY keep the host's toolchain. Tested and shipped Linux artifacts remain static musl.

#### Scenario: Linux host without a C compiler
- **WHEN** `bazel test //...` runs on a Linux host (amd64 or arm64) that has no `gcc`, `cc`, `clang` or binutils on `PATH`
- **THEN** the build passes and every test passes that needs neither a container runtime, nor Landlock, nor the host's `file` and `readelf` (`//deploy:images_structure_test` calls `file`; `//deploy/images/claude-harness:structure_test` and `//deploy/images/opencode-harness:structure_test` call `file` and `readelf`)

#### Scenario: Host with the gold linker
- **WHEN** `bazel build //...` runs on a Linux host whose default `ld` is gold
- **THEN** no action prints the gold linker deprecation warning

#### Scenario: Shared cache between different Linux hosts
- **WHEN** two Linux hosts of the same arch with different system compilers build the same commit with the same remote cache
- **THEN** the second build reuses the exec tool actions of the first from the cache

#### Scenario: Static targets unchanged
- **WHEN** `factory-agent` is built for the runtime image after the toolchain change
- **THEN** `file` still reports a statically linked ELF executable and `//deploy:images_structure_test` passes

### Requirement: Remote execution configuration
The repository SHALL provide a Bazel configuration `rbe` that runs build and test actions on BuildBuddy remote execution with a Linux execution platform, targeting the workers' arch. With it, no action SHALL depend on the client's C toolchain, and the client MAY be Linux or macOS. Tests that need a container runtime SHALL run on the client; clients that cannot run the target binaries exclude them. Without `--config=rbe`, builds SHALL keep running locally.

#### Scenario: Build from a macOS client
- **WHEN** `bazel build --config=rbe //...` runs on an arm64 macOS client with a valid BuildBuddy API key
- **THEN** the build succeeds and the exec tools are built and run on the remote Linux workers

#### Scenario: Same results from different clients
- **WHEN** the same commit is built with `--config=rbe` from a Linux client and then from a macOS client
- **THEN** the second build is fully served from the cache

#### Scenario: Default stays local
- **WHEN** `bazel build //...` runs without `--config=rbe`
- **THEN** no action is sent to a remote executor

#### Scenario: Container-backed tests on the client
- **WHEN** `bazel test --config=rbe //...` runs on an amd64 Linux client with a container runtime
- **THEN** the tests tagged `no-remote-exec` (every test that depends on `df-test-postgres`, the `:test` targets of the two harness images, which need a container runtime, and `//contract:buf_lint_test`) run on the client and pass, and every other test runs on the remote workers

#### Scenario: Tests from an arm64 or macOS client
- **WHEN** `bazel test --config=rbe-cross //...` (`rbe` plus the exclusion of the tag `no-remote-exec` from build and test) runs on an arm64 Linux or a macOS client
- **THEN** the test binaries are built for amd64 Linux and every selected test runs and passes on the remote workers

#### Scenario: One command for all tests on an arm64 Linux client
- **WHEN** a developer runs `just bazel-test-rbe` on an arm64 Linux machine with a container runtime, and the spike showed the recipe is faster than `just bazel-test` there (design D4)
- **THEN** every test, including the database tests, runs once and passes, without the developer passing any tag filter

### Requirement: CI and releases on remote execution
The Bazel jobs of the CI workflow and of the release workflow SHALL use the `rbe` configuration, provided the spike showed that both workflows pass with it and that the CI job is not slower than with the remote cache alone (design D6). Otherwise they SHALL keep the remote cache configuration.

#### Scenario: Pull request build
- **WHEN** the CI workflow runs for a pull request
- **THEN** its Bazel job builds and tests `//...` with `--config=rbe`, and the database tests run on the GitHub runner

#### Scenario: Release
- **WHEN** a release is started
- **THEN** its image jobs build, test and push the images with `--config=rbe`

#### Scenario: Remote execution rejected in the spike
- **WHEN** the spike showed a failing workflow or a slower CI job with `--config=rbe`
- **THEN** both workflows keep `--config=cache`, and the ADR records the measurements

### Requirement: Pinned linker for plain cargo
Plain `cargo` builds for the `aarch64-unknown-linux-gnu` host target SHALL link with lld: the `rust-lld` that ships with the pinned Rust toolchain, selected through the linker driver `.cargo/cc-lld.sh` (design D5, ADR-0058; `x86_64-unknown-linux-gnu` already links with it by default). The driver SHALL fall back to plain `cc` when the toolchain has no `rust-lld`. `cargo build --workspace` SHALL pass in both cases.

#### Scenario: Linking with lld
- **WHEN** `cargo test --workspace --no-run` runs on an aarch64 Linux host
- **THEN** the test binaries are linked by lld (`readelf -p .comment` names LLD)

#### Scenario: rust-lld missing
- **WHEN** the Rust toolchain has no `rust-lld` and `cargo build --workspace` runs on an aarch64 Linux host
- **THEN** the build passes with the default linker
