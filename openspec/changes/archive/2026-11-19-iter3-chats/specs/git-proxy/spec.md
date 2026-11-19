# Spec Delta

## ADDED Requirements

### Requirement: Chat runs can't push
The proxy SHALL reject every ref update pushed by the sandbox of a chat run with reason `df:read-only`, without contacting upstream. Fetching SHALL work as for tasks.

#### Scenario: Push from a chat
- **WHEN** the sandbox of `DF-12-CHAT` pushes `refs/heads/factory/task/DF-12-CHAT`
- **THEN** git reports `remote rejected` with reason `df:read-only` and nothing reaches upstream

#### Scenario: Fetch from a chat
- **WHEN** the sandbox of `DF-12-CHAT` fetches `main`
- **THEN** the fetch succeeds
