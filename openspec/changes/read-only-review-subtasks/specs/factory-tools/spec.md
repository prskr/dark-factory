# Spec Delta

## ADDED Requirements

### Requirement: create_task for a read-only task
`create_task` SHALL accept an optional boolean `read_only` (default false). With `read_only: true` it SHALL create a task of kind `task` with a read-only workspace. `read_only: true` without `base_task` SHALL produce an error result saying that a read-only task needs a base task, and create nothing. A board member SHALL NOT be able to create a read-only task in the UI.

#### Scenario: Review subtask
- **WHEN** the agent of `DF-19` calls `create_task` with persona "Grace Hopper", a prompt, `base_task: "DF-20"` and `read_only: true`
- **THEN** a task `DF-21` of kind `task` exists with parent `DF-19`, base task `DF-20` and a read-only workspace

#### Scenario: Without a base task
- **WHEN** `create_task` names `read_only: true` and no `base_task`
- **THEN** the result has `is_error: true` saying a read-only task needs a base task, and no task is created

#### Scenario: Default
- **WHEN** `create_task` names `base_task: "DF-20"` and no `read_only`
- **THEN** the new task has a writable workspace and its own task branch

### Requirement: Tools of a read-only task
A run of a task of kind `task` with a read-only workspace SHALL be offered every factory tool v2 except `update_pull_request` and `merge_subtask_pr`. A call to one of these two SHALL be answered with a tool error saying the tool is not available in a read-only task, and SHALL change nothing.

#### Scenario: Tools listed
- **WHEN** a run of read-only task `DF-21` connects
- **THEN** `Assign.tools` contains `complete_task`, `comment_on_pull_request`, `read_pull_request_feedback`, `read_ci_status`, `create_task` and `write_artifact`, and contains neither `update_pull_request` nor `merge_subtask_pr`

#### Scenario: Update a PR
- **WHEN** the agent of read-only task `DF-21` calls `update_pull_request`
- **THEN** the result has `is_error: true` and no title or body is stored on any task

### Requirement: Pull request of a read-only task
For a task with a read-only workspace, `read_pull_request_feedback` and `comment_on_pull_request` SHALL act on the pull request of its base task. If the base task has no pull request, both SHALL fail with an error result saying no pull request was opened yet.

#### Scenario: Review comment on the implementation's PR
- **WHEN** the agent of read-only task `DF-21` (base task `DF-20` with PR #12) calls `comment_on_pull_request` with body "Two findings"
- **THEN** a PR comment "Two findings" exists on PR #12

#### Scenario: Reply in a review thread
- **WHEN** the agent of `DF-21` passes the id of a review thread on PR #12
- **THEN** the reply appears in that thread

#### Scenario: Earlier feedback on a second review
- **WHEN** the agent of `DF-21` calls `read_pull_request_feedback` while PR #12 has its own earlier comment and the implementer's replies
- **THEN** the result contains both, its own comment with role `bot`

#### Scenario: Base task without a PR
- **WHEN** the agent of `DF-21` calls `comment_on_pull_request` and `DF-20` has no PR yet
- **THEN** the result has `is_error: true` with "no pull request opened yet"
