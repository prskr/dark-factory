# Spec Delta

## MODIFIED Requirements

### Requirement: Read-only workspace
For a read-only workspace the runtime SHALL clone if needed, fetch the base branch, check out `origin/<base>` detached and reset the working tree to it, removing untracked files. It SHALL NOT restore or push WIP snapshots and SHALL NOT push any branch, during or after the run. A chat run's data volume is empty at the start of the run, so it always clones.

#### Scenario: Prepare a chat workspace
- **WHEN** a run of `DF-12-CHAT` prepares an empty data volume
- **THEN** `/data/repo` is a blobless clone with `origin/main` checked out detached

#### Scenario: Commit in a chat
- **WHEN** the agent of `DF-12-CHAT` commits and the turn ends
- **THEN** the runtime pushes nothing
