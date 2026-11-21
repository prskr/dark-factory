# Spec Delta

## ADDED Requirements

### Requirement: List issues
`issues.list` SHALL return the repository's issues filtered by state, labels and free-text query, newest first, and SHALL NOT return pull requests. It SHALL return at most `limit` issues, an opaque `next-cursor` while more issues exist, and `truncated: true` if it stopped at its page limit of 5 forge pages before the last page. A `limit` of 0 SHALL yield `invalid-input`.

#### Scenario: Pull requests dropped
- **WHEN** the forge's issue list contains items with a `pull_request` key
- **THEN** those items are not in the result and the page is filled from the following forge page up to `limit`

#### Scenario: Text query
- **WHEN** `query` is "timeout"
- **THEN** the plugin searches with `is:issue repo:<owner>/<repo> timeout` and returns issues only

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
