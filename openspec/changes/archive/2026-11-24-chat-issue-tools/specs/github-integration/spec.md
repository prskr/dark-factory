# Spec Delta

## ADDED Requirements

### Requirement: List issues
`issues.list` SHALL return the repository's issues filtered by state, labels and free-text query, newest first, and SHALL NOT return pull requests. The `query` SHALL be literal text, never search syntax; double quotes in it SHALL be replaced by spaces. A `limit` of 0 SHALL yield `invalid-input`.

#### Scenario: Pull requests dropped
- **WHEN** the forge's issue list contains items with a `pull_request` key
- **THEN** those items are not in the result and the page is filled from the following forge page up to `limit`

#### Scenario: Text query
- **WHEN** `query` is "timeout"
- **THEN** the plugin searches with `is:issue repo:<owner>/<repo>` and the phrase "timeout" and returns issues only

#### Scenario: Quotes in the query
- **WHEN** `query` is `foo "bar"`
- **THEN** the plugin searches for the phrase `foo bar`

#### Scenario: Qualifier in the query
- **WHEN** `query` is `repo:other/private is:pr`
- **THEN** it is searched as literal text and every returned issue belongs to the project's repository

### Requirement: List issues paging
`issues.list` SHALL return at most `limit` issues and an opaque `next-cursor` whenever a further page can be read. The cursor SHALL resume after the last returned item, also inside a forge page; concurrent changes to the issues MAY make it skip or repeat one. `truncated` SHALL be `true` if more matching issues exist than were returned (`limit`, page limit of 5, search cap of 1,000, `incomplete_results`) and `false` otherwise.

#### Scenario: Limit reached
- **WHEN** 100 issues match and `limit` is 30
- **THEN** 30 issues are returned with `truncated: true` and a `next-cursor`

#### Scenario: Everything fits
- **WHEN** 20 issues match and `limit` is 30
- **THEN** 20 issues are returned with `truncated: false` and no `next-cursor`

#### Scenario: Incomplete results
- **WHEN** GitHub reports `incomplete_results: true` and a further page is readable
- **THEN** the result has `truncated: true` and a `next-cursor`

#### Scenario: Search cap
- **WHEN** more than 1,000 issues match
- **THEN** the result has `truncated: true` and, once the 1,000th result is returned, no `next-cursor`

#### Scenario: Cursor inside a page
- **WHEN** a call returns 10 of the 30 issues left on a forge page after PR filtering
- **THEN** the next call with its cursor starts at the 11th and repeats none

#### Scenario: Page limit
- **WHEN** more than 5 forge pages are needed to fill `limit`
- **THEN** the result has `truncated: true` and a `next-cursor` that continues after the last returned issue

### Requirement: Create issue
`issues.create` SHALL create an issue with the given title, Markdown body and labels and return it with its number and URL. A label that does not exist SHALL yield `invalid-input` naming the label, and no issue SHALL be created.

#### Scenario: Issue with labels
- **WHEN** `create` is called with labels `bug` and `ui`
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
