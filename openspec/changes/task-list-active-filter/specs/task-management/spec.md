## MODIFIED Requirements

### Requirement: Task status groups
`awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached` SHALL form the "Waiting" group, `completed` and `failed` the "Done" group. The project counters and the `waiting` link filter of the task list SHALL use the "Waiting" group; the task list's Active filter SHALL cover every status outside the "Done" group.

#### Scenario: Waiting group
- **WHEN** the task list is filtered by the "Waiting" group
- **THEN** it lists the tasks in status `awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached`

#### Scenario: Active filter
- **WHEN** the task list is filtered by Active
- **THEN** it lists the tasks in every status except `completed`, `cancelled` and `failed`
