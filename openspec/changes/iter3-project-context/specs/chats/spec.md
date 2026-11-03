# Spec Delta

## MODIFIED Requirements

### Requirement: Chat instructions
Every run of a chat SHALL receive the persona's system prompt, then the project context (see `project-context`), then fixed chat instructions from the control plane. The instructions SHALL state that the agent talks to a board member, that the checkout is read-only, that work needing a branch goes into tasks created with `create_task`, that the chat is never completed, and that changes to the project context are proposed with `request_approval` type `project-context`.

#### Scenario: System prompt of a chat run
- **WHEN** a run of `DF-25-CHAT` with persona Grace connects and the context of `DF` is not empty
- **THEN** its system prompt is Grace's system prompt, followed by the project context, followed by the chat instructions

#### Scenario: Task runs unchanged
- **WHEN** a run of task `DF-20` with persona Grace connects
- **THEN** its system prompt contains no chat instructions

### Requirement: Archiving a chat
Archiving SHALL record when and by whom a chat was archived, keep its status, and be rejected while the chat is `queued` or `running` or has an open approval request other than `project-context`. Archiving SHALL NOT change the chat's subtasks or its pending `project-context` requests.

#### Scenario: Archive an idle chat
- **WHEN** a board member archives `DF-12-CHAT` in status `waiting`
- **THEN** the chat is archived, its status stays `waiting` and an audit entry `chat.archive` names the board member

#### Scenario: Archive while running
- **WHEN** a board member archives `DF-12-CHAT` while its run is active
- **THEN** the request is rejected and the chat is not archived

#### Scenario: Archive with an open approval request
- **WHEN** `DF-12-CHAT` has a pending `network-access` request and a board member archives it
- **THEN** the request is rejected with a reason naming the open request

#### Scenario: Archive with a pending context proposal
- **WHEN** `DF-12-CHAT` has only a pending `project-context` request and a board member archives it
- **THEN** the chat is archived, the request stays pending in the inbox, and its decision queues a message on the archived chat without starting a run

#### Scenario: Running subtask
- **WHEN** a board member archives `DF-12-CHAT` while its subtask `DF-20` is running
- **THEN** the chat is archived and `DF-20` keeps running
