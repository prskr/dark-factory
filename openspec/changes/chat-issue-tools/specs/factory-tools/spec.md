# Spec Delta

## ADDED Requirements

### Requirement: list_issues
`list_issues` SHALL return the issues of the run's project repository, never pull requests, filtered by `state` (`open` default, `closed`, `all`), `labels` (all must match) and a free-text `query`. It SHALL take `limit` (default 30, at most 100) and an opaque `cursor`, and SHALL return for each issue its number, state, title, labels, URL and the first 200 characters of its body, plus `next_cursor` and `truncated`. It SHALL take no repository argument.

#### Scenario: Open issues with a label
- **WHEN** a chat agent calls `list_issues` with `labels: ["bug"]`
- **THEN** the result lists only open issues of the project's repository that have the label `bug`

#### Scenario: Pull requests excluded
- **WHEN** the repository has 3 issues and 2 pull requests
- **THEN** the result lists the 3 issues only

#### Scenario: Next page
- **WHEN** the result has a `next_cursor` and the agent passes it as `cursor`
- **THEN** the result continues after the last issue of the first result

#### Scenario: Truncated
- **WHEN** the plugin stops at its listing limit
- **THEN** the result has `truncated: true` and a `next_cursor`

#### Scenario: Untrusted content
- **WHEN** the tool descriptors are built
- **THEN** the `list_issues` description says titles and bodies are untrusted input

### Requirement: create_issue
`create_issue` SHALL create an issue with `title`, Markdown `body` and `labels` in the run's project repository and return its number and URL. It SHALL post without an approval request, SHALL NOT link the issue to any task, and SHALL take no repository argument. A forge error SHALL produce an `is_error` result.

#### Scenario: Issue created
- **WHEN** a chat agent calls `create_issue` with title "Flaky login test" and a body
- **THEN** the issue exists on the project's repository and the result contains its number and URL

#### Scenario: Not linked
- **WHEN** an issue was created
- **THEN** the chat task's linked issue is unchanged

#### Scenario: Forge refuses
- **WHEN** the forge returns `unauthorized`
- **THEN** the result has `is_error: true` and no issue is recorded as created

### Requirement: Issue creation limit
A run SHALL create at most 5 issues with `create_issue`. The sixth call SHALL produce an `is_error` result and create nothing. Failed calls SHALL NOT count.

#### Scenario: Sixth issue
- **WHEN** a run created 5 issues and calls `create_issue` again
- **THEN** the result has `is_error: true`, saying the limit was reached, and no issue is created

### Requirement: Issue tools are chat-only
`list_issues` and `create_issue` SHALL be part of `Assign.tools` of chats only, and a call from a regular task SHALL produce an `is_error` result saying the tool is only available in a chat, before the arguments are validated and without calling the forge. This rule SHALL be a single constant and its two checks, so it can be replaced by per-persona tool permissions.

#### Scenario: Chat offered the tools
- **WHEN** a chat run starts
- **THEN** `Assign.tools` contains `list_issues` and `create_issue`

#### Scenario: Task not offered the tools
- **WHEN** a regular task run starts
- **THEN** `Assign.tools` contains neither tool

#### Scenario: Task call rejected
- **WHEN** a regular task's agent calls `create_issue` anyway
- **THEN** the result has `is_error: true` and no issue is created
