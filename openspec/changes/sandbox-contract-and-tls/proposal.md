# Proposal

## Why

Two crates depend on others only to reuse a small piece of shared infrastructure:

- `factory-agent`, the sandbox runtime, depends on all of `df-core` (control-plane config, routing, `minijinja`, `secrecy`, …) for 8 path constants, the path-glob helpers and `LandlockMode`.
- `df-gateway` depends on `df-agent-channel`, the orchestration crate with its database access, only in its tests: the TLS reloader, the test PKI, the metric test probe and `Metrics`. `df-web` does the same for the probe.

Both break the boundary rules of `docs/architecture.md` (rule 4, infrastructure in infrastructure crates; rule 5, the sandbox runtime stays small).

## What Changes

- New crate `df-sandbox-contract`: what control plane and sandbox share about the sandbox filesystem. It contains the `SANDBOX_*` path constants and `INSTANCE_CA_PATH` (from `df-core/src/harness.rs`), the path globs (`df-core/src/glob.rs`) and `LandlockMode` (from `df-core/src/config.rs`). Callers use it directly; `df-core` depends on it.
- `factory-agent` drops its `df-core` dependency.
- New crate `df-tls`: `TlsReloader`, `TlsError`, `mtls_incoming`, `MtlsIncoming` (from `df-agent-channel/src/server.rs`) and the peer identity extraction (`df-agent-channel/src/identity.rs`). It also has `TestPki` behind a `test-util` feature. `TlsReloader::load` takes a `df-tls` paths struct instead of `df_core::config::AgentTlsConfig`.
- The metric test `Probe` moves from `df-agent-channel` (`metrics::probe`) to `df-telemetry` (`test-util`).
- `df-gateway` loses its dependency on `df-agent-channel`, and `df-web` loses its dev-dependency on it. The gateway tests that wrapped the channel's `Metrics` use a local counter.
- `docs/architecture.md`: the rows of `df-sandbox-contract`, `df-tls`, `df-telemetry` (probe) and `factory-agent` change from **target** to current.
- No change in behavior, configuration or wire format.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Pure refactor: `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- Splitting `df-agent-channel` into agent port and orchestration (architecture #4) and moving the gateway's control-plane side out of `df-gateway` (#5): follow-up changes, planned in `docs/architecture.md`. They are added to the backlog of `product/iterations/roadmap.md` at archive.
- A separate `df-config` crate. Rejected (`docs/architecture.md`, rule 7).
- Splitting `df-k8s`. Rejected for now: it waits for a second sandbox backend (backlog: cloud VM sandboxes).

## Impact

- New crates: `crates/df-sandbox-contract`, `crates/df-tls`, each with `BUILD.bazel`, a manifest entry in `MODULE.bazel` and the `[workspace.dependencies]` entry.
- Changed: `df-core` (`harness.rs`, `glob.rs`, `config.rs`), `df-agent-channel` (`server.rs`, `identity.rs`, `metrics.rs`, `testing.rs`), `df-telemetry`, `df-gateway`, `df-web`, `df-db`, `factory-agent`, `dark-factory` (imports only).
- Contributor docs: `docs/architecture.md`, and `AGENTS.md` (project overview list of crates). No published docs change.
- Dependencies: no new external crates. Existing ones (`globset`, `serde`, `rustls`, `tokio-rustls`, `x509-parser`, `rcgen`, `opentelemetry_sdk`) move to the new crates.
- Order: independent of `db-write-ownership`; `gateway-control-crate` builds on `df-tls` from this change.
