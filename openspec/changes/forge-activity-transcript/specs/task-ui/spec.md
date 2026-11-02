# Spec Delta

## ADDED Requirements

### Requirement: Forge activity as transcript rows
A forge activity event SHALL be shown as one line in the transcript in the notice row layout, tinted with the accent color and with its own icon: a commit icon for a push, a draft pull request icon for an opened draft PR, a pull request icon for a non-draft PR opened or marked ready. A rejected or failed push SHALL be tinted as an error and show the reason code. PR rows SHALL link to the PR URL.

#### Scenario: Accepted push row
- **WHEN** the transcript contains an accepted push of `factory/task/DF-1` from `abc1234` to `def5678`
- **THEN** one row with the commit icon shows the branch and `abc1234..def5678`

#### Scenario: Rejected push row
- **WHEN** the transcript contains a push rejected with `df:non-fast-forward`
- **THEN** the row is tinted as an error and shows `df:non-fast-forward`

#### Scenario: Draft PR row
- **WHEN** draft PR #12 is opened while the page is open
- **THEN** a row with the draft pull request icon and a link to PR #12 appears without a reload, in order with the agent's output

#### Scenario: PR ready row
- **WHEN** the transcript contains PR #12 marked ready
- **THEN** one row with the pull request icon says PR #12 is ready for review and links to it

### Requirement: Forge activity links are safe
A PR link in a forge activity row SHALL only be rendered for an `http` or `https` URL; any other URL SHALL be shown as text without a link.

#### Scenario: Unexpected scheme
- **WHEN** a forge activity event carries the URL `javascript:alert(1)`
- **THEN** the row shows the PR number without a link
