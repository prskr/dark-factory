# factory-tools Specification

## Purpose

The factory tools v1 (`contract/mcp/factory-tools.v1.json`) let an agent act on Dark Factory's state for its own task only; the control plane executes them exactly once per call (ADR-0027).

## Requirements

### Requirement: Calls scoped to the run's task
A tool call SHALL be executed only if it arrives on the stream of an active run and names a tool from that run's `Assign.tools`; it SHALL act only on that run's task, project, PR and linked issue. Invalid arguments per the tool's JSON Schema SHALL produce an error result explaining the problem.

#### Scenario: Unknown tool
- **WHEN** a call names `delete_project`
- **THEN** the result has `is_error: true` and nothing is executed

#### Scenario: Schema violation
- **WHEN** `report_progress` is called without `summary`
- **THEN** the result has `is_error: true` naming the missing field

### Requirement: Exactly-once execution
Each `call_id` SHALL be executed at most once; a repeated request with the same `call_id` SHALL receive the stored result. Every executed call SHALL be stored with arguments and result and audited with the agent as actor.

#### Scenario: Retry after reconnect
- **WHEN** the runtime re-sends `comment_on_issue` with the same `call_id` after a reconnect
- **THEN** only one comment exists on the issue and both requests get the same result

### Requirement: update_pull_request
`update_pull_request` SHALL store the given title and/or body on the task and, if the PR exists, update it through the forge integration.

#### Scenario: Before the PR exists
- **WHEN** the agent sets a title before the first push
- **THEN** the call succeeds and the title is used when the draft PR is opened

### Requirement: comment_on_issue
`comment_on_issue` SHALL add the body as a comment on the task's linked issue, and SHALL fail with an explanatory error if the task has no linked issue.

#### Scenario: No linked issue
- **WHEN** the task has no issue and the agent calls `comment_on_issue`
- **THEN** the result has `is_error: true` with "this task has no linked issue"

### Requirement: report_progress
`report_progress` SHALL set the task's status line and publish a `task_changed` notification.

#### Scenario: Progress shown
- **WHEN** the agent reports "tests pass"
- **THEN** the task's status line is "tests pass"

### Requirement: complete_task
`complete_task` SHALL store the summary on the task and return a result with `end_run_after_turn: true`.

#### Scenario: Completion
- **WHEN** the agent calls `complete_task`
- **THEN** the result tells the runtime to finalize after the current turn

### Requirement: Integration failures are tool errors
Forge errors SHALL be returned as `is_error: true` results with a message the model can act on (e.g. rate limited, retry later); they SHALL NOT end the run.

#### Scenario: Forge unreachable
- **WHEN** Forgejo is down during `comment_on_issue`
- **THEN** the result has `is_error: true` and the run continues
