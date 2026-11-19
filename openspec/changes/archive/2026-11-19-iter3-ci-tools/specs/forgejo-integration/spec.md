# Spec Delta

## ADDED Requirements

### Requirement: List open pull requests with filters
`list-open` SHALL additionally filter by `head-prefix`, `author`, `label` and `base`, all optional and combined with AND. The head prefix SHALL be matched on the plugin side. Filtering by author SHALL require Forgejo v10 or later and SHALL otherwise fail as `unsupported`.

#### Scenario: Prefix and label
- **WHEN** `list-open` is called with head prefix `renovate/` and label `major`, and #40 (`renovate/a`, label `major`) and #41 (`renovate/b`, no label) are open
- **THEN** the result contains only #40

### Requirement: CI status from commit statuses
`ci.status` SHALL read the combined commit status of the ref and return one check per status context with its state and target URL. For a check created by Forgejo Actions on v16 or later, it SHALL include the job id resolved through the Actions API.

#### Scenario: Actions job failed
- **WHEN** the head of PR #40 has status "ci / test (pull_request)" with state `failure` from Forgejo Actions on v16
- **THEN** `ci.status` returns state `failure` and that check with its URL and job id

#### Scenario: Older Forgejo
- **WHEN** the same status exists on Forgejo v15
- **THEN** the check is returned with state and URL and without a job id

### Requirement: CI job logs
`ci.job-log` SHALL return the plain-text log of the given Actions job, at most its last 4 MiB, through the Actions API of Forgejo v16 or later. On an older Forgejo it SHALL fail as `unsupported` with a message naming v16.

#### Scenario: Log read
- **WHEN** `ci.job-log` is called for job 812 on Forgejo v16
- **THEN** the job's plain-text log is returned

#### Scenario: Forgejo v15
- **WHEN** `ci.job-log` is called on Forgejo v15
- **THEN** the call fails as `unsupported` naming Forgejo v16
