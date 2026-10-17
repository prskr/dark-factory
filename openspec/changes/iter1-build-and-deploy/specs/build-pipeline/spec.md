# Spec Delta

## Purpose

Defines how Dark Factory's binaries, plugin components and images are built from one source of truth, and how a developer deploys a dev instance in one step.

## ADDED Requirements

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
