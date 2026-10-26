# Spec Delta

## MODIFIED Requirements

### Requirement: PolicyWatch stream
The control plane SHALL serve `PolicyWatch` on its internal port to gateway clients authenticated by certificates from the instance CA. On connect it SHALL send a full snapshot (version, sandbox registry with pod IP → run (id and number), task, project, persona, and each run's effective `http-host` allowlist), then a new snapshot or update with a higher version after every relevant change.

#### Scenario: Gateway connects
- **WHEN** a gateway pod opens `PolicyWatch`
- **THEN** it receives a snapshot containing every active run that has a pod IP, each with its run id and run number

#### Scenario: Unauthenticated client
- **WHEN** a client without a valid instance certificate calls `PolicyWatch`
- **THEN** the call is rejected
