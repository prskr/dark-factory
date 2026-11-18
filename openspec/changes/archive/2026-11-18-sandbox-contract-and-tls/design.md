# Design

## Context

See proposal.md for motivation. Current state:

| Item | Today | Used by |
|---|---|---|
| `SANDBOX_DATA_DIR`, `SANDBOX_REPO_DIR`, `SANDBOX_HOME_DIR`, `SANDBOX_RUN_DIR`, `SANDBOX_RUNTIME_SOCK`, `SANDBOX_TLS_DIR`, `SANDBOX_CA_DIR`, `INSTANCE_CA_PATH` | `df-core/src/harness.rs` (a module that otherwise holds harness profiles) | `df-core`, `df-k8s`, `factory-agent` |
| `protected_glob`, `hidden_glob`, `hidden_glob_set`, `sparse_patterns` | `df-core/src/glob.rs` (`globset`) | `df-core`, `df-db`, `df-gateway`, `df-web`, `factory-agent` |
| `LandlockMode` | `df-core/src/config.rs` (serde, field of the instance config) | `df-core`, `df-k8s`, `factory-agent` |
| `TlsReloader`, `TlsError`, `mtls_incoming`, `MtlsIncoming` | `df-agent-channel/src/server.rs`, taking `df_core::config::AgentTlsConfig` | `df-agent-channel`, `dark-factory`; `df-gateway` tests |
| `pod_uri_san` | `df-agent-channel/src/identity.rs` (`x509-parser`) | `df-agent-channel` server |
| `TestPki` | `df-agent-channel/src/testing.rs` (`test-util`, next to database fixtures) | channel and gateway tests |
| `metrics::probe::Probe` | `df-agent-channel/src/metrics.rs` (`test-util`) | channel, gateway and web tests |

`df-gateway` production code does not use `df-agent-channel`: the dependency is only reached from tests (`control/tests.rs`, `control/usage_records/tests.rs`, `proxy/tests.rs`, `inference/tests.rs`, `gateway/tests.rs`, `peer.rs` test module).

## Goals / Non-Goals

**Goals:**
- `factory-agent` has no `df-core` dependency; `df-gateway` and `df-web` have no `df-agent-channel` dependency, not even a dev-dependency.
- Code moves without changes in behavior. Only imports and the TLS load signature change.

**Non-Goals:**
- Architecture #4 and #5: follow-up changes (proposal, Out of Scope).
- Moving the harness profiles (`harness.rs` minus the constants) out of `df-core`. Rejected: they are domain rules.

## Decisions

### 1. Callers use the new crates directly, no re-exports from `df-core`

`df-core` stops exporting `harness::SANDBOX_*`, `glob` and `LandlockMode`; every caller imports from `df_sandbox_contract` (`paths`, `glob`, `LandlockMode`). This is pre-MVP, so the break is cheap, and every use states which crate owns the item.

Alternative: re-export from `df-core` for compatibility. Rejected: two paths to the same item, and new code would keep reaching for `df-core`.

### 2. `df-sandbox-contract` layout

- `paths.rs`: the constants, with their doc comments.
- `glob.rs`: moved as is, with its tests.
- `LandlockMode` in `lib.rs`, with the serde attributes it has today. `df_core::config` uses it as a field type, so the instance config format does not change.

Dependencies: `globset`, `serde`, `thiserror` (if `glob` errors need it). No workspace dependency.

### 3. `df-tls` takes a paths struct, not the config type

`df-tls` must not depend on `df-core` (`docs/architecture.md`). `TlsReloader::load(&TlsPaths)` takes `TlsPaths { cert_file, key_file, client_ca_file }`, a plain struct without serde. `AgentTlsConfig` stays in `df_core::config` for parsing. Callers (`dark-factory`, `df-gateway/src/link.rs`, the channel tests) build `TlsPaths` from it in one line.

Alternative: `df-core` depends on `df-tls` and `AgentTlsConfig` moves there. Rejected: domain types would depend on TLS infrastructure.

Contents:
- `reload.rs`: `TlsReloader`, `TlsError`, `mtls_incoming`, `MtlsIncoming` (the TLS part of `server.rs`). The tonic service and the identity binding to runs stay in `df-agent-channel`.
- `identity.rs`: `pod_uri_san` and its error, moved as is.
- `testing.rs` (`test-util`): `TestPki` and its certificate helpers. The database and runtime-client fixtures stay in `df-agent-channel::testing`.

### 4. `Probe` in `df-telemetry`

`Probe` moves to `df_telemetry::testing::Probe` behind the existing `test-util` feature, which then also enables the `opentelemetry_sdk` in-memory exporter it needs. Its users switch imports. The gateway tests that wrap `df_agent_channel::Metrics` in a `Counter` use a counter defined in the test module instead: they check the gateway's call, not the channel's metrics.

## Risks / Trade-offs

- [The `test-util` feature of `df-telemetry` now pulls OpenTelemetry for tests of crates without `otel`] → Test-only cost. `df-gateway` and `df-agent-channel` already build it.
- [Splitting `server.rs` leaves the reload logic and the tonic server in two crates] → The boundary is clear: `df-tls` hands out a stream of TLS connections, the channel serves gRPC on it.
- [Two new crates for little code] → Each removes a wrong dependency edge. A crate is the only boundary cargo enforces.

## Migration Plan

Code-only refactor. No configuration, Helm or wire change. Rollback is a revert.
