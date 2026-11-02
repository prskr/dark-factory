# Spec Delta

## ADDED Requirements

### Requirement: Requests from chats
A request raised by a chat run SHALL be listed like any other and link to the chat page instead of the task page.

#### Scenario: Approval request from a chat
- **WHEN** the agent of `DF-12-CHAT` requests an approval
- **THEN** the inbox lists it and its link opens `/chats/DF-12-CHAT`
