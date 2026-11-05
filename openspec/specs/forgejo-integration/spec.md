# forgejo-integration Specification

## Purpose

The Forgejo plugin implements pull requests and issues for Forgejo behind the forge-neutral `darkfactory:integrations@0.1.0` interfaces, hiding Forgejo specifics such as its work-in-progress title prefix.

## Requirements

### Requirement: Create pull request
`create` SHALL open a PR from head into base with title and body via the Forgejo API. With `draft: true` it SHALL apply Forgejo's work-in-progress marker; the returned record SHALL have `draft: true` and the title without the marker. An existing open PR for the same head and base SHALL yield `conflict`.

#### Scenario: Draft PR
- **WHEN** `create` is called with title "DF-1: Fix login" and `draft: true`
- **THEN** Forgejo shows a work-in-progress PR and the result has title "DF-1: Fix login" and `draft: true`

#### Scenario: Duplicate
- **WHEN** an open PR for `factory/task/DF-1 → main` exists and `create` is called again
- **THEN** the result is `conflict`

### Requirement: Find open pull request
`find-open` SHALL return the open PR from head into base if one exists, else none.

#### Scenario: Recover after crash
- **WHEN** a PR was created but not recorded
- **THEN** `find-open` returns it with number, URL and head SHA

### Requirement: Update pull request
`update` SHALL change only the given fields; `draft: false` SHALL remove the work-in-progress marker, `draft: true` add it, and a new title SHALL keep the current draft state.

#### Scenario: Ready for review
- **WHEN** `update` is called with `draft: false`
- **THEN** the PR title no longer has the marker and Forgejo no longer treats it as work in progress

#### Scenario: Retitle a draft
- **WHEN** a draft PR gets the new title "DF-1: Fix login redirect"
- **THEN** it remains a draft with that title

### Requirement: Issues
`get` SHALL return an issue's title, body, state, labels and URL; `add-comment` SHALL add a Markdown comment and return its id and URL. PR numbers SHALL be accepted as issue numbers.

#### Scenario: Comment on a PR
- **WHEN** `add-comment` is called with a PR's number
- **THEN** the comment appears on the PR

### Requirement: Error mapping
HTTP 404 SHALL map to `not-found`, 401/403 to `unauthorized`, 409 and "already exists" responses to `conflict`, 422 to `invalid-input`, 429 to `rate-limited` (with `Retry-After` if present), other non-2xx to `upstream`, missing `base-url` to `config`.

#### Scenario: Bad token
- **WHEN** the injected token is invalid
- **THEN** every call returns `unauthorized`

### Requirement: Forgejo webhook signature
`handle-webhook` SHALL hex-decode the `X-Forgejo-Signature` header and check it through the host. A missing or malformed header SHALL fail the check.

#### Scenario: Missing header
- **WHEN** a request has no `X-Forgejo-Signature`
- **THEN** the host answers `401`

### Requirement: Forgejo webhook events
`handle-webhook` SHALL map Forgejo events, by `X-Forgejo-Event` and payload, to the normalized events with the repository URL, the actor's login and, where Forgejo provides it, the head branch: a submitted review, a closed PR, a failed Actions run, and an issue or PR comment with its mentioned logins. Every other event SHALL yield no events.

#### Scenario: Commit status from an external CI
- **WHEN** an external CI sets a commit status with state `failure` on `factory/task/DF-1`
- **THEN** Forgejo 15 sends no webhook, so no task is woken

#### Scenario: Single review comment
- **WHEN** the recorded payload of a single line comment arrives (Forgejo sends it as a comment review without body)
- **THEN** the result is one review submitted event with verdict comment

#### Scenario: Review submitted with changes requested
- **WHEN** the recorded payload of a review requesting changes on the PR from `factory/task/DF-1` arrives
- **THEN** the result is one review submitted event with verdict changes requested, head `factory/task/DF-1` and the reviewer's login

#### Scenario: Merged PR
- **WHEN** the recorded payload of a merged PR arrives
- **THEN** the result is one pull request closed event with merged set

#### Scenario: Mention in a PR comment
- **WHEN** a comment "@factory-bot please rebase" on PR #12 arrives
- **THEN** the result is one comment event for #12, marked as a pull request, with mentioned login `factory-bot`

#### Scenario: Failed Actions run on a push
- **WHEN** the recorded payload of a failed Actions run triggered by a push to `factory/task/DF-3` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-3` with the workflow file as check name and the run URL

#### Scenario: Failed Actions run on a pull request
- **WHEN** the recorded payload of a failed Actions run triggered by the PR from `factory/task/DF-3` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-3`, taken from the run's event payload

#### Scenario: Unhandled event
- **WHEN** a `push` event arrives
- **THEN** the result is an empty list

#### Scenario: Pull request from a fork
- **WHEN** a review, close or failed Actions run payload arrives whose PR head repository (`head.repo.id`) differs from the event's repository or is null
- **THEN** the result is an empty list

### Requirement: Forgejo bot login and author role
`whoami` SHALL return the login of the account owning the plugin's credential. `author-role` SHALL return `bot` for that account, `member` for write, admin or owner permission on the repository, and `outside` otherwise.

#### Scenario: Reader
- **WHEN** `author-role` is called for a user with read permission
- **THEN** the result is `outside`
