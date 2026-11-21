# Spec Delta

## ADDED Requirements

### Requirement: Memory block before the first prompt
When `Assign.memory_context` is not empty, the runtime SHALL put it as its own text block before the prompts of the first turn of the run, after a continuation summary if there is one, both for a new and for a resumed harness session. It SHALL emit a `memory.loaded` notice (level `INFO`) carrying the block. Later turns of the run SHALL NOT repeat it.

#### Scenario: First turn
- **WHEN** a run with a memory block starts its first turn with a message from "Peter Kurfer"
- **THEN** the harness prompt holds the memory block before the line `Peter Kurfer:`

#### Scenario: Resumed session
- **WHEN** run 3 of `DF-31` resumes the harness session of run 2 and has a memory block
- **THEN** the first turn of run 3 starts with the block

#### Scenario: Second turn
- **WHEN** the second turn of the same run starts
- **THEN** its prompt holds no memory block

#### Scenario: Transcript shows what was loaded
- **WHEN** a run with a memory block starts
- **THEN** the transcript holds a `memory.loaded` notice with the block
