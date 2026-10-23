# Spec Delta

## ADDED Requirements

### Requirement: Queued messages reach the agent between turns
An undelivered message SHALL be forwarded to the task's active run only while the run is between turns: when the run starts, and after each turn ends. Forwarding SHALL mark the message as forwarded. A message queued during a turn SHALL stay held back until that turn ends.

#### Scenario: Message during a turn
- **WHEN** a board member queues a message while the agent is in a turn
- **THEN** the message is not sent to the runtime until the transcript records the end of that turn

#### Scenario: Message on an idle run
- **WHEN** a message is queued while the active run is between turns
- **THEN** the message is forwarded to the runtime and marked as forwarded

#### Scenario: Reconnect after forwarding
- **WHEN** the runtime reconnects while a forwarded message is still undelivered
- **THEN** the message is sent again with the same prompt id and is delivered once

### Requirement: Editing and removing queued messages
A board member SHALL be able to replace the text of, or remove, a queued message written by a human as long as it is neither forwarded nor delivered. Both SHALL be refused once the message is forwarded; prompts authored by the system (such as the assignment prompt) SHALL NOT be editable or removable. Both SHALL write an audit entry (`message.edit` with old and new text, `message.remove` with the removed text) and publish a change notification.

#### Scenario: Edit before forwarding
- **WHEN** a board member edits a held-back message to "also check logout"
- **THEN** the stored message has the new text, keeps its prompt id and queue position, and an audit entry `message.edit` exists

#### Scenario: Edit after forwarding
- **WHEN** a board member edits a message that was forwarded in the meantime
- **THEN** the edit is refused with "The agent has already received this message." and the message is unchanged

#### Scenario: Remove before forwarding
- **WHEN** a board member removes a held-back message
- **THEN** the message no longer exists and an audit entry `message.remove` exists

#### Scenario: Assignment prompt
- **WHEN** a board member tries to remove the assignment prompt of a new task
- **THEN** the removal is refused and the prompt stays queued

#### Scenario: Empty edit
- **WHEN** a queued message is edited to an empty or whitespace-only text
- **THEN** the edit is refused and the message is unchanged

### Requirement: Removing the last queued message
When a removal leaves a task in status `queued` with no undelivered message, the task SHALL return to `waiting`, so no run starts for nothing.

#### Scenario: Last message of a waiting task removed
- **WHEN** a board member queues a message on a `waiting` task and removes it before a run starts
- **THEN** the task status is `waiting` again

#### Scenario: Other messages still queued
- **WHEN** two messages are queued on a `waiting` task and one is removed
- **THEN** the task stays `queued`
