# build-pipeline Specification

## Purpose

Defines how Dark Factory's binaries, plugin components and images are built from one source of truth, and how a developer deploys a dev instance in one step.

## Requirements

### Requirement: Cargo workspace stays the source of truth
Crates and dependencies SHALL be declared only in Cargo manifests; Bazel SHALL derive third-party dependencies from `Cargo.lock`. `cargo build`, `cargo test` and rust-analyzer SHALL keep working without Bazel.

#### Scenario: Dependency added in Cargo
- **WHEN** a developer adds a dependency to a crate's `Cargo.toml` and repins
- **THEN** the Bazel build uses it without hand-written Bazel dependency entries

### Requirement: Static amd64 binaries from macOS
`dark-factory`, `factory-agent` and `fake-acp-agent` SHALL build as statically linked `x86_64-unknown-linux-musl` executables on arm64 macOS and on amd64 Linux.

#### Scenario: Runtime binary is static
- **WHEN** `factory-agent` is built for the image
- **THEN** `file` reports a statically linked x86-64 ELF executable

### Requirement: Plugin components
The Forgejo and GitHub plugins SHALL build as WebAssembly components for `wasm32-wasip2` that validate against world `darkfactory:integrations/forge@0.1.0`.

#### Scenario: Component targets the forge world
- **WHEN** a plugin target is built
- **THEN** the host reports the capabilities `pull-requests` and `issues` when loading it

### Requirement: Images
The build SHALL produce: the control plane image (only the `dark-factory` binary, no plugin components), one image per plugin (only `/plugin.wasm`, an optimized build of the component, usable as an image volume, with an image index for `linux/amd64` and `linux/arm64`), the runtime image (only `factory-agent`, `hooks-empty/` and the read-only `gitconfig`, usable as an image volume at `/opt/factory`) and a test project image (git, shell, glibc, `fake-acp-agent`).

#### Scenario: Runtime image content
- **WHEN** the runtime image is inspected
- **THEN** it contains `/factory-agent`, an empty `/hooks-empty/` directory and `/gitconfig`, and nothing else

#### Scenario: Plugin image content
- **WHEN** the Forgejo or the GitHub plugin image is inspected
- **THEN** it contains `/plugin.wasm` and nothing else, the file is smaller than 1 MiB and the host reports capabilities `pull-requests` and `issues` when loading it

#### Scenario: Control plane image without plugins
- **WHEN** the control plane image is inspected
- **THEN** it contains no `.wasm` file

### Requirement: One-step dev deploy
A single Bazel target SHALL build all images, including the plugin images, push them to the configured registry, and run `helm upgrade --install` of the chart into the dev namespace with the pushed image digests.

#### Scenario: Deploy after a code change
- **WHEN** a developer runs the deploy target after changing control plane code
- **THEN** the control plane Deployment rolls out with the new image digest and the `migrate` Job runs first

#### Scenario: Deploy after a plugin change
- **WHEN** a developer runs the deploy target after changing `plugins/forgejo`
- **THEN** the release references the new Forgejo plugin image digest and the control plane pods are replaced

### Requirement: Claude Code harness image
The repository SHALL provide a harness image for the `claude-code` harness with no base (`scratch`) that, used as an image volume, holds an executable adapter launcher at `bin/claude-agent-acp`, the ACP adapter `@agentclientprotocol/claude-agent-acp`, the Node.js runtime it needs and the Claude Code CLI, so that the adapter runs in a project image without Node.js.

#### Scenario: Adapter from the volume
- **WHEN** the image is mounted at `/opt/harness/claude-code` in a project image that has no Node.js
- **THEN** `/opt/harness/claude-code/bin/claude-agent-acp` completes an ACP `initialize` handshake

#### Scenario: Alpine and glibc project images
- **WHEN** the image is mounted in an Alpine and in a Debian project image
- **THEN** the handshake succeeds in both

