# Spec Delta

## MODIFIED Requirements

### Requirement: Git routes in the snapshot
Each sandbox entry of the policy snapshot SHALL carry the run's Git route (project repository URL, base, task and WIP branch, commit identity, the current public signing key of the run's persona, forge credential reference). The snapshot SHALL carry the referenced forge credentials once each. Gateways SHALL keep credentials in memory only and never log them. The snapshot SHALL never carry a private signing key.

#### Scenario: Run with a Git route
- **WHEN** a gateway receives a snapshot while run R of `DF-7` is active
- **THEN** R's entry names upstream `https://git.example.org/peter/dark-factory.git` and task branch `refs/heads/factory/task/DF-7`, and the referenced credential is present in the snapshot

#### Scenario: Credential rotated
- **WHEN** the Kubernetes Secret behind a project's forge credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute

#### Scenario: Signing key rotated
- **WHEN** a board member rotates the key of the persona of run R
- **THEN** the rotation publishes `egress_policy`, and gateways receive R's Git route with the new public key with the next snapshot, within one minute

#### Scenario: Push during key propagation
- **WHEN** run R pushes a commit signed with the new key before its gateway received the new snapshot
- **THEN** the push is rejected with `df:signature` and detail "signed by another key", and the same push passes once the gateway has the new snapshot
