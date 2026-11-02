# ADR-0002: WASI plugins with Wasmtime, the Component Model and wasi:http

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0018](0018-plugin-packaging-versioning-config.md), [ADR-0028](0028-integrations-wit-capability-interfaces.md)

## Context

Integrations (git forge, issue tracker, possibly sandbox provisioning) should be pluggable without recompiling the control plane, and plugins must call HTTP APIs (read issues, open PRs, comment). The WASIX paper describes socket extensions for this, but it's not an official WASI standard.

## Decision

- Plugins are WebAssembly components executed by the control plane with Wasmtime.
- Interfaces are defined in WIT; outbound HTTP uses the standard `wasi:http/outgoing-handler` (WASI 0.2+). WASIX is not used.
- The host owns every outbound request: it enforces a per-plugin host allowlist and injects authentication headers, so plugins never see credentials.
- Guests target WASI 0.2 for now (mature toolchains: `cargo component`/`wit-bindgen`, TinyGo, componentize-py, jco). WASI 0.3 (native async, ratified June 2026) follows once guest tooling settles.
- The plugin system is built in iteration 1 with a minimal forge interface, to prove it early.

## Consequences

- Plugins can be written in several languages and run sandboxed, with no network access except through the host.
- Credentials stay in the control plane.
- The host needs request interception code and a capability-free WASI context.
- A later migration from WASI 0.2 to 0.3 guests.

## Alternatives considered

- **WASIX**: non-standard; standard `wasi:http` covers the need.
- **Native Rust traits**: no isolation, every integration needs a control plane release.
- **Out-of-process plugins over gRPC**: separate deployments and credentials per plugin; heavier than in-process components.