#### Scenario: Glibc fallback
- **WHEN** the static-musl approach was rejected in the spike (design D2)
- **THEN** the image is built against glibc, and the reference documentation names glibc as a requirement of project images

### Requirement: Reference Claude Code project image
The repository SHALL provide a reference project image with git, bash, curl, CA certificates and OpenSSH client that runs as UID 1000 with `/workspace` as working directory, and contains neither Node.js nor the ACP adapter.

#### Scenario: Non-root user
- **WHEN** the image is inspected
- **THEN** its configured user is UID 1000

#### Scenario: No harness inside
- **WHEN** the image is inspected
- **THEN** `node` and `claude-agent-acp` are absent

### Requirement: Reference images are checked and published
CI SHALL build the harness image and the reference project image and run their structure checks for every pull request that changes either; a release SHALL push both for `linux/amd64` and `linux/arm64` to `ghcr.io/prskr/dark-factory/claude-harness` (tagged with the adapter version) and `ghcr.io/prskr/dark-factory/claude-project`.

#### Scenario: Broken harness image in a pull request
- **WHEN** a pull request changes the Dockerfile so that `bin/claude-agent-acp` is missing or not executable
- **THEN** the CI job fails

#### Scenario: Release
- **WHEN** a release tag is pushed
- **THEN** `claude-harness:<adapter version>` and `claude-project:<release tag>` exist for amd64 and arm64

### Requirement: Lint rules have one source
Lint levels SHALL be declared only in the Cargo manifests (`[workspace.lints]` and the crates' `[lints]`) and in `clippy.toml`. The Bazel lint run SHALL apply the same lints as `cargo clippy --all-targets --all-features` to every Rust library, binary and test of the workspace, including integration tests under `tests/`. Code outside the Cargo workspace (`tools/xtask`) SHALL be linted by the pre-push hook.

#### Scenario: Lint added in the workspace manifest
- **WHEN** a lint is set to `deny` in `[workspace.lints.clippy]` and code violates it
- **THEN** both `cargo clippy --all-targets --all-features` and `bazel build --config=lint //crates/... //plugins/...` fail, without any change to `.bazelrc`

#### Scenario: Forbidden unsafe code under Bazel
- **WHEN** a crate contains an `unsafe` block
- **THEN** the Bazel build of that crate fails

#### Scenario: Integration test without Bazel target
- **WHEN** a file `tests/<name>.rs` is added to a crate without a Bazel test target
- **THEN** the repository check fails and names the file

#### Scenario: Crate without workspace lints
- **WHEN** the `Cargo.toml` of a member under `crates/` does not inherit the workspace lints, or a Rust target in a member's `BUILD.bazel` has no lint configuration
- **THEN** the repository check fails and names the crate or target

#### Scenario: Plugin lints differ from the workspace
- **WHEN** a plugin's `[lints.rust]` or `[lints.clippy]` lacks a workspace lint or sets it to a different level (`unsafe_code` only has to be present)
- **THEN** the repository check fails and names the plugin and the lint

### Requirement: File length check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail for a source file longer than 1,500 lines unless the file is on an allowlist in the root `Cargo.toml` with a reason. An allowlist entry for a missing file or a file within the limit SHALL fail the check.

#### Scenario: Oversized file
- **WHEN** a tracked source file grows past 1,500 lines and is not on the allowlist
- **THEN** the check fails and prints the file and its line count

#### Scenario: Stale allowlist entry
- **WHEN** an allowlisted file shrinks to 1,500 lines or fewer
- **THEN** the check fails and asks to remove the entry

### Requirement: Test name check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail for a test whose name contains a review finding, review round or tech-debt ID as a `_`-separated word: `finding`, `r<n>`, `round<n>` or `td<n>`. The check is a heuristic; whether a name describes behavior is left to review.

#### Scenario: Test named after a review finding
- **WHEN** a test function is named `test_review_r12_…`, `test_finding_3_…` or `test_td41_…`
- **THEN** the check fails and names the test
