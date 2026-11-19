# Spec Delta

## ADDED Requirements

### Requirement: Chat keys
A chat SHALL get the key `<PROJECT_KEY>-<n>-CHAT`, with `n` taken from the same per-project counter as task keys, so a task and a chat never share a number.

#### Scenario: Chat between tasks
- **WHEN** project `DF` has tasks up to `DF-24` and a chat is started, then a task is created
- **THEN** the chat is `DF-25-CHAT` and the task is `DF-26`

### Requirement: Task kinds
Every task SHALL have the kind `task` or `chat`, fixed at creation. Tasks created by board members in the task UI and by `create_task` SHALL have the kind `task`.

#### Scenario: Task created by a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task`
- **THEN** the created task has the kind `task`

### Requirement: Chats are not cancelled or marked done
Cancelling and marking done SHALL be rejected for tasks of kind `chat`, as a conflict, without an audit entry.

#### Scenario: Mark a chat done
- **WHEN** a board member marks `DF-12-CHAT` done
- **THEN** the request is rejected as a conflict and the status does not change
