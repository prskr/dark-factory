# Spec Delta

## ADDED Requirements

### Requirement: Chat state between runs
A chat SHALL have a read-only workspace. It SHALL keep only its transcript, its artifacts and its harness session state between runs. Files of a run below `/data` outside the declared session state directories SHALL be gone at the next run that starts in a new pod. A chat SHALL stay read-only through the gateway's rejection of every ref update and the reset of its checkout, without a file-system write block.

#### Scenario: File in home
- **WHEN** the agent of `DF-12-CHAT` writes `/data/home/notes.md`, the run is released, and the next message starts a new run
- **THEN** `/data/home/notes.md` doesn't exist in the new run

#### Scenario: Write in the checkout
- **WHEN** the agent of `DF-12-CHAT` edits a file below `/data/repo`
- **THEN** the write succeeds, and the edit is gone at the next run and is never pushed

#### Scenario: Session continues in a new pod
- **WHEN** the run of `DF-12-CHAT` with a `claude-code` persona is released after its keep-alive window and a board member sends a message
- **THEN** the new run loads the earlier harness session and writes no `context_compacted` notice
