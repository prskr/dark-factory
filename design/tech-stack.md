# Tech Stack

> The whole stack in one place. The reasoning lives in the linked ADRs ([`design/adr/`](adr/README.md)); this document only says *what* is used where.
>
> **Status column:** *Decided* — backed by an accepted ADR. *Proposed* — backed by a proposed ADR. *Suggested* — a reasonable default nobody has decided yet; settle it when the component is built. *Later* — planned for a later iteration.

## Overview

| Component | Runs as | Language | Talks to |
|---|---|---|---|
| Control plane | Kubernetes Deployment (1..n replicas) | Rust | PostgreSQL, Kubernetes API, Keycloak, WASI plugins → forge/tracker, agent runtimes, gateway |
| Gateway | Kubernetes Deployment (≥ 2 replicas), same binary in `gateway` mode | Rust | Control plane (policy stream, log relay), sandboxes (proxy), the internet |
| Agent runtime | Static binary in every sandbox pod (image volume) | Rust | Control plane (gRPC, mTLS), harness (ACP over stdio), Git remote and other hosts via the gateway |
| Harness | Subprocess of the runtime | — (Claude Code: Node) | Model provider, runtime (ACP, MCP over stdio) |
| Integration plugins | WebAssembly components inside the control plane | Any language with Component Model support (Rust first) | Forge/tracker HTTP APIs via the host |
| Web UI | Server-rendered by the control plane | Rust (maud) + vendored htmx/Alpine | Control plane (HTTP, SSE) |

