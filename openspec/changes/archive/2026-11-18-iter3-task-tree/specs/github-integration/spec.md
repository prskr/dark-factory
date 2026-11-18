# Spec Delta

## ADDED Requirements

### Requirement: Merge a pull request
`merge` SHALL merge an open pull request only if its head is still the given head SHA, using the repository's merge style: `merge` if the repository allows merge commits, else `squash`, else `rebase`. A moved head, a merge conflict or a failed required check SHALL be returned as `conflict` or `invalid-input` with GitHub's message; an already merged pull request with the same head SHALL count as success.

#### Scenario: Merged
- **WHEN** `merge` is called for open PR #12 with its current head SHA
- **THEN** PR #12 is merged and the call succeeds

#### Scenario: Head moved
- **WHEN** `merge` is called with a head SHA that is no longer PR #12's head
- **THEN** the call fails and PR #12 stays open

#### Scenario: Already merged
- **WHEN** `merge` is called again for PR #12 after it was merged with that head
- **THEN** the call succeeds

### Requirement: Create a branch
`create-branch` SHALL create the branch `name` at the tip of the branch `from`. If `name` already exists, it SHALL return `conflict` and leave the branch unchanged; an unknown `from` SHALL return `not-found` or `invalid-input`.

#### Scenario: Feature branch
- **WHEN** `create-branch` is called with name `factory/task/DF-30` and from `main`
- **THEN** `factory/task/DF-30` exists at the tip of `main`

#### Scenario: Branch exists
- **WHEN** `create-branch` is called for an existing `factory/task/DF-30`
- **THEN** the call fails with `conflict` and the branch is unchanged
