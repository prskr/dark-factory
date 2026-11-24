## ADDED Requirements

### Requirement: Policy before the pod
The control plane SHALL create a run's per-run policy (the `NetworkPolicy`, or the Cilium policy in `cilium` enforcement mode) before it creates the run's pod, and SHALL set the pod as the policy's owner once the pod exists. A policy without owner SHALL be deleted when the run is torn down. No container of the pod, sidecars included, SHALL run while the pod has no per-run policy, whether or not the namespace has a default-deny policy.

#### Scenario: Policy exists when the first container starts
- **WHEN** run 1 of `DF-7` with sidecar `playwright` is provisioned in a namespace without a default-deny policy
- **THEN** the run's policy exists before the pod is created, and the first connection of `playwright` to `1.1.1.1:443` fails

#### Scenario: Owner set after the pod
- **WHEN** the pod of run 1 of `DF-7` has been created
- **THEN** its policy has the pod as owner and is garbage-collected with the pod

#### Scenario: Interrupted between policy and pod
- **WHEN** a replica stops after creating the policy of run 1 of `DF-7` and before creating its pod
- **THEN** the next provisioning attempt reuses the policy, creates the pod and sets the owner; a teardown before that deletes the ownerless policy
