# egress-policy-distribution Specification

## Purpose
Gets the sandbox registry and each run's effective allowlist from the database to every gateway pod within about a second, without giving the gateway database access (`design/egress.md` §6).

## Requirements

### Requirement: PolicyWatch stream
The control plane SHALL serve `PolicyWatch` on its internal port to gateway clients authenticated by certificates from the instance CA. On connect it SHALL send a full snapshot (version, sandbox registry with pod IP → run (id and number), task, project, persona, and each run's effective `http-host` allowlist), then a new snapshot or update with a higher version after every relevant change.

#### Scenario: Gateway connects
- **WHEN** a gateway pod opens `PolicyWatch`
- **THEN** it receives a snapshot containing every active run that has a pod IP, each with its run id and run number

#### Scenario: Unauthenticated client
- **WHEN** a client without a valid instance certificate calls `PolicyWatch`
- **THEN** the call is rejected

### Requirement: Changes propagate within a second
Allowlist changes and run start/end SHALL reach every connected gateway pod via notifications, typically within one second of commit, regardless of which control plane replica the gateway is connected to.

#### Scenario: Entry added on replica A, gateway on replica B
- **WHEN** a board member adds `pypi.org` to project `DF` through replica A
- **THEN** a gateway pod connected to replica B allows `pypi.org` for `DF` runs within one second

#### Scenario: Run ends
- **WHEN** a run's sandbox is gone
- **THEN** its pod IP is removed from the registry in the next update

### Requirement: Monotonic versions and resync
Versions SHALL increase monotonically; a gateway receiving a version gap or reconnecting SHALL obtain a full snapshot, and it SHALL keep enforcing its last snapshot while disconnected.

#### Scenario: Control plane restart
- **WHEN** the control plane replica restarts
- **THEN** the gateway keeps its policy, reconnects to any replica and receives a full snapshot

### Requirement: Git routes in the snapshot
Each sandbox entry of the policy snapshot SHALL carry the run's Git route (project repository URL, base, task and WIP branch, commit identity, forge credential reference). The snapshot SHALL carry the referenced forge credentials once each. Gateways SHALL keep credentials in memory only and never log them.

#### Scenario: Run with a Git route
- **WHEN** a gateway receives a snapshot while run R of `DF-7` is active
- **THEN** R's entry names upstream `https://git.example.org/peter/dark-factory.git` and task branch `refs/heads/factory/task/DF-7`, and the referenced credential is present in the snapshot

#### Scenario: Credential rotated
- **WHEN** the Kubernetes Secret behind a project's forge credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute

### Requirement: Push policies and exceptions in the snapshot
The policy snapshot SHALL carry each active run's project push policy and the run's task exceptions, and SHALL be updated within one second after a policy change or an approved exception.

#### Scenario: Exception reaches the gateway
- **WHEN** a `push-check-exception` request of `DF-7` is approved through replica A
- **THEN** a gateway connected to replica B has the exception for `DF-7` within one second

### Requirement: Inference routes in the snapshot
Each sandbox entry of the policy snapshot SHALL carry the run's inference route: chosen wire API, upstream URL per offered wire API, credential reference and scheme, and the provider headers rendered for the run. The snapshot SHALL carry the referenced provider credentials once each. Gateways SHALL keep them in memory only and never log them.

#### Scenario: Run with an inference route
- **WHEN** a gateway receives a snapshot while run R of persona "Linus" is active
- **THEN** R's entry names wire API Anthropic Messages, upstream `https://api.anthropic.com`, scheme `claude-subscription`, and the referenced credential is present in the snapshot

#### Scenario: Provider credential rotated
- **WHEN** the Kubernetes Secret behind a model provider's credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute

#### Scenario: Unresolvable provider credential
- **WHEN** the Secret of a run's model provider cannot be read
- **THEN** the snapshot carries the route without that credential and the other entries are unaffected