## Languages and repository

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Language | Rust (control plane, runtime, first plugins) | Decided | [0001](adr/0001-rust-for-control-plane-and-runtime.md) |
| Workspace | One Cargo workspace; shared crates for protocol and types | Decided | [0001](adr/0001-rust-for-control-plane-and-runtime.md), [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Contracts | Protobuf (agent protocol, transcript), WIT (plugins), JSON Schema (factory tools) in [`contract/`](../contract/README.md) | Decided | [0007](adr/0007-custom-protobuf-agent-protocol.md), [0017](adr/0017-transcript-schema-and-versioning.md), [0018](adr/0018-plugin-packaging-versioning-config.md) |
| Rust edition / toolchain | Edition 2024, stable toolchain pinned in `rust-toolchain.toml` | Suggested | — |

## Control plane

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Async runtime / HTTP | tokio, axum | Decided | [0003](adr/0003-control-plane-stack.md) |
| Integration API docs | utoipa (OpenAPI) | Decided | [0003](adr/0003-control-plane-stack.md) |
| Agent port | tonic + prost, rustls with the `ring` provider, client-certificate verification | Decided | [0006](adr/0006-agent-dial-back-grpc-mtls.md), [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Agent protocol semantics | Agent → CP sequenced with acks; CP → agent idempotent; typed tool calls | Decided | [0027](adr/0027-agent-protocol-delivery-and-typed-tool-calls.md) |
| Kubernetes | kube-rs, `kopium`-generated agent-sandbox types, Lease-based leader election | Decided | [0001](adr/0001-rust-for-control-plane-and-runtime.md), [0003](adr/0003-control-plane-stack.md) |
| Database access | sqlx (offline mode for builds) | Decided | [0012](adr/0012-postgresql-and-blob-store.md), [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Migrations | Embedded via `sqlx::migrate!`, applied by `dark-factory migrate` (same image, run as a Job with owner credentials), which optionally manages the application role and always applies its grants; the server runs without DDL rights; expand/contract | Decided | [0012](adr/0012-postgresql-and-blob-store.md) |
| Job queue / routines | Postgres `FOR UPDATE SKIP LOCKED`, `croner` for schedules | Decided | [0003](adr/0003-control-plane-stack.md) |
| Cross-replica fan-out | Postgres `LISTEN/NOTIFY` after persisting | Decided | [0003](adr/0003-control-plane-stack.md) |
| Plugin host | wasmtime, wasmtime-wasi (no capabilities granted), wasmtime-wasi-http (request interception), wasmtime-wasi-config | Decided | [0002](adr/0002-wasi-plugins-wasmtime-component-model.md), [0028](adr/0028-integrations-wit-capability-interfaces.md) |
| Factory tools | Typed tool calls on the agent stream, executed by the control plane | Decided | [0027](adr/0027-agent-protocol-delivery-and-typed-tool-calls.md) |
| MCP tool catalogs | `rmcp` client for discovery; per-server instance filter (selection, globs, read-only only), new tools disabled by default | Decided | [0032](adr/0032-upstream-mcp-through-gateway-with-tool-catalog.md) |
| OIDC client | `openidconnect` crate (code flow + PKCE, server-side) | Suggested | [0011](adr/0011-authentication-authorization-personas.md) |
| Sessions | `tower-sessions` with a Postgres store | Suggested | [0011](adr/0011-authentication-authorization-personas.md) |
| JWT validation (integration API) | `jsonwebtoken` with JWKS caching | Suggested | [0011](adr/0011-authentication-authorization-personas.md) |
| Markdown rendering | comrak + syntect | Decided | [0004](adr/0004-server-rendered-ui-maud-htmx-alpine.md) |
| Static assets | rust-embed | Decided | [0004](adr/0004-server-rendered-ui-maud-htmx-alpine.md) |
| Telemetry | tracing + OpenTelemetry | Decided | [0003](adr/0003-control-plane-stack.md) |
| Sandbox provisioning | `SandboxProvider` trait, agent-sandbox the only implementation; write access via a Role in the sandbox namespace only | Decided | [0030](adr/0030-kubernetes-only-with-sandbox-provider-seam.md) |
| Network policy rendering | Per-run `NetworkPolicy` (default) or `CiliumNetworkPolicy`, reconciled by the leader | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Gateway policy push | `PolicyWatch` gRPC stream (snapshot + updates), fed by `LISTEN/NOTIFY` | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Egress log relay | Merge `Tail` streams from all gateway pods, serve via SSE | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |

## Gateway

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Egress proxy | tokio + hyper (`CONNECT` and plain HTTP forwarding), ClientHello SNI check (e.g. `tls-parser`), own DNS resolution with private-range blocking (`hickory-resolver`) | Decided (crates suggested) | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Policy | In-memory snapshot from `PolicyWatch`; fail-closed until the first snapshot; no database access | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Live egress log | In-memory ring buffer per pod, `Tail` gRPC stream | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Git smart-HTTP proxy | Ref-enforcing proxy for task branches | Later | F1 in [`product/context.md`](../product/context.md) |
| Secret scanning of pushes | In the Git proxy only: Kingfisher (embedded library or subprocess, live validation off); none in the sandbox | Later (scanner suggested) | [0009](adr/0009-agent-managed-workspaces-git-durable.md) |
| MCP endpoint | Streamable HTTP aggregating third-party MCP servers (`rmcp` server + clients); per-run tool set, credential injection, meta-tools for large sets, call records to the control plane | Later | [0032](adr/0032-upstream-mcp-through-gateway-with-tool-catalog.md) |
| Inference endpoint | OpenAI-compatible HTTP API with per-provider adapters; sandbox identified by pod IP; usage measured and reported to the control plane | Later | [0029](adr/0029-egress-gateway-and-generated-network-policies.md), F5 in [`product/context.md`](../product/context.md) |
| Link to the control plane | Internal gRPC API `darkfactory.gateway.v1` (`PolicyWatch`, `Tail`, usage reports), mTLS | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |

## Web UI

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Templates | maud (UI in its own crate) | Decided | [0004](adr/0004-server-rendered-ui-maud-htmx-alpine.md) |
| Interactivity | htmx 4 (fetch-based, SSE, morph swaps), Alpine.js — both vendored | Decided | [0004](adr/0004-server-rendered-ui-maud-htmx-alpine.md) |
| Live updates | Server-sent events (per task transcript, per page dashboard) | Decided | [0004](adr/0004-server-rendered-ui-maud-htmx-alpine.md) |
| CSS | Hand-written CSS, or Tailwind via its standalone binary (no Node) | Suggested — open | — |

## Agent runtime

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Async runtime / gRPC client | tokio, tonic + rustls (`ring`) | Decided | [0005](adr/0005-agent-runtime-stack.md), [0006](adr/0006-agent-dial-back-grpc-mtls.md) |
| Harness protocol | `agent-client-protocol` (official Rust SDK), ACP client role | Decided | [0005](adr/0005-agent-runtime-stack.md) |
| MCP toward the harness | `factory` server: `rmcp` over stdio (subprocess bridged to the runtime via Unix socket on tmpfs); plus the gateway's MCP endpoint registered as a second server | Decided | [0005](adr/0005-agent-runtime-stack.md), [0032](adr/0032-upstream-mcp-through-gateway-with-tool-catalog.md) |
| Git | `git` CLI with runtime-owned `.git/config`, runtime as credential helper (v1) | Decided | [0009](adr/0009-agent-managed-workspaces-git-durable.md) |
| Build | Static musl binary, amd64 | Decided | [0005](adr/0005-agent-runtime-stack.md), [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Built-in loop (full-harness mode) | reqwest + SSE (or `async-openai`); study `rtk` and open-source harnesses (Codex CLI, goose) first | Later | [0005](adr/0005-agent-runtime-stack.md) |
| LSP | `async-lsp` + `lsp-types` (after agent memory) | Later | [0005](adr/0005-agent-runtime-stack.md) |
| Agent-side WASI tools | Wasmtime without compiler, AOT-precompiled components | Later | F2 in [`product/context.md`](../product/context.md) |

## Harnesses

| Concern | Choice | Status | ADR |
|---|---|---|---|
| First harness | Claude Code via `@zed-industries/claude-agent-acp`, subscription auth (`CLAUDE_CODE_OAUTH_TOKEN`) | Decided | [0005](adr/0005-agent-runtime-stack.md), [0021](adr/0021-usage-and-cost-data.md) |
| Delivery | Node + adapter baked into the project image (iteration 1); configurable adapter path | Decided | [0022](adr/0022-project-images-runtime-volume-no-warm-pools.md) |
| Other ACP harnesses | Any ACP-compatible agent | Later | — |

## Integration plugins

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Interface | WIT package `darkfactory:integrations@0.1.0` (`pull-requests`, `issues`) | Decided | [0028](adr/0028-integrations-wit-capability-interfaces.md) |
| WASI | `wasi:*@0.2.12` imports, `wasi:http` only path to the network, `wasi:config/store@0.2.0-rc.1` | Decided | [0028](adr/0028-integrations-wit-capability-interfaces.md) |
| Guest toolchain | Rust `wasm32-wasip2` with `wit-bindgen` / `cargo component` | Suggested | [0002](adr/0002-wasi-plugins-wasmtime-component-model.md) |
| Distribution | OCI artifacts in Harbor | Decided | [0018](adr/0018-plugin-packaging-versioning-config.md) |
| First plugin | Forgejo (world `forge`) | Decided | [0002](adr/0002-wasi-plugins-wasmtime-component-model.md) |

## Storage

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Primary store | PostgreSQL (existing homelab instance or CloudNativePG) | Decided | [0012](adr/0012-postgresql-and-blob-store.md) |
| Full-text search | `tsvector` + `pg_trgm` on text-only extracts | Decided | [0012](adr/0012-postgresql-and-blob-store.md) |
| Blobs | `BlobStore` trait: Postgres `bytea` default, S3-compatible optional | Decided | [0012](adr/0012-postgresql-and-blob-store.md) |
| Agent memory | pgvector | Later | — |

## Infrastructure (assumed present)

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Platform | Kubernetes only, behind a `SandboxProvider` seam (homelab: Talos Linux, 5 bare-metal nodes) | Decided | [0030](adr/0030-kubernetes-only-with-sandbox-provider-seam.md) |
| Sandboxes | kubernetes-sigs agent-sandbox (pre-1.0, pinned) | Decided | [0006](adr/0006-agent-dial-back-grpc-mtls.md), [0022](adr/0022-project-images-runtime-volume-no-warm-pools.md) |
| Isolation | Kata Containers runtime class, restricted pod security | Decided | [0023](adr/0023-kata-isolation-no-nested-containers.md) |
| Networking | Any CNI enforcing `NetworkPolicy`; Cilium optional (`CiliumNetworkPolicy`, FQDN rules, Hubble) — the homelab runs Cilium | Decided | [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |
| Certificates | cert-manager + csi-driver, private CA Issuer | Decided | [0006](adr/0006-agent-dial-back-grpc-mtls.md) |
| Workspace volumes | Longhorn StorageClass with one replica | Decided | [0009](adr/0009-agent-managed-workspaces-git-durable.md) |
| Runtime delivery | Kubernetes image volumes (fallback: init container) | Decided | [0022](adr/0022-project-images-runtime-volume-no-warm-pools.md) |
| Identity provider | Keycloak (OIDC, `roles` claim mapper) | Decided | [0011](adr/0011-authentication-authorization-personas.md) |
| Registry | Harbor (images, plugins, pull-through cache) | Decided | [0018](adr/0018-plugin-packaging-versioning-config.md), [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Forge / issue tracker | Forgejo | Decided | [0014](adr/0014-tasks-authoritative-issues-linked.md) |
| Container-based tests | Bazel remote build execution (in-cluster NativeLink, reached as a cluster target) | Decided | [0023](adr/0023-kata-isolation-no-nested-containers.md), [0029](adr/0029-egress-gateway-and-generated-network-policies.md) |

## Build, test and development

| Concern | Choice | Status | ADR |
|---|---|---|---|
| Build | Bazel: rules_rust + crate_universe (from the Cargo workspace), rules_oci; amd64 musl cross-compilation from arm64 macOS | Decided | [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Deployment packaging | Helm chart, one release per instance; sandbox namespace and RBAC defined by the chart, never created by the control plane | Decided | [0031](adr/0031-helm-chart-deployment-packaging.md), [0030](adr/0030-kubernetes-only-with-sandbox-provider-seam.md) |
| Deploy loop | One Bazel target: build, push to Harbor, `helm upgrade --install` into the dev namespace | Decided | [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Contract tooling | protoc (29.x), wasm-tools (≥ 1.245 for WASI 0.2.12) | Decided | — |
| End-to-end tests | Fake ACP agent built on `agent-client-protocol` | Decided | [0024](adr/0024-build-tooling-and-dev-loop.md) |
| Inner loop | telepresence | Later | [0024](adr/0024-build-tooling-and-dev-loop.md) |
| CI integration environment | kind/k3d with Cilium, cert-manager, agent-sandbox | Later | [0024](adr/0024-build-tooling-and-dev-loop.md) |

## Open choices
- CSS approach for the UI.
- OIDC client, session store and JWT libraries (suggested above).
- Secret scanner for the Git proxy (Kingfisher suggested; embedded library vs subprocess depends on whether its Vectorscan dependency builds cleanly in Bazel).
- Protobuf linting/breaking-change checks (e.g. `buf`) — not discussed yet.
