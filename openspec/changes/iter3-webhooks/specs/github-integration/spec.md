# Spec Delta

## ADDED Requirements

### Requirement: GitHub webhook signature
`handle-webhook` SHALL strip the `sha256=` prefix of `X-Hub-Signature-256`, hex-decode the rest and check it through the host. A missing header, a missing prefix or bad hex SHALL fail the check.

#### Scenario: Legacy SHA-1 header only
- **WHEN** a request carries only `X-Hub-Signature`
- **THEN** the host answers `401`

### Requirement: GitHub webhook events
`handle-webhook` SHALL map GitHub events, by `X-GitHub-Event` and payload, to the normalized events with the repository URL, the actor's login and, where GitHub provides it, the head branch: `pull_request_review` `submitted`, `pull_request` `closed`, `check_suite` completed with `failure` or `timed_out`, `status` with `failure` or `error`, and `issue_comment` `created` with its mentioned logins. Every other event or action SHALL yield no events.

#### Scenario: Review submitted
- **WHEN** GitHub's documented `pull_request_review` example with action `submitted` and state `approved` arrives
- **THEN** the result is one review submitted event with verdict approved

#### Scenario: Failed check suite
- **WHEN** a `check_suite` event with conclusion `failure` on head branch `factory/task/DF-1` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-1`

#### Scenario: Merged pull request
- **WHEN** a `pull_request` event with action `closed` and `merged: true` arrives
- **THEN** the result is one pull request closed event with merged set

#### Scenario: Ping
- **WHEN** a `ping` event arrives
- **THEN** the result is an empty list

### Requirement: GitHub bot login and author role
`whoami` SHALL return the login of the account owning the plugin's credential. `author-role` SHALL return `bot` for that account, `member` for write, maintain or admin permission on the repository, and `outside` otherwise.

#### Scenario: Triage permission
- **WHEN** `author-role` is called for a user with triage permission
- **THEN** the result is `outside`
