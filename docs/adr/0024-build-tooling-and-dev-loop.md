# ADR-0024: Build tooling and development loop

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The author uses Bazel across projects, including a single target that builds, packages, pushes and deploys an operator. Tilt was tried and didn't add value. Development happens on an arm64 Mac against amd64 cluster nodes.

## Decision

- The Cargo workspace is the source of truth for crates and dependencies (rust-analyzer, `cargo test`, `cargo deny` keep working).
- Bazel builds, packages and deploys: rules_rust with crate_universe generated from the Cargo workspace, rules_oci for images, cross-compilation to static amd64 musl binaries, one target to build, push to Harbor and deploy to the dev namespace with the Helm chart (`helm upgrade --install` with the new image digests, ADR-0031). No Tilt.
- Development against a dev instance in a dedicated homelab namespace; telepresence is an option later.
- Tests use a fake ACP agent (scripted, built on `agent-client-protocol`) so end-to-end tests run without Claude and without using the subscription.

## Consequences

- Points to watch: crates with C/asm build scripts (prefer `ring` over `aws-lc-rs` for rustls), `sqlx` offline mode (`.sqlx` data checked in), the `migrations/` directory and `rust-embed` assets as compile data, protoc for tonic/prost, wasm32-wasip2 builds for plugins.

## Alternatives considered

- **Cargo-only tooling with Tilt/skaffold**: tried before without added value.
- **Local kind/k3d clusters for development**: a second copy of the environment; kept as a later option for CI integration tests.
