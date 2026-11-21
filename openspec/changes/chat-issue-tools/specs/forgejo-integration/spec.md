# Spec Delta

## ADDED Requirements

### Requirement: List issues
`issues.list` SHALL return the repository's issues filtered by state, labels and free-text query, newest first, using the issue listing with `type=issues`, and SHALL NOT return pull requests, also not items that carry a `pull_request` field. It SHALL return at most `limit` issues, an opaque `next-cursor` while more issues exist, and `truncated: true` if it stopped at its page limit of 5 forge pages before the last page. A `limit` of 0 SHALL yield `invalid-input`.

#### Scenario: Pull requests excluded
- **WHEN** the repository has issues and pull requests
- **THEN** the result contains only issues

#### Scenario: Closed issues
- **WHEN** the state filter is `closed`
- **THEN** only closed issues are returned

#### Scenario: Page limit
- **WHEN** more than 5 forge pages are needed to fill `limit`
- **THEN** the result has `truncated: true` and a `next-cursor`

### Requirement: Create issue
`issues.create` SHALL create an issue with the given title, Markdown body and labels and return it with its number and URL. Label names SHALL be resolved to Forgejo label ids; a name that does not exist SHALL yield `invalid-input` naming the label, and no issue SHALL be created.

#### Scenario: Issue with labels
- **WHEN** `create` is called with the existing labels `bug` and `ui`
- **THEN** the issue exists with both labels and the result has its URL

#### Scenario: Unknown label
- **WHEN** a label does not exist in the repository
- **THEN** `create` returns `invalid-input` naming the label

### Requirement: Pull request flag on issues
`issues.get` SHALL set `pull-request` to true if the number belongs to a pull request and to false otherwise; `list` and `create` SHALL return `false`.

#### Scenario: Pull request number
- **WHEN** `get` is called with the number of a pull request
- **THEN** the result has `pull-request: true`

#### Scenario: Issue number
- **WHEN** `get` is called with the number of an issue
- **THEN** the result has `pull-request: false`
