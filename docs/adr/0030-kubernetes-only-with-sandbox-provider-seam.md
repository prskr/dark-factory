# ADR-0030: Kubernetes only, with a sandbox provider seam and scoped RBAC

- **Status:** Accepted (amended by [ADR-0054](0054-vm-sandboxes-wireguard-relay-proxmox-in-tree.md))
- **Date:** 2026-10-16
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md), [ADR-0031](0031-helm-chart-deployment-packaging.md)

## Context

Dark Factory relies heavily on Kubernetes: sandbox pods, pod identity and csi-driver certificates for mTLS, PVCs as per-task locks, network policies, image volumes, Lease-based leader election. The question was whether this dependency should be weakened.

## Decision

- Kubernetes stays the only supported platform.
- The control plane talks to sandboxes through a `SandboxProvider` trait: provision a run's sandbox, tear it down, provide per-task volumes with exclusive-mount semantics, verify a workload's identity (for Kubernetes: pod UID and IP), and render network policy. The Kubernetes provider is the only implementation. Tasks, runs, transcripts, plugins and auth stay free of Kubernetes types.
- Sandboxes run in their own namespace, configured per instance. The control plane never creates it: the deployment (Helm chart, ADR-0031) or the operator defines it, including Pod Security labels, a default-deny baseline policy and quotas. At startup the control plane verifies that the namespace exists and that its permissions are in place, and refuses to start otherwise.
- The control plane's write permissions (Pods, PVCs, network policies) are granted by a Role in the sandbox namespace only; read access to Services/EndpointSlices is limited to the namespaces of cluster targets. `pods/exec`, `pods/attach` and namespace creation are never granted. The gateway and sandboxes get no Kubernetes API access. Full list: `design/deployment.md` §3.

## Consequences

- No abstraction cost beyond one trait; the runtime is already environment-agnostic (dials back, receives everything over the protocol).
- A later local Podman provider for contributors and CI end-to-end tests stays possible without touching the core.
- A compromised control plane can't modify workloads outside the sandbox namespace.
- Accepted costs of Kubernetes: adoption barrier (cluster with cert-manager + csi-driver, image volumes, RWO storage, a NetworkPolicy-enforcing CNI) and seconds to tens of seconds of per-run start-up latency.

## Alternatives considered

- **Platform-neutral core with several providers from the start** (Docker/Podman, microVMs, hosted sandboxes): large surface for no current need.
- **No seam**: Kubernetes types would spread through the core and make any later provider a rewrite.
- **Sandbox provider as a WASI plugin**: provisioning needs Kubernetes watches and identity checks; an in-process trait is enough.

## Amendment 2026-11-03: plain Pods instead of agent-sandbox

The Kubernetes provider creates one plain `Pod` (`restartPolicy: Never`) per run; agent-sandbox is no longer a prerequisite. The `Sandbox` wrapper added nothing the control plane used: run state came from the Pod watcher, and warm pools, claims, templates, suspend/resume and `shutdownTime` are ruled out (ADR-0022). Dropping it removes the CRDs and controller from the cluster prerequisites, the 4,750 lines of kopium-generated types that had to follow a pre-1.0 release, a second watcher, the Pod to Sandbox ownership hop in identity resolution and the `agents.x-k8s.io` RBAC. The per-run `NetworkPolicy` is owned by the Pod. The `SandboxProvider` seam is unchanged. Details: `openspec/changes/archive/2026-11-03-k8s-plain-pod-sandboxes`.
