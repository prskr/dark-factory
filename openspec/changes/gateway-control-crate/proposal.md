# Proposal

## Why

`df-gateway` holds two different things:

- The egress data plane: proxy, git proxy, inference proxy, policy watch and log tail. It runs as `dark-factory gateway` without database access.
- The control plane's side of the gateway links (`src/control/`, about 4,000 lines with tests): the policy service, git, usage and denial records, inference routes and the log relay. It needs the database.

Because of `control/`, the data-plane crate depends on `df-db`, and through it on `sqlx` and `df-test-postgres`. A database change recompiles and retests the proxy, and nothing stops data-plane code from reaching for the database. This breaks `docs/architecture.md` rule 1 (dependency direction): the target says `df-gateway` has no database.

## What Changes

- New crate `df-gateway-control` with the modules of `df-gateway/src/control/` and their tests: `policy_service`, `snapshot`, `git_records`, `usage_records`, `denial_events`, `inference_routes`, `internal`, `relay`.
- `df-gateway-control` depends on `df-gateway` for the link and flow API (`link`, `flow`), the peer check (`peer::require_dns_san`) and the wire conversions (`convert`, made public). `df-gateway` does not depend on `df-gateway-control`.
- `df-gateway` loses `df-db`, `sqlx` and `df-test-postgres`.
- `dark-factory` imports the control side from `df_gateway_control` instead of `df_gateway::control`.
- `docs/architecture.md`: the rows of `df-gateway` and `df-gateway-control` change from **target** to current.
- No change in behavior, configuration or wire format.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Pure refactor: `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- Splitting `df-agent-channel` (architecture #4): its own change, `orchestrator-crate`.
- Moving `convert` (the `df-core` ↔ `df-proto` conversions for wire APIs and credential schemes) into `df-proto`. Rejected: `df-proto` depends on nothing, and the conversions belong to the gateway link.
- A separate binary for the gateway. Rejected: one binary with subcommands is the deployment model.

## Impact

- New crate: `crates/df-gateway-control` (`Cargo.toml`, `BUILD.bazel` with its `no-remote-exec` database tests, `MODULE.bazel` manifest entry, `[workspace.dependencies]`, lock files).
- Changed: `df-gateway` (`lib.rs`, `convert.rs` visibility, `Cargo.toml`, `BUILD.bazel`), `dark-factory` (imports and dependency).
- Contributor docs: `docs/architecture.md`, `AGENTS.md` (project overview). No published docs change.
- Dependencies: no new external crates.
- Order: after `sandbox-contract-and-tls`. The moved control tests use `df-tls` (TLS reloader, `TestPki`) and the `df-telemetry` probe instead of `df-agent-channel`. Independent of `db-write-ownership`: `control/` already uses `df-db` functions only.
