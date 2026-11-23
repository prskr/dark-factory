# Spec Delta

## RENAMED Requirements

- FROM: `### Requirement: Chat runs can't push`
- TO: `### Requirement: Read-only runs can't push`

## MODIFIED Requirements

### Requirement: Read-only runs can't push
The proxy SHALL reject every ref update pushed by the sandbox of a run whose task has a read-only workspace with reason `df:read-only`, without contacting upstream. The control plane SHALL take this from the task's read-only workspace property, not from the task key. Fetching SHALL work as for other tasks.

#### Scenario: Push from a chat
- **WHEN** the sandbox of `DF-12-CHAT` pushes `refs/heads/factory/task/DF-12-CHAT`
- **THEN** git reports `remote rejected` with reason `df:read-only` and nothing reaches upstream

#### Scenario: Fetch from a chat
- **WHEN** the sandbox of `DF-12-CHAT` fetches `main`
- **THEN** the fetch succeeds

#### Scenario: Route from the property
- **WHEN** the gateway snapshot is built for a run whose task has the read-only workspace property
- **THEN** its Git route is read-only with no task and no WIP branch, whatever the task key
