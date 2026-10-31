# Spec Delta

## ADDED Requirements

### Requirement: Git routes in the snapshot
Each sandbox entry of the policy snapshot SHALL carry the run's Git route (project repository URL, base, task and WIP branch, commit identity, forge credential reference). The snapshot SHALL carry the referenced forge credentials once each. Gateways SHALL keep credentials in memory only and never log them.

#### Scenario: Run with a Git route
- **WHEN** a gateway receives a snapshot while run R of `DF-7` is active
- **THEN** R's entry names upstream `https://git.example.org/peter/dark-factory.git` and task branch `refs/heads/factory/task/DF-7`, and the referenced credential is present in the snapshot

#### Scenario: Credential rotated
- **WHEN** the Kubernetes Secret behind a project's forge credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute
