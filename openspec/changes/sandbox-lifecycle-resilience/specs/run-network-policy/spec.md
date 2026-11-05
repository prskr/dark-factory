## MODIFIED Requirements

### Requirement: Policies follow the database
When cluster-service entries change, or a referenced Service or EndpointSlice changes, or a run's policy was edited manually, the leader control plane replica SHALL re-render and overwrite the policies of all active runs within 30 seconds. A run whose cluster target is temporarily unresolvable and has no earlier resolution on this replica SHALL keep its current policy until the target resolves.

#### Scenario: Target added mid-run
- **WHEN** a cluster-service entry is added to project `DF` while `DF-1` runs
- **THEN** the run's NetworkPolicy allows the new target within 30 seconds

#### Scenario: Manual edit
- **WHEN** someone edits a run's NetworkPolicy by hand
- **THEN** the leader restores the rendered version within 30 seconds

#### Scenario: New leader cannot resolve a target
- **WHEN** a replica becomes leader while `DF-1` runs with target `nativelink/scheduler`, and its first lookup of that Service times out
- **THEN** the NetworkPolicy of `DF-1` keeps its rule for `nativelink/scheduler`, a warning naming the target is logged, and the policy is rendered again once the lookup succeeds

#### Scenario: Deleted target Service
- **WHEN** the Service of target `nativelink/scheduler` is deleted while `DF-1` runs
- **THEN** the run's NetworkPolicy no longer allows that target within 30 seconds
