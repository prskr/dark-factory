# Spec Delta

## MODIFIED Requirements

### Requirement: Creator and parent in the task details
The task details SHALL show the task's creator (the board member's name, the creating task's key with its persona, or "Routine · " with the routine's name at firing time, linked to the routines page of its project while the routine exists) and, if the task has a parent, the parent's key and title as a link to the parent task.

#### Scenario: Created by an agent
- **WHEN** a board member opens `DF-21`, created by the agent of `DF-19` (persona "CEO")
- **THEN** the details show "DF-19 · CEO" as creator and `DF-19` with its title as parent link

#### Scenario: Created by a board member
- **WHEN** a board member opens a task they created
- **THEN** the details show their name as creator and no parent

#### Scenario: Created by a routine
- **WHEN** a board member opens `DF-41`, created by routine "Nightly deps"
- **THEN** the details show "Routine · Nightly deps" as creator, linked to `/routines?project=DF`, and no parent

#### Scenario: Routine deleted
- **WHEN** routine "Nightly deps" was deleted after creating `DF-41`
- **THEN** the details of `DF-41` still show "Routine · Nightly deps" as creator, without a link
