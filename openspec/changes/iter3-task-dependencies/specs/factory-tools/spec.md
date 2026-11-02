# Spec Delta

## MODIFIED Requirements

### Requirement: Calls scoped to the run's task
A tool call SHALL be executed only if it arrives on the stream of an active run and names a tool from that run's `Assign.tools`; it SHALL act only on that run's task, project, PR and linked issue, except that `create_task` creates tasks in the run's project and `send_message` acts on the children of the run's task. Invalid arguments per the tool's JSON Schema SHALL produce an error result explaining the problem.

#### Scenario: Unknown tool
- **WHEN** a call names `delete_project`
- **THEN** the result has `is_error: true` and nothing is executed

#### Scenario: Schema violation
- **WHEN** `report_progress` is called without `summary`
- **THEN** the result has `is_error: true` naming the missing field

#### Scenario: Message to a task outside the children
- **WHEN** the agent of `DF-20` calls `send_message` for `DF-5`, which is not its child
- **THEN** the result has `is_error: true` and nothing is queued

## ADDED Requirements

### Requirement: create_task
`create_task` SHALL create a task in the run's project with `persona` (name), `title`, `prompt` and optional `depends_on` (list of `{task, kind}`, task given by key), with the run's task as parent and its agent as creator, and SHALL return the new task's key and status. The title SHALL be sanitized and stored with source `agent`. An unknown persona, an empty prompt or title, or an invalid dependency SHALL produce an error result and create nothing.

#### Scenario: Review task for an implementation
- **WHEN** the agent of `DF-19` calls `create_task` with persona "Reviewer", title "Review DF-20", a prompt and `depends_on: [{task: "DF-20", kind: "completion"}]` while `DF-20` is `running`
- **THEN** the result contains key `DF-21` and status `awaiting_dependencies`, and `DF-21` has parent `DF-19`

#### Scenario: Unknown persona
- **WHEN** `create_task` names persona "Nobody"
- **THEN** the result has `is_error: true` naming the persona and no task is created

#### Scenario: Retried call
- **WHEN** the runtime re-sends `create_task` with the same `call_id` after a reconnect
- **THEN** exactly one task exists and both requests get the same key

### Requirement: list_personas
`list_personas` SHALL return every persona of the instance with its name and description, ordered by name.

#### Scenario: Choosing a reviewer
- **WHEN** personas "Linus" (description "Rust implementation") and "Reviewer" (empty description) exist and the agent calls `list_personas`
- **THEN** the result lists "Linus" with its description and "Reviewer" with an empty description

### Requirement: send_message
`send_message` SHALL queue `text` as a message on the task given by key, which SHALL be a child of the run's task, authored by the run's agent, and SHALL return the prompt id and the child's resulting status. An empty text SHALL produce an error result.

#### Scenario: Next review round
- **WHEN** the agent of `DF-19` calls `send_message` for its child `DF-21` (`in_review`) with "DF-20 addressed your findings, review again"
- **THEN** the result contains a prompt id and status `queued`, and the message's author is the agent of `DF-19`

#### Scenario: Empty text
- **WHEN** `send_message` is called with only whitespace as text
- **THEN** the result has `is_error: true` and nothing is queued
