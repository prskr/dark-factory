# Spec Delta

## MODIFIED Requirements

### Requirement: complete_task
`complete_task` SHALL store the summary on the task and return a result with `end_run_after_turn: true`. Its description SHALL ask the agent to call it also when the instructions leave nothing to do, with a summary that says why, and SHALL say that ending the turn without it leaves the task waiting for a human.

#### Scenario: Completion
- **WHEN** the agent calls `complete_task`
- **THEN** the result tells the runtime to finalize after the current turn

#### Scenario: Nothing left to do
- **WHEN** a run receives the factory tools
- **THEN** the `complete_task` description asks the agent to call it when the instructions leave nothing to do, with a summary that says why
