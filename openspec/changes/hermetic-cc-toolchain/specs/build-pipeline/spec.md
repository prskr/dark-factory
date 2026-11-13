# Spec Delta

## ADDED Requirements

### Requirement: Hermetic C toolchain for Linux exec tools
On Linux amd64 and arm64 hosts, Bazel SHALL compile and link exec tools (build scripts, proc-macros, other host tools) with a C toolchain that the module pins by version and checksum, including a pinned glibc sysroot. The C compiler, linker and libraries of the host SHALL NOT be used for them. macOS hosts MAY keep the host's C toolchain for local exec tools.

#### Scenario: Linux host without a C compiler
- **WHEN** `bazel test //...` runs on a Linux host (amd64 or arm64) that has no `gcc`, `cc`, `clang` or binutils on `PATH`
- **THEN** the build and every test that does not need a container runtime pass

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
The repository SHALL provide a Bazel configuration `rbe` that runs build and test actions on BuildBuddy remote execution with a Linux execution platform. With it, no action SHALL depend on the client's C toolchain, and the client MAY be Linux or macOS. Tests that need a container runtime SHALL run on the client. Without `--config=rbe`, builds SHALL keep running on the local machine.

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
- **WHEN** `bazel test --config=rbe //...` runs on a client with a container runtime
- **THEN** the tests that depend on `df-test-postgres` run on the client and pass, and every other test runs on the remote workers

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
When the pinned `ld.lld` is on `PATH` (through `bazel_env`), plain `cargo` builds for the `*-unknown-linux-gnu` host targets SHALL link with it. When it is not on `PATH`, `cargo` SHALL fall back to the default linker and `cargo build --workspace` SHALL still pass. If the link share measured in the spike is too small to matter (design D5), this requirement SHALL be removed from the change before archiving.

#### Scenario: Linker from bazel_env
- **WHEN** `cargo test --workspace --no-run` runs on an aarch64 Linux host with `.bazel_env/bin` on `PATH`
- **THEN** the test binaries are linked by the pinned `ld.lld`

#### Scenario: Without bazel_env
- **WHEN** `cargo build --workspace` runs on a Linux host without `.bazel_env/bin` on `PATH`
- **THEN** the build passes with the default linker
