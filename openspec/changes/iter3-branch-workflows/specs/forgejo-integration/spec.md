# Spec Delta

## ADDED Requirements

### Requirement: List open pull requests by head
`list-open` SHALL return every open pull request of the repository whose head branch in that repository equals the filter's head branch, following Forgejo's pagination. Pull requests from forks SHALL NOT match.

#### Scenario: One open PR
- **WHEN** `list-open` is called with head `renovate/serde-1.x` and PR #40 `renovate/serde-1.x → main` is open
- **THEN** the result contains only PR #40

#### Scenario: Closed PR
- **WHEN** the only PR from `fix/login` is closed
- **THEN** `list-open` with head `fix/login` returns an empty list

#### Scenario: Several bases
- **WHEN** PRs #42 `fix/shared → main` and #43 `fix/shared → release/1.2` are open
- **THEN** `list-open` with head `fix/shared` returns both
