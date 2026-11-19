# Spec Delta

## ADDED Requirements

### Requirement: Read-only workspace
For a read-only workspace the runtime SHALL clone if needed, fetch the base branch, check out `origin/<base>` detached and reset the working tree to it, removing untracked files. It SHALL NOT restore or push WIP snapshots and SHALL NOT push any branch, during or after the run.

#### Scenario: Prepare a chat workspace
- **WHEN** a run of `DF-12-CHAT` prepares a volume with an edited file from an earlier run
- **THEN** the working tree equals `origin/main` and the edit is gone

#### Scenario: Commit in a chat
- **WHEN** the agent of `DF-12-CHAT` commits and the turn ends
- **THEN** the runtime pushes nothing
