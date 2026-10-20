# helm-deployment Specification

## Purpose

Defines what one Helm release creates for a Dark Factory instance and the exact permissions each component receives, following `design/deployment.md` and ADR-0031.

## Requirements

### Requirement: One release is one instance
A release SHALL create the control plane Deployment, the gateway Deployment (≥ 2 replicas with a PodDisruptionBudget), their Services (incl. a headless Service for gateway pods), the instance config file and ServiceAccounts. Cluster-scoped objects SHALL be prefixed with the release name and only created where enabled.

#### Scenario: Two instances in one cluster
- **WHEN** two releases with different names and namespaces are installed
- **THEN** no object of one release conflicts with an object of the other

### Requirement: Migrations before rollout
The chart SHALL run `dark-factory migrate` as a pre-install/pre-upgrade hook Job with the owner credentials Secret and the optional application password Secret.

#### Scenario: Upgrade with a new migration
- **WHEN** a release is upgraded
- **THEN** the migrate Job completes before the control plane pods are replaced

### Requirement: Sandbox namespace
With `sandboxNamespace.create: true` the chart SHALL create the sandbox namespace with Pod Security Admission `restricted`, a default-deny ingress and egress NetworkPolicy, a ResourceQuota and a LimitRange; otherwise it SHALL reference an existing namespace. The sandbox namespace SHALL contain no Secrets.

#### Scenario: Default-deny baseline
- **WHEN** a pod without a run policy starts in the sandbox namespace
- **THEN** it can neither send nor receive any traffic

### Requirement: Least-privilege RBAC
The chart SHALL grant exactly the permissions in `design/deployment.md` §3: the control plane's write access only in the sandbox namespace, Secret read and Leases in the instance namespace, Service/EndpointSlice read in cluster-target namespaces; no Kubernetes API access for the gateway and sandboxes. `pods/exec`, `pods/attach` and namespace create/delete SHALL never be granted.

#### Scenario: No exec rights
- **WHEN** `kubectl auth can-i create pods/exec --as=<control plane SA> -n <sandbox ns>` is evaluated
- **THEN** the answer is `no`

#### Scenario: Gateway has no API token
- **WHEN** a gateway pod is inspected
- **THEN** `automountServiceAccountToken` is `false`

### Requirement: Certificates
The chart SHALL create (or reference) a private CA Issuer usable by cert-manager csi-driver, and Certificates for the control plane (agent and internal ports) and the gateway, all issued by that CA.

#### Scenario: Gateway authenticates to the control plane
- **WHEN** a gateway pod connects to the control plane's internal port
- **THEN** both sides present certificates issued by the instance's private CA

### Requirement: Instance network policies
The chart SHALL restrict the instance namespace: control plane ingress only from the sandbox namespace (agent port), gateway pods (internal port) and the ingress controller (UI); gateway ingress only from the sandbox namespace (proxy port) and control plane pods (internal port).

#### Scenario: Sandbox cannot reach the UI port
- **WHEN** a sandbox pod connects to the control plane's UI port
- **THEN** the connection is dropped
