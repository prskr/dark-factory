# Spec Delta

## Purpose

The Forgejo plugin implements pull requests and issues for Forgejo behind the forge-neutral `darkfactory:integrations@0.1.0` interfaces, hiding Forgejo specifics such as its work-in-progress title prefix.

## ADDED Requirements

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
