# Tasks

Requires `sandbox-contract-and-tls` (the moved tests use `df-tls` and the `df-telemetry` probe). Code moves with its tests; a new crate follows "Adding a crate" in the `rust` skill. No user- or operator-visible behavior changes, so the published docs (`docs/content/`) do not change. Contributor docs are task 3.1. Database tests need PostgreSQL; report the ones that could not run.

## 1. New crate

- [x] 1.1 Create `crates/df-gateway-control` (`Cargo.toml` with workspace lints and the dependencies of `control/`, `BUILD.bazel` with library and unit-test targets, `no-remote-exec` on the database tests, `MODULE.bazel` manifest entry, `[workspace.dependencies]` entry, lock files regenerated); verify `cargo xtask check-crates`.
- [x] 1.2 Move `df-gateway/src/control/` into the new crate (`mod.rs` → `lib.rs`, the modules, `tests.rs`, `relay/`, `usage_records/`), rewriting `crate::control::` to `crate::` and `crate::{convert, flow, link, peer, tail}` to `df_gateway::`. Give `df-gateway` a `test-util` feature that exposes `link::PlainConnector` as `pub`, together with its `GrpcConnector` impl (design decision 3) and add it as a dev-dependency feature in the new crate. Make the module `df_gateway::convert` and the functions `wire_api_to_proto`, `scheme_to_proto`, `wire_api_from_proto` `pub`, with a doc comment naming the link as their user. Verify `cargo nextest run -p df-gateway-control` runs the same number of tests that `cargo nextest run -p df-gateway -E 'test(/^control::/)'` ran before the move (module-scoped: a plain `control` filter also matches tests such as `git::policy_checks::tests::control_characters_…`).

## 2. Cut the edge

- [x] 2.1 Remove `pub mod control` from `df-gateway`, switch `dark-factory` to `df_gateway_control`, and add the dependency; verify `cargo nextest run -p dark-factory`.
- [x] 2.2 Remove `df-db`, `sqlx` and `df-test-postgres` from `df-gateway` (`Cargo.toml`, `BUILD.bazel`). Move any test outside `control/` that still needs them into `df-gateway-control`. Verify `cargo build -p df-gateway --tests`, `cargo machete`, and that `cargo tree -p df-gateway -e normal,dev | grep -E "df-db|sqlx"` finds nothing.

## 3. Documentation and integration

- [x] 3.1 In `docs/architecture.md`, change the rows of `df-gateway` and `df-gateway-control` from **target** to current and check the graph against `cargo metadata --no-deps`. Add `df-gateway-control` to the project overview in `AGENTS.md`. Verify `cargo xtask check-crates`.
- [x] 3.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents test //crates/df-gateway/... //crates/df-gateway-control/... //crates/dark-factory/...`; all pass.
