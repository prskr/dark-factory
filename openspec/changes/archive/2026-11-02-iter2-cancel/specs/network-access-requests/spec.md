# Spec Delta

## MODIFIED Requirements

### Requirement: Requests from denials
The gateway SHALL report `not-allowlisted` denials to the control plane at most once per run, host and port per minute. For each report, the control plane SHALL raise a non-blocking `network-access` request on the run's task, raised by the control plane. It SHALL ignore the report if the run is no longer active, the task is completed or cancelled, or the run's current effective allowlist already covers the host and port.

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
