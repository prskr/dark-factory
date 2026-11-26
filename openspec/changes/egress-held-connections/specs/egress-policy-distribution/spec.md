# Spec Delta

## MODIFIED Requirements

### Requirement: PolicyWatch stream
The control plane SHALL serve `PolicyWatch` on its internal port to gateways with instance-CA client certificates. On connect it SHALL send a full snapshot, then a higher-versioned snapshot or update after each change. The snapshot SHALL contain:
- the version;
- the deny rules;
- the sandbox registry: pod IP → run (id, number), task, project, persona;
- each run's effective `http-host` allowlist;
- each run's refused host:port pairs (`network-access` request denied for the run's task).

#### Scenario: Gateway connects
- **WHEN** a gateway pod opens `PolicyWatch`
- **THEN** it receives a snapshot containing every active run that has a pod IP, each with its run id and run number

#### Scenario: Unauthenticated client
- **WHEN** a client without a valid instance certificate calls `PolicyWatch`
- **THEN** the call is rejected

#### Scenario: Refusals in the snapshot
- **WHEN** deny rule `*.datadoghq.com` exists and the request of `DF-7` for `example.com:443` was denied
- **THEN** the snapshot lists `*.datadoghq.com` as deny rule and `example.com:443` as refused for the runs of `DF-7` only

### Requirement: Changes propagate within a second
Allowlist changes, deny rule changes, denied `network-access` requests and run start/end SHALL reach every connected gateway pod via notifications, typically within one second of commit, regardless of which control plane replica the gateway is connected to.

#### Scenario: Entry added on replica A, gateway on replica B
- **WHEN** a board member adds `pypi.org` to project `DF` through replica A
- **THEN** a gateway pod connected to replica B allows `pypi.org` for `DF` runs within one second

#### Scenario: Run ends
- **WHEN** a run's sandbox is gone
- **THEN** its pod IP is removed from the registry in the next update

#### Scenario: Request denied
- **WHEN** a board member denies the `network-access` request of `DF-7` for `example.com:443`
- **THEN** every gateway pod receives a snapshot listing `example.com:443` as refused for `DF-7` within one second
