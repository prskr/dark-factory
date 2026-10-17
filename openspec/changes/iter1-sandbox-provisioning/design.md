# Design

## Context

`design/agent-run.md` §4–5, `design/egress.md` §5, `design/deployment.md` §1–3, ADR-0022, ADR-0023, ADR-0029, ADR-0030. Trait definitions and fakes: `iter1-foundation` design D3. agent-sandbox is pre-1.0 (v0.5.x) — pinned, types generated from the CRD actually deployed in the homelab.

## Goals / Non-Goals

**Goals:**
- All Kubernetes knowledge lives in `df-k8s`; nothing else imports kube types (ADR-0030).
- Object rendering is pure and snapshot-tested; API interaction is tested against the dev cluster.

**Non-Goals:**
- Cilium mode, `cidr` and `fqdn` targets, persona/task scopes, PVC garbage collection, "reset workspace", warm pools.

## Decisions

### D1 — Rendering as pure functions
`render_sandbox(&RunSandboxSpec, &ClusterTargets, &ProvisioningConfig) -> Sandbox`, `render_pvc(..)`, `render_network_policy(..)`. Snapshot tests (`insta`) cover every input variant. Server-side apply with field manager `dark-factory` for all writes, so re-renders are idempotent and manual edits are overwritten.

### D2 — Naming and labels
Sandbox `run-<task key lower>-<run number>` (≤ 63 chars, task keys are short). Labels: `app.kubernetes.io/managed-by=dark-factory`, `dark-factory.io/instance=<release>`, `dark-factory.io/run-id`, `dark-factory.io/task-key`, `dark-factory.io/project`, `dark-factory.io/persona`. NetworkPolicy `run-…` with an ownerReference to the Sandbox.

### D3 — Certificate volume
csi-driver attributes: issuer from config, `uri-sans: spiffe://dark-factory/ns/${POD_NAMESPACE}/pod/${POD_NAME}/${POD_UID}`, duration and renew-before from config (default 6 h / 2 h), key and cert at `/run/factory/tls/` — mounted read-only next to the tmpfs, never in a Secret. The CA bundle for verifying the control plane comes from the same csi volume (`ca.crt`).

### D4 — Informer cache and events
One `reflector` on pods in the sandbox namespace filtered by the instance label, one on Sandboxes. `resolve_identity` is a cache lookup (no API call per connection). `events()` is derived from pod watch transitions: Running with IP → `Started`; deleted, Failed, or a container waiting with `ErrImagePull`/`ImagePullBackOff`/`CreateContainerConfigError` → `Gone` with reason.

### D5 — Network policy reconciliation
Leader election with a `coordination.k8s.io` Lease (`kube-runtime` or a small Lease loop). The leader re-renders all active runs' policies on: `egress_policy` notifications, Service/EndpointSlice watch events in cluster-target namespaces, and a 30 s resync that also overwrites drift. `hostAliases`/`NO_PROXY` are fixed at pod creation — a target added mid-run is reachable by IP immediately and by name from the next run. *ponytail:* no in-place pod mutation; acceptable since runs are short-lived.

### D6 — Fallback for image volumes
If spike 1.1 shows image volumes don't work with Talos containerd + Kata, the renderer switches (config flag `runtimeDelivery: imageVolume|initContainer`) to an init container copying from the runtime image into an `emptyDir` at `/opt/factory`. Pod layout stays identical for the runtime.

## Risks / Trade-offs

- [agent-sandbox pod template drops fields (image volumes, CSI, runtime class)] → spike 1.3 first; fallback is the `initContainer` delivery and reporting upstream.
- [Kata breaks pod source IP or NetworkPolicy enforcement] → spike 1.4; without source IPs, identity falls back to SAN + UID only — that would weaken the stolen-key guarantee and needs a decision before continuing.
- [Lease leader dies mid-reconcile] → the next leader resyncs everything; rendering is idempotent.
