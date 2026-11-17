# Tasks

Code moves with its tests. A new crate follows "Adding a crate" in the `rust` skill. No user- or operator-visible behavior changes, so the published docs (`docs/content/`) do not change. Contributor docs are tasks 5.1 and 5.2.

## 1. `df-sandbox-contract`

- [ ] 1.1 Create `crates/df-sandbox-contract` (`Cargo.toml` with workspace lints, `BUILD.bazel`, `MODULE.bazel` manifest entry, `[workspace.dependencies]` entry, lock files regenerated); verify `cargo xtask check-crates` and `cargo build -p df-sandbox-contract`.
- [ ] 1.2 Move the `SANDBOX_*` constants and `INSTANCE_CA_PATH` (`paths.rs`), `glob.rs` with its tests and `LandlockMode` with its serde attributes; `df-core` depends on the new crate and uses `LandlockMode` as its config field type. Verify `cargo nextest run -p df-sandbox-contract -p df-core` and that the instance config tests still parse `landlock`.
- [ ] 1.3 Switch every caller to `df_sandbox_contract` (`df-core`, `df-db`, `df-gateway`, `df-web`, `df-k8s`, `factory-agent`) with no re-export from `df-core`. Remove `df-core` from `factory-agent/Cargo.toml` and `BUILD.bazel`. Verify `cargo build --workspace`, `cargo machete`, and that `cargo tree -p factory-agent -e normal | grep df-core` finds nothing.

## 2. `df-tls`

- [ ] 2.1 Create `crates/df-tls` (checklist as in 1.1, with a `test-util` feature); verify `cargo xtask check-crates`.
- [ ] 2.2 Move `TlsReloader`, `TlsError`, `mtls_incoming`, `MtlsIncoming` into `reload.rs`, with `load(&TlsPaths)` replacing `load(&AgentTlsConfig)`, and `identity.rs` (`pod_uri_san`). Move the reload and identity tests along. Write a failing test first for any reload behavior that only the channel tests covered. Verify `cargo nextest run -p df-tls`.
- [ ] 2.3 Move `TestPki` and its certificate helpers into `df_tls::testing` (`test-util`); keep the database and runtime-client fixtures in `df_agent_channel::testing`. Verify `cargo nextest run -p df-tls --features test-util`.
- [ ] 2.4 Switch `df-agent-channel` (`server.rs`, `testing.rs`), `dark-factory/src/main.rs` and `df-gateway` (`link.rs`, the tests) to `df-tls`, building `TlsPaths` from `AgentTlsConfig`; verify `cargo nextest run -p df-agent-channel -p df-gateway -p dark-factory`.

## 3. Metric test probe

- [ ] 3.1 Move `Probe` to `df_telemetry::testing` behind `test-util` (with the `opentelemetry_sdk` in-memory exporter it needs); switch `df-agent-channel`, `df-gateway` and `df-web` tests; verify `cargo nextest run -p df-telemetry --features test-util -p df-web webhooks -p df-gateway`.

## 4. Remove the wrong edges

- [ ] 4.1 Replace `df_agent_channel::Metrics` in `df-gateway/src/control/usage_records/tests.rs` with a test-local counter. Remove `df-agent-channel` from `df-gateway` (dependency and dev-dependency) and from `df-web` (dev-dependency), in `Cargo.toml` and `BUILD.bazel`. Verify `cargo tree -p df-gateway | grep df-agent-channel` and `cargo tree -p df-web -e dev | grep df-agent-channel` find nothing, and `cargo nextest run -p df-gateway -p df-web` passes.

## 5. Documentation

- [ ] 5.1 In `docs/architecture.md`, change the rows of `df-sandbox-contract`, `df-tls`, `df-telemetry` (probe) and `factory-agent` from **target** to current, and check the Mermaid graph matches `cargo metadata`. Verify the dependency lists of the touched rows against `cargo metadata --no-deps`.
- [ ] 5.2 Add `df-sandbox-contract` and `df-tls` to the project overview in `AGENTS.md`; verify `cargo xtask check-crates`.

## 6. Integration

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents test //...`; all pass. Report container-backed tests that could not run.

## Workflow follow-up

- At archive: add backlog rows for architecture #4 (split `df-agent-channel` into agent port and `df-orchestrator`) and #5 (`df-gateway-control`) to `product/iterations/roadmap.md`, unless they are already proposed as changes.
