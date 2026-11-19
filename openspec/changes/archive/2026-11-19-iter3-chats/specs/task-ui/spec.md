# Spec Delta

## ADDED Requirements

### Requirement: Task list without chats
The task list and its counts SHALL contain only tasks of kind `task`.

#### Scenario: Chat in the project
- **WHEN** project `DF` has `DF-12-CHAT` and `DF-20`
- **THEN** the task list shows `DF-20` and not `DF-12-CHAT`

### Requirement: Parent chat link
When a task's parent is a chat, the task details SHALL show it as `<CHAT KEY> · <persona> · <topic>` linked to the chat page.

#### Scenario: Task created in a chat
- **WHEN** a board member opens `DF-20`, created by `DF-12-CHAT` of Linus with topic "Task dependencies plan"
- **THEN** the parent reads "DF-12-CHAT · Linus · Task dependencies plan" and links to `/chats/DF-12-CHAT`
