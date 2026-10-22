# run-network-policy Specification

## Purpose

Every run gets a generated network policy and network wiring so the sandbox reaches only the control plane, the egress proxy and its allowed cluster targets — without DNS (`design/egress.md` §5, ADR-0029; Kubernetes mode only in iteration 1).

## Requirements

### Requirement: Proxy and control plane wiring
Sandbox pods SHALL get `HTTPS_PROXY`/`HTTP_PROXY` (and lower-case variants) pointing to the gateway Service's ClusterIP and proxy port, the control plane agent endpoint as `<ClusterIP>:9443` plus its TLS server name, and `NO_PROXY` listing every cluster target of the run.

#### Scenario: No DNS needed
- **WHEN** the runtime starts
- **THEN** it reaches the control plane and the proxy using IP addresses from its environment

### Requirement: Cluster targets reachable by name
For each `cluster-service` entry applying to the run (instance and project scope), the pod SHALL get, for a non-headless Service, `hostAliases` mapping both `<svc>.<ns>.svc.<cluster-domain>` and `<svc>.<ns>.svc` to the Service's ClusterIP. A headless Service with `allowDns` SHALL get no `hostAliases` and relies on DNS. A headless Service without `allowDns` SHALL be rejected. The `hostAliases` point to the ClusterIP while the policy allows the backing pods or endpoint addresses, so this relies on egress policy being evaluated after Service translation (kube-proxy DNAT or the CNI's own service handling); any CNI enforcing upstream `NetworkPolicy` that does so is supported, Cilium included (`design/egress.md` §5, ADR-0029).

#### Scenario: ClusterIP of a target
- **WHEN** the sandbox connects to the ClusterIP of an allowed target on an allowed port
- **THEN** the connection succeeds
- **WHEN** it connects to the ClusterIP of a Service that is not a target
- **THEN** the connection fails

#### Scenario: NativeLink target
- **WHEN** project `DF` has target `nativelink/scheduler` ports `[50051]`
- **THEN** the pod resolves `scheduler.nativelink.svc.cluster.local` via `/etc/hosts` to the Service's ClusterIP

### Requirement: Generated per-run policy
For each run the control plane SHALL create a `NetworkPolicy` selecting only that run's pod, owned by its Sandbox, with no ingress and egress only to: control plane pods on the agent port; gateway pods on the proxy port; each cluster target on the Service's target ports: for a Service with a selector, its pods (namespace selector plus the Service's selector); for a selector-less Service, its EndpointSlice addresses as `/32` (`/128` for IPv6) `ipBlock` rules; kube-dns on port 53 only if a target sets `allowDns`.

#### Scenario: Internet blocked
- **WHEN** the sandbox opens a direct TCP connection to 1.1.1.1:443
- **THEN** the connection fails

#### Scenario: Target port mapping
- **WHEN** Service port 50051 maps to target port 8080
- **THEN** the policy allows port 8080 to the selected pods

#### Scenario: Service without selector
- **WHEN** a target Service has no selector
- **THEN** its EndpointSlice addresses are rendered as `/32` ipBlocks

### Requirement: Policies follow the database
When cluster-service entries change, or a referenced Service or EndpointSlice changes, or a run's policy was edited manually, the leader control plane replica SHALL re-render and overwrite the policies of all active runs within 30 seconds.

#### Scenario: Target added mid-run
- **WHEN** a cluster-service entry is added to project `DF` while `DF-1` runs
- **THEN** the run's NetworkPolicy allows the new target within 30 seconds

#### Scenario: Manual edit
- **WHEN** someone edits a run's NetworkPolicy by hand
- **THEN** the leader restores the rendered version within 30 seconds
