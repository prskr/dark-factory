# Spec Delta

## ADDED Requirements

### Requirement: Memory block in Assign
`Assign.memory_context` SHALL carry the run's memory block (`agent-memory`, "Memory block at run start"), or be empty when the run has none. Chat runs SHALL carry it the same way. Every `Assign` for the same run SHALL carry the same block.

#### Scenario: Task run with memories
- **WHEN** run 1 of `DF-40` by persona `linus` connects and `DF` has memories in its scopes
- **THEN** `Assign.memory_context` starts with the heading "Memory of project DF" and contains the index lines

#### Scenario: Chat run
- **WHEN** a run of `DF-12-CHAT` by persona `woz` connects
- **THEN** `Assign.memory_context` holds the block for persona `woz` in `DF`

#### Scenario: Not in the system prompt
- **WHEN** a run with memories connects
- **THEN** the system prompt in the harness files contains no memory text
