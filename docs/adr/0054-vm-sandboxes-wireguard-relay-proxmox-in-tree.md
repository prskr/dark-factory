# ADR-0054: VM sandboxes behind a WireGuard relay, Proxmox in-tree, cloud providers as plugins

- **Status:** Accepted (not planned; prerequisites in the roadmap backlog)
- **Date:** 2026-11-03
- **Related:** amends [ADR-0030](0030-kubernetes-only-with-sandbox-provider-seam.md) and [ADR-0023](0023-kata-isolation-no-nested-containers.md); [ADR-0002](0002-wasi-plugins-wasmtime-component-model.md), [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md), [ADR-0050](0050-model-traffic-and-usage-only-through-the-gateway.md)

## Context

Some tasks need what a Kata pod doesn't offer: starting containers (ADR-0023 forbids container daemons in sandboxes), more CPU or memory than the cluster should give one pod, GPUs or another architecture. Full VMs cover this, on local Proxmox hypervisors (the most common homelab hypervisor) or at cloud providers. A VM sits outside the cluster network: no network policy, no ClusterIPs, no csi-driver certificate, no pod identity. An earlier concept (`design/cloud-vm-sandboxes.md`) solved this with nftables interception in the VM and mTLS tunnels to gateway ports exposed to the internet; the handoff in issue #60 proposed a WireGuard relay instead. Design: [`design/vm-sandboxes.md`](../../design/vm-sandboxes.md).

## Decision

- **Capability tier, selected by sandbox profiles.** Admins define profiles (provider, size, placement, template) in the instance configuration; projects pick a default, tasks may override within what the project allows. `kubernetes` stays the default.
- **Providers:** Proxmox is an in-tree `SandboxProvider` (crate `df-proxmox`, cargo feature `proxmox`), permanently. Cloud providers (Hetzner first) are WASI plugins with a `darkfactory:sandbox` WIT contract; the host keeps enrollment, identity, user-data rendering and orphan decisions. AWS is out of scope.
- **Network:** a VM's only way out is a WireGuard tunnel to the **vpn-gateway**, a new unprivileged component (own crate, image, Deployment) that terminates WireGuard in userspace and relays a fixed set of TCP services into the cluster — control plane agent and enrollment ports, gateway proxy, Git, inference, MCP and DNS — each with a PROXY v2 header carrying the VM's tunnel IP. It never routes packets. The provider or hypervisor firewall allows only UDP to the vpn-gateway endpoint; in-guest nftables is the second layer.
- **Identification:** the gateway and the control plane accept PROXY headers only from vpn-gateway pods; the sandbox registry is keyed by pod IP or tunnel IP. Tunnel /32s are allocated per run by the control plane, which also serves the peer set to the vpn-gateway as a gRPC watch.
- **Identity:** VMs enroll with a single-use token and a bootstrap WireGuard key delivered at provisioning; `Enroll` returns a certificate with SAN `spiffe://dark-factory/sandbox/<run-id>` (signed via cert-manager `CertificateRequest`, default approver) and swaps in a WireGuard key generated in the VM. Certificates are short-lived and renewed over mTLS until the run ends. The control plane binds tunnel IP and SAN on every stream.
- **Workload:** the VM runs the same OCI images and the same rendered Pod spec as Kubernetes through rootless `podman kube play`, under a root-owned `df-bootstrap` delivered per run. Image volumes become read-only extracted host directories.
- **Containers in VMs:** agents may start sibling containers through the rootless Podman socket; seccomp applies to them. Kubernetes sandboxes keep ADR-0023 unchanged.
- **DNS:** VM sandboxes require the gateway DNS resolver; no interim.

## Consequences

- VM sandboxes get the same egress allowlist, live log, Git proxy and inference path as pods; the gateway changes only by accepting PROXY headers.
- No ports of the homelab are exposed to the internet for Proxmox; the vpn-gateway's exposure (Service type, `UDPRoute`, NetBird, …) is the operator's choice.
- Nothing delivered at provisioning (token, bootstrap WireGuard key) is useful after enrollment.
- New moving parts: the vpn-gateway, the enrollment listener, Proxmox ISO handling, task disks moved between VMs, a second Pod-spec target.
- Prerequisites before the Proxmox provider: gateway DNS, the inference gateway (no model credentials in VMs), spikes for the relay and for enrollment.
- VM sandboxes have no cluster targets in the first version.
- Operators build a VM template and provide an isolated bridge or VLAN; the control plane verifies the Proxmox firewall but not the router.

## Alternatives considered

- **nftables interception plus internet-exposed mTLS tunnel listeners** (previous concept): only a declared outer fence, public ports on the homelab, and a DNS stub and transparent proxy in the runtime.
- **WireGuard as an L3 router into the cluster:** routing state, `NET_ADMIN`, and masquerading loses the identity.
- **Dedicated VLAN with source-IP identity for Proxmox:** security would rest on LAN configuration, and clouds would need a second mechanism.
- **Every provider as a plugin, Proxmox included:** Proxmox needs ISO building, disk moves and stateful polling, and is permanent scope.
- **Every provider in-tree:** does not scale across cloud providers.
- **Per-project machine images:** an extra artifact per project and no shared Pod model.
- **approver-policy as a prerequisite, SPIFFE/SPIRE:** extra cluster dependencies for checks the control plane already does.
