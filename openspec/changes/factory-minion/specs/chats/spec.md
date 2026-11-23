## MODIFIED Requirements

### Requirement: Starting a chat
A board member SHALL be able to start a chat in a project by choosing a persona and writing a first message. The chat SHALL be a task of kind `chat` without a parent, created by that board member. The first message SHALL be queued as an ordinary message from the board member, followed, while the minion is off, by the hidden `set_task_title` instruction. Agents SHALL NOT be able to start chats.

#### Scenario: New chat
- **WHEN** a board member starts a chat in project `DF` with persona Grace and the message "Plan the Helm chart release"
- **THEN** a chat `DF-25-CHAT` exists with persona Grace, its transcript starts with that message authored by the board member, and the chat becomes `queued`

#### Scenario: Several chats with one persona
- **WHEN** persona Linus already has a chat in project `DF` and a board member starts another chat with Linus in `DF`
- **THEN** both chats exist side by side

#### Scenario: Empty first message
- **WHEN** a board member starts a chat with an empty message
- **THEN** it is rejected with a validation error and no chat is created

#### Scenario: Chat started with the minion on
- **WHEN** the minion is on and a board member starts a chat with the message "Plan the Helm chart release"
- **THEN** the first message has no hidden `set_task_title` instruction and a `task-title` job is enqueued

### Requirement: Topic
The topic of a chat SHALL be its task title. While the minion is off, the agent SHALL set it with `set_task_title` in the first run; while the minion is on, the minion SHALL generate it like a task title. Until then the placeholder derived from the first message SHALL be shown. A board member SHALL be able to rename a chat like a task.

#### Scenario: Topic set by the agent
- **WHEN** the minion is off and the agent of `DF-25-CHAT` calls `set_task_title` with "Helm chart for iteration 3"
- **THEN** the chat's topic is "Helm chart for iteration 3"

#### Scenario: Topic from the minion
- **WHEN** the minion is on and the `task-title` job of `DF-25-CHAT` returns "Helm chart release plan"
- **THEN** the chat's topic is "Helm chart release plan" with source `generated`

#### Scenario: Renamed by a board member
- **WHEN** a board member renames `DF-25-CHAT` to "CRD migration"
- **THEN** the topic is "CRD migration" and a later `set_task_title` call does not change it
