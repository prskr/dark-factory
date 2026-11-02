# Spec Delta

## ADDED Requirements

### Requirement: Forgejo webhook signature
`handle-webhook` SHALL hex-decode the `X-Forgejo-Signature` header and check it through the host. A missing or malformed header SHALL fail the check.

#### Scenario: Missing header
- **WHEN** a request has no `X-Forgejo-Signature`
- **THEN** the host answers `401`

### Requirement: Forgejo webhook events
`handle-webhook` SHALL map Forgejo events, by `X-Forgejo-Event` and payload, to the normalized events with the repository URL, the actor's login and, where Forgejo provides it, the head branch: a submitted review, a closed PR, a failed commit status or Actions run, and an issue or PR comment with its mentioned logins. Every other event SHALL yield no events.

#### Scenario: Review submitted with changes requested
- **WHEN** the recorded payload of a review requesting changes on the PR from `factory/task/DF-1` arrives
- **THEN** the result is one review submitted event with verdict changes requested, head `factory/task/DF-1` and the reviewer's login

#### Scenario: Merged PR
- **WHEN** the recorded payload of a merged PR arrives
- **THEN** the result is one pull request closed event with merged set

#### Scenario: Mention in a PR comment
- **WHEN** a comment "@factory-bot please rebase" on PR #12 arrives
- **THEN** the result is one comment event for #12, marked as a pull request, with mentioned login `factory-bot`

#### Scenario: Failed commit status
- **WHEN** a commit status with state `failure` for branch `factory/task/DF-1` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-1` with the status context as check name

#### Scenario: Unhandled event
- **WHEN** a `push` event arrives
- **THEN** the result is an empty list

### Requirement: Forgejo bot login and author role
`whoami` SHALL return the login of the account owning the plugin's credential. `author-role` SHALL return `bot` for that account, `member` for write, admin or owner permission on the repository, and `outside` otherwise.

#### Scenario: Reader
- **WHEN** `author-role` is called for a user with read permission
- **THEN** the result is `outside`
