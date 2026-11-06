# Spec Delta

## MODIFIED Requirements

### Requirement: Cargo workspace stays the source of truth
Crates and dependencies SHALL be declared only in Cargo manifests; Bazel SHALL derive third-party dependencies from `Cargo.lock`. Rust code, including generated Rust code, SHALL build with `cargo build` and be resolved by rust-analyzer without Bazel. `cargo build`, `cargo test --workspace` and `cargo clippy --all-targets` SHALL pass without Bazel. Apart from code generators for Rust code (`protoc`), they SHALL NOT need non-Rust tools. Compiled web assets are exempt (see `web-asset-build`).

#### Scenario: Dependency added in Cargo
- **WHEN** a developer adds a dependency to a crate's `Cargo.toml` and repins
- **THEN** the Bazel build uses it without hand-written Bazel dependency entries

#### Scenario: Plain cargo without asset tools
- **WHEN** `cargo test --workspace` runs on a machine with `protoc` but without Bazel and without the web asset tools on `PATH`
- **THEN** the build and every test that does not need compiled web assets pass
