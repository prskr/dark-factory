# Spec Delta

## ADDED Requirements

### Requirement: List open pull requests with filters
`list-open` SHALL additionally filter by `head-prefix`, `author`, `label` and `base`, all optional and combined with AND. `base` SHALL be passed to the API; the head prefix, author and label SHALL be matched on the plugin side.

#### Scenario: Author filter
- **WHEN** `list-open` is called with author `renovate[bot]`, and #40 by `renovate[bot]` and #44 by `alice` are open
- **THEN** the result contains only #40

### Requirement: CI status from checks and statuses
`ci.status` SHALL combine the check runs of the ref (latest per name) and the legacy commit statuses into one list of checks. A check run of GitHub Actions SHALL carry its job id. The combined state SHALL be `failure` if any check failed, `pending` if any is still running, `success` if all succeeded, and `none` without checks.

#### Scenario: Actions and an external status
- **WHEN** the ref has the Actions check run "test" failed and an external commit status "deploy-preview" succeeded
- **THEN** `ci.status` returns state `failure`, "test" with its job id and "deploy-preview" without one

### Requirement: CI job logs
`ci.job-log` SHALL return the log of the given Actions job, at most its last 4 MiB, with the leading timestamp of every line and a leading byte order mark removed.

#### Scenario: Timestamps removed
- **WHEN** the log line is `2026-11-03T10:00:00.1234567Z error[E0425]: cannot find value`
- **THEN** the returned line is `error[E0425]: cannot find value`
