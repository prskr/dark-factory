# Spec Delta

## MODIFIED Requirements

### Requirement: Generated per-run policy
For each run the control plane SHALL create a `NetworkPolicy` selecting only that run's pod, owned by that pod, with no ingress and egress only to: control plane pods on the agent port; gateway pods on the proxy port; each cluster target; kube-dns on port 53 only if a target sets `allowDns`.

#### Scenario: Internet blocked
- **WHEN** the sandbox opens a direct TCP connection to 1.1.1.1:443
- **THEN** the connection fails

#### Scenario: Policy removed with the pod
- **WHEN** the pod of run 1 of `DF-1` is deleted by anyone other than the control plane
- **THEN** its NetworkPolicy is garbage-collected by Kubernetes
