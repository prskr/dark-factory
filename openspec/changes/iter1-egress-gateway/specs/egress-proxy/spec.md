# Spec Delta

## Purpose

The gateway's egress proxy is the only path from sandboxes to external HTTP(S) hosts; it allows exactly the hosts on each run's allowlist and attributes every attempt to a run (`design/egress.md` §3, §7).

## ADDED Requirements

### Requirement: Source identification
The proxy SHALL identify the run by the connection's source pod IP using the sandbox registry from the latest policy snapshot, and SHALL deny connections from unknown sources with reason `unknown-source`.

#### Scenario: Unknown pod
- **WHEN** a pod that isn't an active sandbox connects to the proxy
- **THEN** every request is answered with 403 and logged as `unknown-source`

### Requirement: Fail closed
Until a gateway pod has received its first policy snapshot, it SHALL deny every request and SHALL report not ready on its health endpoint, so that the Service sends it no sandbox traffic during rollouts.

#### Scenario: Gateway starts before the control plane
- **WHEN** a sandbox sends `CONNECT` to a gateway pod without a snapshot
- **THEN** the request is denied with 503

#### Scenario: Readiness follows the snapshot
- **WHEN** a new gateway pod starts during a rollout
- **THEN** its readiness probe fails until the first snapshot arrived and succeeds afterwards

### Requirement: CONNECT allowlist
`CONNECT host:port` SHALL be allowed only if an `http-host` entry of the run (instance or project scope) matches the host exactly or as `*.suffix`, and the port (default 443). Otherwise it SHALL be answered with 403 and logged as `not-allowlisted`.

#### Scenario: Allowed host
- **WHEN** a run with entry `api.anthropic.com` sends `CONNECT api.anthropic.com:443`
- **THEN** the tunnel is established

#### Scenario: Wildcard
- **WHEN** entry `*.crates.io` exists and the run connects to `static.crates.io:443`
- **THEN** the tunnel is established

#### Scenario: Not allowlisted
- **WHEN** the run sends `CONNECT example.com:443` without an entry
- **THEN** the proxy answers 403

### Requirement: SNI must match
For allowed `CONNECT` tunnels the proxy SHALL read the TLS ClientHello and close the tunnel if the SNI is missing or differs from the `CONNECT` host (reason `sni-mismatch`).

#### Scenario: Domain fronting
- **WHEN** a run connects to an allowed host but sends SNI `evil.example.com`
- **THEN** the tunnel is closed before any data reaches the destination

### Requirement: No private destinations
The proxy SHALL resolve hostnames itself and SHALL refuse destinations in private, loopback, link-local, CGNAT and cluster ranges (reason `private-destination`); `CONNECT` to IP literals SHALL be denied.

#### Scenario: Allowed name resolves internally
- **WHEN** an allowlisted name resolves to 10.0.0.5
- **THEN** the connection is refused with `private-destination`

#### Scenario: IP literal
- **WHEN** a run sends `CONNECT 1.1.1.1:443`
- **THEN** the proxy answers 403

### Requirement: Plain HTTP only when explicit
Plain HTTP forwarding (`GET http://…`) SHALL be allowed only for entries with port 80 explicitly set.

#### Scenario: Port 80 not allowed
- **WHEN** entry `deb.debian.org` (port 443) exists and the run sends `GET http://deb.debian.org/`
- **THEN** the proxy answers 403

### Requirement: Allowlist changes apply to running sandboxes
Each new connection SHALL be checked against the latest snapshot. When a host is removed from a run's allowlist, the gateway SHALL close that run's open tunnels to it.

#### Scenario: Host removed mid-run
- **WHEN** `registry.npmjs.org` is removed while a run has an open tunnel to it
- **THEN** the tunnel is closed and the next `CONNECT` is denied
