# Spec Delta

## MODIFIED Requirements

### Requirement: Requests from denials
For each `not-allowlisted` request the gateway reports, the control plane SHALL raise a non-blocking `network-access` request on the run's task, or merge into the pending one, and SHALL store the hold deadline on it. It SHALL ignore the report if the run is no longer active, the task is completed or cancelled, the run's effective allowlist covers the host and port, a deny rule matches the host, or a `network-access` request of the task for the host and port was denied.

#### Scenario: Host denied for the task
- **WHEN** the request of `DF-7` for `example.com:443` was denied and its run connects to `example.com:443` again
- **THEN** no new request is raised

#### Scenario: Harness hits an unknown host
- **WHEN** the run of `DF-7` is denied `statsig.anthropic.com:443` 50 times within a minute
- **THEN** exactly one pending `network-access` request for `statsig.anthropic.com:443` exists on `DF-7`

#### Scenario: Other denial reasons
- **WHEN** a connection is denied with `sni-mismatch` or `private-destination`
- **THEN** no request is raised

#### Scenario: Denial reported after the host was allowed
- **WHEN** a denial of `pypi.org:443` is reported for `DF-7` after a task entry for `pypi.org` was committed
- **THEN** no request is raised

#### Scenario: Denial reported after cancellation
- **WHEN** a denial is reported for a run of a task that was cancelled in the meantime
- **THEN** no request is raised

#### Scenario: Denied host
- **WHEN** deny rule `*.datadoghq.com` exists and the run of `DF-7` is denied `http-intake.logs.us5.datadoghq.com:443`
- **THEN** no request is raised

#### Scenario: Hold reported with its deadline
- **WHEN** the gateway holds `CONNECT pypi.org:443` of `DF-7` at 10:00:00 with hold duration 60 s
- **THEN** the pending `network-access` request for `pypi.org:443` on `DF-7` carries the deadline 10:01:00

## ADDED Requirements

### Requirement: Gateway reports held and denied requests
The gateway SHALL report `not-allowlisted` requests it holds or denies, with the hold deadline if held, at most once per run, host and port per report interval (60 s, or the hold duration if it is shorter and not 0). A hold that starts a new deadline SHALL be reported even inside the interval; a hold that joins an active hold of the same pair SHALL NOT be reported again. The gateway SHALL NOT report refused host:port pairs.

#### Scenario: Retry after a timed-out hold
- **WHEN** the hold of `pypi.org:443` timed out at 10:01:00 and the run connects again at 10:01:05
- **THEN** the hold is reported again and the same pending request carries the deadline 10:02:05

#### Scenario: Hold after an immediate denial
- **WHEN** a run's `CONNECT pypi.org:443` is denied at once at 10:00:00 because its hold limit is full, and a retry is held at 10:00:01 with hold duration 60 s
- **THEN** the hold is reported, and the pending request carries the deadline 10:01:01

### Requirement: Denied requests refuse the host for the task
Denying a `network-access` request SHALL publish an `egress_policy` notification in the same transaction, so the host and port are refused for every run of the task, until an entry allows them.

#### Scenario: Deny then allow
- **WHEN** the request of `DF-7` for `example.com:443` is denied, and later a board member adds a task entry for `example.com`
- **THEN** runs of `DF-7` may connect to `example.com:443`
