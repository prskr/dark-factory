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
The Forgejo plugin SHALL build as a WebAssembly component for `wasm32-wasip2` that validates against world `darkfactory:integrations/forge@0.1.0`.

#### Scenario: Component targets the forge world
- **WHEN** the plugin target is built
- **THEN** `wasm-tools component wit` on the output shows exports `pull-requests` and `issues`

### Requirement: Images
The build SHALL produce: the control plane image (`dark-factory` binary and the Forgejo plugin component), the runtime image (only `factory-agent`, `hooks-empty/` and the read-only `gitconfig`, usable as an image volume at `/opt/factory`) and a test project image (git, shell, glibc, `fake-acp-agent`).

#### Scenario: Runtime image content
- **WHEN** the runtime image is inspected
- **THEN** it contains `/factory-agent`, an empty `/hooks-empty/` directory and `/gitconfig`, and nothing else

### Requirement: One-step dev deploy
A single Bazel target SHALL build all images, push them to the configured registry, and run `helm upgrade --install` of the chart into the dev namespace with the pushed image digests.

#### Scenario: Deploy after a code change
- **WHEN** a developer runs the deploy target after changing control plane code
- **THEN** the control plane Deployment rolls out with the new image digest and the `migrate` Job runs first

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
