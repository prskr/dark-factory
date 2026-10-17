# ADR-0001: Rust for the control plane and the agent runtime

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Paperclip, the inspiration for Dark Factory, is mostly Node.js and comparatively heavy to run in a homelab. Dark Factory needs two compiled components: a control plane (API, web UI, coordinator, WASI plugin host) and a small runtime inside every agent sandbox. The plugin host must run WebAssembly components with the Component Model and outbound HTTP (`wasi:http`), see ADR-0002.

## Decision

- Both the control plane and the agent runtime are written in Rust.
- One Cargo workspace; protocol and shared type crates are used by both components.
- Kubernetes access via kube-rs; agent-sandbox types are generated from its CRDs with `kopium`.

## Consequences

- Embedding Wasmtime from Rust is the most mature way to host Component Model plugins with `wasi:http`.
- Shared crates keep protocol types identical on both ends of the agent stream.
- Small static binaries (musl) fit the low-overhead goal for sandboxes.
- agent-sandbox ships Go and Python SDKs but no Rust SDK; generated types and kube-rs replace it.
- Longer compile times than Go; existing Go experience doesn't carry over directly.

## Alternatives considered

- **Go control plane, Rust runtime**: wazero (Go) doesn't support the Component Model; Go component runtimes built on it exist but are young. The plugin host is the riskiest part, so it shouldn't depend on them.
- **Go everywhere**: same plugin-host problem.
