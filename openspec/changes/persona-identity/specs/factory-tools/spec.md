# Spec Delta

## MODIFIED Requirements

### Requirement: create_task
`create_task` SHALL create a task in the run's project with `persona` (handle), `title`, `prompt`, optional `depends_on` (list of `{task, kind}`, task given by key) and optional `branch`, with the run's task as parent and its agent as creator, and SHALL return the new task's key and status. The title SHALL be sanitized and stored with source `agent`. An unknown persona, an empty prompt or title, or an invalid dependency SHALL produce an error result and create nothing.

#### Scenario: Review task for an implementation
- **WHEN** the agent of `DF-19` calls `create_task` with persona `reviewer`, title "Review DF-20", a prompt and `depends_on: [{task: "DF-20", kind: "completion"}]` while `DF-20` is `running`
- **THEN** the result contains key `DF-21` and status `awaiting_dependencies`, and `DF-21` has parent `DF-19`

#### Scenario: Unknown persona
- **WHEN** `create_task` names persona `nobody`
- **THEN** the result has `is_error: true` naming the persona and no task is created

#### Scenario: Display name instead of handle
- **WHEN** persona `reviewer` has display name "Code Reviewer" and `create_task` names persona "Code Reviewer"
- **THEN** the result has `is_error: true` stating that personas are addressed by handle, and no task is created

#### Scenario: Retried call
- **WHEN** the runtime re-sends `create_task` with the same `call_id` after a reconnect
- **THEN** exactly one task exists and both requests get the same key

### Requirement: list_personas
`list_personas` SHALL return every persona of the instance with its handle, display name and description, ordered by handle.

#### Scenario: Choosing a reviewer
- **WHEN** personas `linus` ("Linus", description "Rust implementation") and `reviewer` ("Reviewer", empty description) exist and the agent calls `list_personas`
- **THEN** the result lists `linus` with display name "Linus" and its description, then `reviewer` with display name "Reviewer" and an empty description
