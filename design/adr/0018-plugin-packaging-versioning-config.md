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

## Amendment 2026-10-22: OCI images, not wasm artifacts

- Plugins ship as ordinary OCI images (`FROM scratch`, one `.wasm`). The wasm OCI artifact media type is not used: container runtimes cannot mount it reliably.
- The cluster delivers the file to the control plane as a Kubernetes image volume. There is no init-container fallback (a `scratch` image has no copy tool); clusters without image volumes build a custom control plane image (`FROM control-plane` + `COPY plugin.wasm`).
- The control plane only reads a component from `plugins.<name>.path`; it never fetches, unpacks or caches components itself.
- Iteration 1 still bundles the Forgejo component in the control plane image (optimized build); the image-volume delivery follows with `iter2-plugin-distribution`.
