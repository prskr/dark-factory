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
`create_issue` SHALL NOT create an issue itself. It SHALL raise a non-blocking approval request of type `create-issue` (see `agent-requests`) with `title`, Markdown `body` and `labels` as payload, and return the request id. It SHALL take no repository argument and SHALL NOT link the issue to any task.

#### Scenario: Description guidance
- **WHEN** the tool descriptors are built
- **THEN** the `create_issue` description says nothing is posted before approval, to put no secrets and no workspace data into the issue, and to pass the issue number to `create_task` with `issue`

#### Scenario: Request raised
- **WHEN** a chat agent calls `create_issue` with title "Flaky login test", a body and label `bug`
- **THEN** a pending `create-issue` request with exactly that title, body and labels exists, no issue exists yet, and the result contains the request id

#### Scenario: Not linked
- **WHEN** an issue was created after approval
- **THEN** the chat task's linked issue is unchanged

#### Scenario: Invalid arguments
- **WHEN** the title is empty
- **THEN** the result has `is_error: true` and no request is stored

### Requirement: create_task links an issue
`create_task` SHALL accept an optional `issue` (issue number, at least 1) and link the created task to it. Before creating the task it SHALL check with `issues.get` on the run's project repository that the issue exists and is not a pull request; otherwise it SHALL return an `is_error` result naming the problem and create no task. It SHALL work in every run that is offered `create_task` and SHALL NOT change anything on the forge.

#### Scenario: Issue linked
- **WHEN** the agent calls `create_task` with persona "Linus" and `issue: 42`, and #42 is an issue of the project's repository
- **THEN** the new task has issue #42 as its linked issue

#### Scenario: Pull request number
- **WHEN** the agent passes `issue: 40` and #40 is a pull request
- **THEN** the result has `is_error: true` saying #40 is a pull request, and no task is created

#### Scenario: Unknown issue
- **WHEN** the agent passes `issue: 9999` and the repository has no such number
- **THEN** the result has `is_error: true` saying the issue does not exist, and no task is created

#### Scenario: From a regular task
- **WHEN** the agent of a regular task calls `create_task` with `issue: 42`
- **THEN** the child task is linked to #42 as in a chat

#### Scenario: Child comments on the issue
- **WHEN** the child of the first scenario calls `comment_on_issue`
- **THEN** the comment is posted on #42

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
