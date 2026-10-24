# egress-policy-distribution Specification

## Purpose
Gets the sandbox registry and each run's effective allowlist from the database to every gateway pod within about a second, without giving the gateway database access (`design/egress.md` §6).

## Requirements

### Requirement: PolicyWatch stream
The control plane SHALL serve `PolicyWatch` on its internal port to gateway clients authenticated by certificates from the instance CA. On connect it SHALL send a full snapshot (version, sandbox registry with pod IP → run, task, project, persona, and each run's effective `http-host` allowlist), then a new snapshot or update with a higher version after every relevant change.

#### Scenario: Gateway connects
- **WHEN** a gateway pod opens `PolicyWatch`
- **THEN** it receives a snapshot containing every active run that has a pod IP

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
