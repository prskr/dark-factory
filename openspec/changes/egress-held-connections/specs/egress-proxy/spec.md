# Spec Delta

## MODIFIED Requirements

### Requirement: CONNECT allowlist
`CONNECT host:port` SHALL be allowed only if an `http-host` entry of the run (instance or project scope) matches the host exactly or as `*.suffix`, and the port (default 443). Otherwise it SHALL be held as described in "Held requests" and "Requests that are not held" and, unless an entry arrives during the hold, answered with 403 and logged as `not-allowlisted`.

#### Scenario: Allowed host
- **WHEN** a run with entry `api.anthropic.com` sends `CONNECT api.anthropic.com:443`
- **THEN** the tunnel is established

#### Scenario: Wildcard
- **WHEN** entry `*.crates.io` exists and the run connects to `static.crates.io:443`
- **THEN** the tunnel is established

#### Scenario: Not allowlisted
- **WHEN** the run sends `CONNECT example.com:443` without an entry and none is added within the hold duration
- **THEN** the proxy answers 403 when the hold ends

### Requirement: Shutdown
When a gateway pod stops, it SHALL close open tunnels, answer requests still waiting for the destination or held for a decision with 503 and cut off streaming responses. In-flight requests SHALL get up to 2 s to record their closing event, with reason `gateway-shutdown` and verdict allowed; a held request SHALL record verdict denied with reason `not-allowlisted`.

#### Scenario: Request waiting at shutdown
- **WHEN** the gateway stops while a plain HTTP request waits for response headers
- **THEN** the client gets 503 and exactly one closing event with `gateway-shutdown` is recorded

#### Scenario: Held request at shutdown
- **WHEN** the gateway stops while a `CONNECT example.com:443` is held
- **THEN** the client gets 503 and exactly one denied event with `not-allowlisted` is recorded

## ADDED Requirements

### Requirement: Held requests
A request of a known sandbox that would be denied as `not-allowlisted` (a `CONNECT`, or plain HTTP to port 80) SHALL be held for up to the configured hold duration instead of being answered at once, except as stated in "Requests that are not held". The gateway SHALL decide again on every new policy snapshot, for the run that started the hold only. When the request becomes allowed, it SHALL continue exactly as an allowed request. With a hold duration of 0, no request SHALL be held.

#### Scenario: Approved during the hold
- **WHEN** a run sends `CONNECT pypi.org:443` without an entry and a task entry for `pypi.org` is committed 20 s later
- **THEN** the client receives `200` about 20 s after its request, the tunnel works, and one allowed flow is recorded

#### Scenario: Plain HTTP approved during the hold
- **WHEN** a run sends `GET http://deb.debian.org/` and a port-80 entry for `deb.debian.org` is committed during the hold
- **THEN** the request is forwarded and the client receives the upstream response

#### Scenario: Hold times out
- **WHEN** a run sends `CONNECT example.com:443` with hold duration 60 s and nothing changes
- **THEN** the client receives 403 `not-allowlisted` after 60 s and one denied flow is recorded

#### Scenario: Holding off
- **WHEN** the hold duration is 0 and a run sends `CONNECT example.com:443` without an entry
- **THEN** the proxy answers 403 at once

#### Scenario: Run ends during the hold
- **WHEN** the run's pod IP leaves the registry while its request is held
- **THEN** the client receives 403 and the flow is logged as denied with `unknown-source`

#### Scenario: Pod IP reused during the hold
- **WHEN** run A's request to `pypi.org:443` is held, A's pod IP is reassigned to run B, and B's allowlist covers `pypi.org`
- **THEN** the client receives 403 and the flow is logged as denied with `unknown-source`

### Requirement: Requests that are not held
A `not-allowlisted` request whose host is not an exact valid host name (a wildcard or an invalid name), or whose denial report cannot be queued, SHALL NOT be held and SHALL be answered with 403 `not-allowlisted` at once.

#### Scenario: Wildcard host
- **WHEN** a run sends `CONNECT *.example.com:443`
- **THEN** the proxy answers 403 `not-allowlisted` at once, without a hold

#### Scenario: Report queue full
- **WHEN** a run sends `CONNECT example.com:443` while the gateway's denial report queue is full
- **THEN** the proxy answers 403 `not-allowlisted` at once, and the next attempt is reported

### Requirement: Overlapping holds share the deadline
A request held while another hold of the same run, host and port is active in the same gateway pod SHALL end at that hold's deadline.

#### Scenario: Overlapping holds
- **WHEN** a run's `CONNECT pypi.org:443` is held at 10:00:00 with hold duration 60 s, and a second one at 10:00:30
- **THEN** both holds end at 10:01:00 unless an entry arrives earlier, and an entry releases both

### Requirement: Refused hosts are not held
A request that is not allowlisted SHALL be answered with 403 `not-allowlisted` at once, without a hold, when its host matches an instance deny rule or its host and port were refused for the run's task. A held request that is still not allowlisted SHALL end with 403 as soon as a snapshot shows such a refusal.

#### Scenario: Deny rule
- **WHEN** deny rule `*.datadoghq.com` exists and a run sends `CONNECT http-intake.logs.us5.datadoghq.com:443`
- **THEN** the proxy answers 403 at once

#### Scenario: Denied during the hold
- **WHEN** a board member denies the `network-access` request for `example.com:443` of `DF-7` while a request of `DF-7` to it is held
- **THEN** the client receives 403 within about a second of the decision

#### Scenario: Later attempt after a deny
- **WHEN** the request for `example.com:443` of `DF-7` was denied and the run connects to `example.com:443` again
- **THEN** the proxy answers 403 at once

### Requirement: Hold limit per sandbox
Each gateway pod SHALL hold at most 32 requests per sandbox at a time. A further request that would be held SHALL be answered with 403 `not-allowlisted` at once.

#### Scenario: Burst of unknown hosts
- **WHEN** a sandbox opens 40 concurrent `CONNECT`s to hosts without entries
- **THEN** 32 are held and 8 are answered with 403 at once
