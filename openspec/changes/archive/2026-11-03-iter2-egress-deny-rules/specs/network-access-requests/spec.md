# Spec Delta

## MODIFIED Requirements

### Requirement: Requests from denials
The gateway SHALL report `not-allowlisted` denials to the control plane at most once per run, host and port per minute. For each report, the control plane SHALL raise a non-blocking `network-access` request on the run's task, raised by the control plane. It SHALL ignore the report if the run is no longer active, the task is completed or cancelled, the run's current effective allowlist already covers the host and port, or a deny rule matches the host.

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

### Requirement: Denial handling is serialized with entries and cancellation
Handling a denial report SHALL be serialized with `http-host` entry creation, with deny rule creation and with task cancellation. Every request raised from a denial SHALL therefore either be settled by a later entry or deny rule, or closed by a later cancellation, or not be raised at all.

#### Scenario: Concurrent entry and denial
- **WHEN** a project entry for `pypi.org` is created while a denial report for `pypi.org:443` of a `DF` task is handled
- **THEN** afterwards no pending `network-access` request for `pypi.org:443` exists on that task

#### Scenario: Concurrent deny rule and denial
- **WHEN** deny rule `*.datadoghq.com` is created while a denial report for `http-intake.logs.us5.datadoghq.com:443` of `DF-7` is handled
- **THEN** afterwards no pending `network-access` request for that host exists on `DF-7`

## ADDED Requirements

### Requirement: Agent requests for denied hosts are rejected
A `network-access` request raised by an agent for a host that a deny rule matches SHALL be rejected with an error stating that the host is denied by instance policy and must not be requested again. Nothing SHALL be stored.

#### Scenario: Agent asks for a denied host
- **WHEN** deny rule `*.datadoghq.com` exists and the agent of `DF-7` requests `network-access` for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the agent receives the error and no request exists on `DF-7`

### Requirement: New deny rules deny pending requests
When a deny rule is created by any path, every pending `network-access` request whose host the rule matches SHALL be denied by the system principal, on every port and every task, with a comment naming the deny rule, and its decision SHALL be delivered as a message.

#### Scenario: Pending requests on two tasks
- **WHEN** `DF-7` and `OPS-3` have pending requests for `http-intake.logs.us5.datadoghq.com:443` and deny rule `*.datadoghq.com` is added
- **THEN** both requests are `denied` with a comment naming `*.datadoghq.com` and both tasks receive a `request.decided` message

#### Scenario: Unrelated request
- **WHEN** `DF-7` has a pending request for `pypi.org:443` and deny rule `*.datadoghq.com` is added
- **THEN** the request stays pending
