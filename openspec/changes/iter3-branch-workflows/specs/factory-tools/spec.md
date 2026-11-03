# Spec Delta

## MODIFIED Requirements

### Requirement: update_pull_request
`update_pull_request` SHALL store the given title and/or body on the task and, if the PR exists, update it through the forge integration. If the task's PR is attached, the call SHALL fail with an error explaining that the PR belongs to someone else, and nothing SHALL be stored.

#### Scenario: Before the PR exists
- **WHEN** the agent sets a title before the first push
- **THEN** the call succeeds and the title is used when the draft PR is opened

#### Scenario: Attached PR
- **WHEN** the agent of `DF-13`, attached to Renovate's PR #40, calls `update_pull_request` with a new title
- **THEN** the result has `is_error: true` and PR #40 is unchanged

### Requirement: create_task
`create_task` SHALL create a task in the run's project with `persona` (name), `title`, `prompt`, optional `depends_on` (list of `{task, kind}`, task given by key) and optional `branch` (an existing branch, see `task-branches`), with the run's task as parent and its agent as creator, and SHALL return the new task's key and status. The title SHALL be sanitized and stored with source `agent`. An unknown persona, an empty prompt or title, an invalid dependency or a rejected branch SHALL produce an error result and create nothing.

#### Scenario: Review task for an implementation
- **WHEN** the agent of `DF-19` calls `create_task` with persona "Reviewer", title "Review DF-20", a prompt and `depends_on: [{task: "DF-20", kind: "completion"}]` while `DF-20` is `running`
- **THEN** the result contains key `DF-21` and status `awaiting_dependencies`, and `DF-21` has parent `DF-19`

#### Scenario: Unknown persona
- **WHEN** `create_task` names persona "Nobody"
- **THEN** the result has `is_error: true` naming the persona and no task is created

#### Scenario: Retried call
- **WHEN** the runtime re-sends `create_task` with the same `call_id` after a reconnect
- **THEN** exactly one task exists and both requests get the same key

#### Scenario: Fan-out over a Renovate branch
- **WHEN** the agent of `DF-10` calls `create_task` with `branch: "renovate/serde-1.x"`, which has open PR #40
- **THEN** the new task holds `renovate/serde-1.x`, is attached to PR #40 and has parent `DF-10`

#### Scenario: Branch held by another task
- **WHEN** `create_task` names `branch: "renovate/serde-1.x"` while `DF-13` holds it
- **THEN** the result has `is_error: true` naming `DF-13` and no task is created
