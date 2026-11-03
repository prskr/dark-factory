# network-access-requests Specification

## Purpose
Turns denied egress into decisions: the agent or the control plane asks for a host, a human approves it at the narrowest fitting scope, and the running agent continues without a restart (ADR-0038, `design/egress.md` §8).

## Requirements

### Requirement: network-access approval type
The `network-access` type SHALL accept payload `{host, port}` validated like an `http-host` entry (port default 443), SHALL offer scopes task, project and instance, and SHALL use `host:port` as fingerprint.

#### Scenario: Invalid host
- **WHEN** an agent requests `network-access` for `10.0.0.5`
- **THEN** the request is rejected with a validation error and nothing is stored

### Requirement: Approval writes the entry
Approving a `network-access` request SHALL create the `http-host` entry at the chosen scope, audited with the deciding human, and SHALL publish the policy change.

#### Scenario: Approved for the task
- **WHEN** a board member approves `pypi.org:443` for task `DF-7` while its run is active
- **THEN** a task entry for `DF-7` exists and the run's next `CONNECT pypi.org:443` succeeds within about a second

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

### Requirement: New entries settle pending requests
When an `http-host` entry is created by any path, every pending `network-access` request whose host the entry's hostname rule matches (exactly, or as `*.suffix` for a wildcard entry), whose port equals the entry's port and whose task the entry's scope covers SHALL be approved with that scope, and its decision delivered as a message.

#### Scenario: Board member adds the entry by hand
- **WHEN** tasks `DF-7` and `DF-8` of project `DF` have pending requests for `pypi.org:443` and a board member adds a project entry `pypi.org`
- **THEN** both requests are `approved` with scope project and both tasks receive a `request.decided` message

#### Scenario: Wildcard entry
- **WHEN** `DF-7` has pending requests for `static.crates.io:443` and `index.crates.io:8443` and a project entry `*.crates.io` (port 443) is added
- **THEN** the request for `static.crates.io:443` is approved and the one for port 8443 stays pending

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
