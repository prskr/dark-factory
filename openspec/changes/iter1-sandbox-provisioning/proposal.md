# Proposal

## Why

Each run needs a cold-started, isolated sandbox with the task's volume, the runtime mounted read-only, a per-pod certificate and a network policy that only lets it reach what it needs. This is the riskiest infrastructure part of iteration 1 (milestone 4 and several §7 spikes of `product/iterations/iteration-1.md`).

## What Changes

- Spikes: image volumes with Talos containerd + Kata, csi-driver volumes in Kata, agent-sandbox pod template passthrough, NetworkPolicy enforcement and pod source IPs with Kata.
- kopium-generated types for the pinned agent-sandbox `Sandbox` CRD.
- `SandboxProvider` implementation in `df-k8s`: task PVC (`factory-workspace` StorageClass, size from config), one `Sandbox` per run (project image, runtime image volume at `/opt/factory`, csi-driver certificate, PVC at `/data`, tmpfs at `/run/factory`, runtime class, restricted security context, no SA token, labels, proxy and control-plane env, `hostAliases`), teardown, identity resolution from a pod informer, sandbox events.
- Generated per-run `NetworkPolicy` (Kubernetes mode) owned by the Sandbox, rendered from the database's cluster targets and re-rendered by the leader on changes and drift.
- `SecretStore` implementation reading referenced Secrets in the instance namespace.
- Startup check of the sandbox namespace and permissions (`SelfSubjectAccessReview`).

## Capabilities

### New Capabilities
- `sandbox-provisioning`: what a run's sandbox and task volume look like and their lifecycle.
- `sandbox-identity`: resolving an mTLS peer to a run.
- `run-network-policy`: the generated per-run network policy and the sandbox's network wiring.
- `credential-resolution`: reading referenced Kubernetes Secrets.

### Modified Capabilities
None.

## Impact

- New crate `df-k8s` (kube-rs, k8s-openapi, kopium output checked in).
- Implements `SandboxProvider` and `SecretStore` from `iter1-foundation`; consumed by `iter1-run-orchestration` and `iter1-integration-plugins`.
- Reads `egress_allowlist` (`cluster-service` entries) from the foundation schema; `iter1-egress-gateway` edits those entries.
- Requires the RBAC and sandbox namespace from `iter1-build-and-deploy`.
- Cilium mode, CIDR targets and persona/task scopes are deferred (ADR-0029, iteration-1 §3).
