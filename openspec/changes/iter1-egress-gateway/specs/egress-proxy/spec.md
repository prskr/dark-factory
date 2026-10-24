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
Until a gateway pod has received its first policy snapshot, it SHALL deny every request (reason `no-snapshot`) and SHALL report not ready on its health endpoint, so that the Service sends it no sandbox traffic during rollouts.

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
The proxy SHALL resolve hostnames itself and SHALL refuse a name if any of its resolved addresses is in a private, loopback, link-local, CGNAT or cluster range (reason `private-destination`); `CONNECT` to IP literals SHALL be denied (reason `ip-literal`).

#### Scenario: Allowed name resolves internally
- **WHEN** an allowlisted name resolves to 10.0.0.5
- **THEN** the connection is refused with `private-destination`

#### Scenario: Mixed answer
- **WHEN** an allowlisted name resolves to a public address and to 10.0.0.5
- **THEN** the connection is refused with `private-destination`

#### Scenario: IP literal
- **WHEN** a run sends `CONNECT 1.1.1.1:443`
- **THEN** the proxy answers 403 and logs `ip-literal`

### Requirement: Upstream failures
A name that doesn't resolve SHALL be answered with 502 (`resolve-failed`), a failed connection or exchange with 502 (`upstream-failed`). If the destination doesn't accept the connection and send response headers within 30 s, the proxy SHALL answer 502 (`upstream-timeout`) and close the upstream connection.

#### Scenario: Silent destination
- **WHEN** an allowed plain HTTP destination accepts the connection but never answers
- **THEN** the proxy answers 502 after 30 s, logs `upstream-timeout` and closes the upstream connection

### Requirement: Plain HTTP only when explicit
Plain HTTP forwarding (`GET http://…`) SHALL be allowed only for entries with port 80 explicitly set.

#### Scenario: Port 80 not allowed
- **WHEN** entry `deb.debian.org` (port 443) exists and the run sends `GET http://deb.debian.org/`
- **THEN** the proxy answers 403

### Requirement: Allowlist changes apply to running sandboxes
Each new connection SHALL be checked against the latest snapshot. When a host is removed from a run's allowlist, the gateway SHALL close that run's open tunnels to it (reason `removed-from-allowlist`). When a run leaves the sandbox registry, the gateway SHALL close its open tunnels (reason `unknown-source`).

#### Scenario: Host removed mid-run
- **WHEN** `registry.npmjs.org` is removed while a run has an open tunnel to it
- **THEN** the tunnel is closed and the next `CONNECT` is denied

#### Scenario: Run ends with an open tunnel
- **WHEN** a run's pod IP is removed from the registry while it has an open tunnel
- **THEN** the tunnel is closed and logged as denied with `unknown-source`

### Requirement: Shutdown
When a gateway pod stops, it SHALL close open tunnels, answer requests still waiting for the destination with 503 and cut off streaming responses. In-flight requests SHALL get up to 2 s to record their closing event, with reason `gateway-shutdown` and verdict allowed.

#### Scenario: Request waiting at shutdown
- **WHEN** the gateway stops while a plain HTTP request waits for response headers
- **THEN** the client gets 503 and exactly one closing event with `gateway-shutdown` is recorded
