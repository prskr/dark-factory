# Proposal

## Why

Forge and issue-tracker access must run as WASI plugins with credentials injected by the host (ADR-0002, ADR-0028), and iteration 1 must prove that early with a real Forgejo plugin (milestone 6 of `product/iterations/iteration-1.md`). The PR lifecycle and the factory tools in `iter1-run-orchestration` need a working `ForgeClient`.

## What Changes

- Spikes: Wasmtime release providing WASI 0.2.12; Forgejo's draft/WIP PR mechanism via API.
- Plugin host in `df-plugins`: Wasmtime component model, WASI linked without capabilities, `wasi:http` outgoing requests intercepted (per-instance host allowlist, `Authorization` injected, plugin-set `Authorization` replaced), `wasi:config/store` with non-secret settings, stderr to tracing, per-call timeout and memory limit, capability detection from exports.
- `ForgeClient` implementation: one plugin instance per project (base URL and repository parsed from the project's repo URL, token from the project's forge credential via `SecretStore`), retry policy per the WIT comments.
- Forgejo plugin (`plugins/forgejo`, world `forge`): `pull-requests` (`create`, `find-open`, `get`, `update` incl. draft emulation) and `issues` (`get`, `add-comment`) with error mapping.

## Capabilities

### New Capabilities
- `plugin-host`: how integration plugins are executed and what they can and cannot reach.
- `forgejo-integration`: Forgejo behaviour behind the `pull-requests` and `issues` interfaces.

### Modified Capabilities
None.

## Impact

- New crate `df-plugins`, new plugin crate `plugins/forgejo` (target `wasm32-wasip2`).
- Implements `ForgeClient` from `iter1-foundation`; uses `SecretStore` (fake until `iter1-sandbox-provisioning` lands).
- The control plane image ships the component at `/plugins/forgejo.wasm` (`iter1-build-and-deploy`); the control plane only reads the component from a configured path. Plugin distribution as OCI images via image volumes (ADR-0018, amended) follows in `iter2-plugin-distribution`.
- New dependencies: wasmtime, wasmtime-wasi, wasmtime-wasi-http, wasmtime-wasi-config (host); wit-bindgen plus a small `wasi:http` client (e.g. `wstd`, decided in design D4) and serde_json (guest).
