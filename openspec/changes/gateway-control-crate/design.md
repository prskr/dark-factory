# Design

## Context

See proposal.md for motivation. Coupling of `df-gateway/src/control/` today:

| Direction | Production uses |
|---|---|
| `control` → rest of `df-gateway` | `convert` (`inference_routes.rs`, `usage_records.rs`), `peer::require_dns_san` (`internal.rs`), `flow` and `link` (`relay.rs`) |
| rest of `df-gateway` → `control` | none |
| `control` → other crates | `df-db` (all record and snapshot modules), `df-core` (`SecretStore`, `RewritePolicy`, `config::{AgentConfig, NetworkConfig}`), `df-proto` (`gateway::v1` servers and the log client), `tonic`, `tokio`, `futures`, `tracing`, `secrecy`, `uuid` |
| `dark-factory` → `control` | `PolicyService`, `Relay`, `relay::DISCOVERY_INTERVAL`, `DnsDirectory`, `serve_internal`, `snapshot::GitSettings`, `TokenCounter` |

Outside `control/`, `df-gateway` uses `df-db` nowhere in production. `control/tests.rs` and the test modules use `df-test-postgres`, TLS fixtures and the metric probe. After `sandbox-contract-and-tls`, those come from `df-tls` and `df-telemetry`.

## Goals / Non-Goals

**Goals:**
- `df-gateway` has no dependency on `df-db`, `sqlx` or `df-test-postgres`.
- `control/` moves without changes in behavior. Only module paths and visibility change.

**Non-Goals:**
- Changing the gateway link protocol or the control services.
- Architecture #4 (`orchestrator-crate`).

## Decisions

### 1. `df-gateway-control` depends on `df-gateway`

The control side is the other end of the gateway link. It uses the link client and connector (`link`), the flow types (`flow`), the DNS-SAN peer check (`peer`) and the wire conversions (`convert`) that the data plane uses too. A dependency on `df-gateway` keeps one definition of each.

Alternative: move those modules into a third crate (`df-gateway-link`) that both depend on. Rejected: one more crate for about 300 lines with no second user. Revisit if the data plane must stop exporting them.

### 2. Module layout keeps the paths

`crates/df-gateway-control/src/lib.rs` takes the content of `control/mod.rs` (same `pub mod` and `pub use` items), and each module keeps its file name. `crate::control::X` becomes `crate::X`, `crate::{convert, flow, link, peer, tail}` becomes `df_gateway::{…}` (`tail` is used by `relay/tests.rs`). `dark-factory` replaces `df_gateway::control::` with `df_gateway_control::`.

In `df-gateway`, `convert` goes from `pub(crate) mod` to `pub mod`, and the functions the control side imports (`wire_api_to_proto`, `scheme_to_proto`, `wire_api_from_proto`) go from `pub(crate)` to `pub`, unchanged otherwise.

### 3. Tests move with their modules

`control/tests.rs`, `control/relay/` and `control/usage_records/tests.rs` move along. `relay/tests.rs` connects through `link::PlainConnector`, which is `#[cfg(test)] pub(crate)`. It becomes `#[cfg(any(test, feature = "test-util"))] pub` behind a new `test-util` feature of `df-gateway`, and `df-gateway-control` takes `df-gateway` with `test-util` as a dev-dependency (`docs/architecture.md`, rule 1). The database tests keep the `no-remote-exec` tag in the new `BUILD.bazel`. `df-gateway`'s `BUILD.bazel` loses its database test targets (if any test outside `control/` needs the database, it fails to compile and shows up during the move).

## Risks / Trade-offs

- [`df-gateway` exports items that only the control side uses (`convert`)] → Accepted: they are part of the link contract. Their doc comments say so.
- [Hidden data-plane use of `df-db` in tests outside `control/`] → `cargo build -p df-gateway --tests` without `df-db` in its manifest finds it. Such tests move to `df-gateway-control` or switch to a fake.

## Migration Plan

Code-only refactor, no configuration or Helm change. Rollback is a revert.
