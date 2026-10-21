# Spec Delta

## ADDED Requirements

### Requirement: ask_question
`ask_question` SHALL raise a question for humans on the run's task with `text`, optional `suggested_answers` (at most 5) and `blocking` (default false), and return the request id and state. A blocking call SHALL return a result with `await_input_after_turn: true`.

#### Scenario: Non-blocking question
- **WHEN** the agent calls `ask_question` with text "Should I also update the docs?" and `blocking: false`
- **THEN** the result contains the request id and state `pending`, and the run continues

#### Scenario: Blocking question
- **WHEN** the agent calls `ask_question` with `blocking: true`
- **THEN** the result has `await_input_after_turn: true`

### Requirement: Tool set v2
Runs SHALL receive the factory tools v2 (`contract/mcp/factory-tools.v2.json`): the four v1 tools plus `ask_question`, plus tools added by later changes to that file.

#### Scenario: Tools listed
- **WHEN** a run connects
- **THEN** `Assign.tools` contains `ask_question` next to the v1 tools
