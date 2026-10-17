# ADR-0029: Egress gateway and generated network policies

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0007](0007-custom-protobuf-agent-protocol.md), [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0023](0023-kata-isolation-no-nested-containers.md), [ADR-0030](0030-kubernetes-only-with-sandbox-provider-seam.md)

## Context

Sandboxes need network access to model providers, package registries, the forge and in-cluster services such as a Bazel RBE scheduler (NativeLink). In the existing Paperclip setup a host allowlist is maintained per environment, but the effective `CiliumNetworkPolicy` is never updated. Requiring Cilium is a heavy prerequisite for anyone else, while upstream `NetworkPolicy` has no DNS awareness. A Git proxy (F1) and an inference gateway (F5) were already planned as control-plane-side data paths.

## Decision

- **Gateway Deployment:** a separate Deployment (same binary, `gateway` mode) hosts the HTTP egress proxy now, and the Git smart-HTTP proxy and inference endpoint later. It has no database credentials and is fail-closed.
- **Native protocols, not the agent protocol:** sandboxes use each data path with its standard protocol — HTTP `CONNECT` for egress, Git smart HTTP, and an OpenAI-compatible HTTP API for inference (full-harness mode, and any harness that accepts a base URL). The gateway identifies the sandbox by pod IP for all of them, injects credentials, and for inference enforces the run's models and budgets and measures usage.
- **Gateway ↔ control plane:** a separate internal gRPC API (`darkfactory.gateway.v1`): `PolicyWatch` (sandbox registry, allowlists, later Git, inference and MCP routing), `Tail` (live log) and usage/call reports. It is specified when the gateway is built.
- **Credentials for data paths** (provider keys, forge tokens, upstream MCP credentials) are pushed by the control plane over this API and held in memory only; the gateway never reads Kubernetes Secrets itself.
- Third-party MCP servers are another data path in the gateway (ADR-0032).
- **HTTP(S) egress goes through the proxy.** Sandboxes get `HTTP(S)_PROXY` with the gateway's ClusterIP and need no DNS. The proxy allows `CONNECT` only to hosts on the run's allowlist, requires the TLS SNI to match, resolves names itself and refuses private/cluster destinations unless explicitly allowed. Sandboxes are identified by source pod IP.
- **Allowlist entries** are scoped to instance, project, persona or task, and come in kinds: `http-host` (proxy), `cluster-service` (namespace/service/ports, direct, any protocol — e.g. NativeLink), `cidr` (escape hatch), and `fqdn` (Cilium mode only).
- **Dark Factory generates and owns one network policy per run**, owned by the Sandbox. Enforcement mode per instance: `kubernetes` (`NetworkPolicy`, default) or `cilium` (`CiliumNetworkPolicy`); cluster targets render into either. `cluster-service` targets also get `hostAliases` and `NO_PROXY` entries, so they work without DNS.
- **Reconciliation:** the control plane leader re-renders policies of active sandboxes when allowlists or referenced Services change, and overwrites drift.
- **Mid-run fan-out:** allowlist commits trigger `NOTIFY`; every control plane replica pushes versioned snapshots and updates to the gateway pods over a `PolicyWatch` gRPC stream.
- **Live egress log:** gateway pods keep an in-memory ring buffer of flow events and serve a `Tail` stream; the control plane relays and merges them to the UI (Hubble Relay pattern), with a "denied hosts" view and one-click allowlisting. Only per-run aggregates are persisted.

## Consequences

- Works with any NetworkPolicy-enforcing CNI; Cilium becomes optional.
- No DNS from sandboxes by default, which closes DNS as an exfiltration channel.
- Proxy allowlist changes reach running sandboxes in under a second; network policy changes as fast as the CNI applies them.
- Every outbound connection is attributable to a run; missing domains are visible live.
- An additional Deployment to operate, and the gateway is on the critical path for all external traffic (two replicas, PDB).
- Tools that ignore proxy settings fail closed and don't show up in the egress log (Hubble shows them in Cilium mode).
- Details: `design/egress.md`.

## Alternatives considered

- **Cilium-only (`toFQDNs`)**: requires Cilium, needs DNS in the sandbox, no per-run attribution beyond Hubble.
- **Upstream `NetworkPolicy` with IP allowlists only**: unusable for SaaS endpoints with changing IPs.
- **Proxy inside the control plane process**: puts bandwidth-heavy data paths into the API process and its scaling.
- **Gateway reading Postgres directly**: gives the component closest to untrusted traffic database credentials.
