# Spec Delta

## ADDED Requirements

### Requirement: List issues
`issues.list` SHALL return the repository's issues filtered by state, labels and free-text query, newest first, using the issue listing with `type=issues`, and SHALL NOT return pull requests, also not items that carry a `pull_request` field. A `limit` of 0 SHALL yield `invalid-input`.

#### Scenario: Pull requests excluded
- **WHEN** the repository has issues and pull requests
- **THEN** the result contains only issues

#### Scenario: Qualifier in the query
- **WHEN** `query` is `repo:other/private`
- **THEN** it is sent as plain `q` text to the project's repository only

#### Scenario: Closed issues
- **WHEN** the state filter is `closed`
- **THEN** only closed issues are returned

### Requirement: List issues paging
`issues.list` SHALL return at most `limit` issues and an opaque `next-cursor` while more issues exist. The cursor SHALL resume after the last returned item, also inside a forge page; issues created, closed or deleted between two calls MAY make it skip or repeat one. `truncated` SHALL be `true` if more matching issues exist than were returned (`limit` reached or page limit of 5) and `false` otherwise.

#### Scenario: Limit reached
- **WHEN** 100 issues match and `limit` is 30
- **THEN** 30 issues are returned with `truncated: true` and a `next-cursor`

#### Scenario: Everything fits
- **WHEN** 20 issues match and `limit` is 30
- **THEN** 20 issues are returned with `truncated: false` and no `next-cursor`

#### Scenario: Cursor inside a page
- **WHEN** a call returns 10 of the 30 issues left on a forge page after PR filtering
- **THEN** the next call with its cursor starts at the 11th and repeats none

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
