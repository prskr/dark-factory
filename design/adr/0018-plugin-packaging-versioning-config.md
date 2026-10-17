# ADR-0018: Plugin packaging, versioning and configuration

- **Status:** Accepted (amended by ADR-0028)
- **Date:** 2026-10-16
- **Related:** [ADR-0002](0002-wasi-plugins-wasmtime-component-model.md), [ADR-0028](0028-integrations-wit-capability-interfaces.md)

## Context

Plugins need a distribution format, interface versioning and non-secret configuration (e.g. base URLs).

## Decision

- Plugins are distributed as OCI artifacts (e.g. from Harbor).
- WIT packages are namespaced and semver-versioned from the start.
- Non-secret configuration comes through a `wasi:config`-style interface; secrets never enter plugins (ADR-0002).

## Consequences

- Plugins use the same registry and supply chain as images.

## Alternatives considered

- **Plugins bundled into the control plane image**: every plugin update needs a control plane release.
