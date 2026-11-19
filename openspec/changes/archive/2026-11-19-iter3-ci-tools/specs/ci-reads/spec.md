# Spec Delta

## Purpose

Lets agents survey the open pull requests of their project and read CI status and job logs of any ref or pull request in the project repository, read-only, through forge plugin capabilities (ADR-0033).

## ADDED Requirements

### Requirement: list_pull_requests
`list_pull_requests` SHALL return the open pull requests of the run's project repository matching every given filter (`head_prefix`, `author`, `label`, `base`), each with number, title, head, base, author login, labels, draft flag, head SHA and URL, ordered by number.

#### Scenario: Renovate survey
- **WHEN** PRs #40 and #41 from `renovate/…` by `renovate-bot` and #44 from `fix/login` by `alice` are open, and the agent calls `list_pull_requests` with `head_prefix: "renovate/"`
- **THEN** the result contains #40 and #41 and not #44

#### Scenario: No filter
- **WHEN** the agent calls `list_pull_requests` without arguments
- **THEN** the result contains every open PR of the repository

### Requirement: Pull request list size limit
`list_pull_requests` SHALL return at most 100 pull requests and SHALL set `truncated: true` when more match; otherwise `truncated` SHALL be `false`.

#### Scenario: Large backlog
- **WHEN** 130 open PRs match
- **THEN** the result holds 100 PRs and `truncated: true`

### Requirement: read_ci_status
`read_ci_status` SHALL take exactly one of `pull_request` (number) or `ref` (branch, tag or commit SHA) of the run's project repository and return the combined state (`success`, `pending`, `failure` or `none`) and one entry per check with name, state, URL and, if the forge provides it, a job id. For a pull request, the state of its current head SHA SHALL be returned.

#### Scenario: Failing PR
- **WHEN** PR #40's head has check "ci / test" failed and "ci / lint" succeeded
- **THEN** the result has state `failure` and both checks, "ci / test" with state `failure` and a job id

#### Scenario: No CI
- **WHEN** the ref has no checks
- **THEN** the result has state `none` and no checks

#### Scenario: Both arguments
- **WHEN** `read_ci_status` is called with `pull_request` and `ref`
- **THEN** the result has `is_error: true` and the forge is not called

### Requirement: read_ci_log
`read_ci_log` SHALL return the log of a CI job of the run's project repository as text without ANSI escape sequences, together with its total size in bytes. Without `offset` it SHALL return the last 64 KiB, with `offset` the 64 KiB starting at that byte, cut at UTF-8 character boundaries.

#### Scenario: Tail of a failed job
- **WHEN** job 812's log has 300 KiB and the agent calls `read_ci_log` with `job: 812`
- **THEN** the result holds the last 64 KiB, the total size, and no ANSI escape sequences

#### Scenario: Earlier page
- **WHEN** the agent calls `read_ci_log` with `job: 812` and `offset: 0`
- **THEN** the result holds the first 64 KiB

#### Scenario: Log no longer available
- **WHEN** the forge has expired the log of job 812
- **THEN** the result has `is_error: true` saying the log is not available

### Requirement: Long logs keep their end
When a job log is larger than 4 MiB, only its last 4 MiB SHALL be available. The total size and offsets SHALL refer to that part, and the result SHALL say that the beginning was dropped.

#### Scenario: Huge log
- **WHEN** job 813's log has 20 MiB
- **THEN** `read_ci_log` reports a total size of 4 MiB and that the beginning was dropped

### Requirement: CI reads need forge support
If the project's forge plugin doesn't export `ci`, or the forge doesn't support an operation (Forgejo before v16 for logs), the tools SHALL fail with `is_error: true` naming what isn't supported. They SHALL NOT end the run.

#### Scenario: Old Forgejo
- **WHEN** the project's Forgejo is v15 and the agent calls `read_ci_log`
- **THEN** the result has `is_error: true` saying job logs need Forgejo v16 or later

### Requirement: CI reads stay in the project repository
The tools SHALL read only the run's project repository. A pull request number, ref or job id that doesn't exist there SHALL produce an error result.

#### Scenario: Unknown job
- **WHEN** the agent calls `read_ci_log` with a job id from another repository
- **THEN** the result has `is_error: true` and no log content
