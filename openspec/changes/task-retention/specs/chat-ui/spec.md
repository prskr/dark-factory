# Spec Delta

## ADDED Requirements

### Requirement: Delete an archived chat
The archive modal SHALL offer the action Delete for the selected chat when it is deletable (`task-retention`), with a confirmation that names what is removed. A deleted chat SHALL leave the archive list.

#### Scenario: Delete from the archive modal
- **WHEN** a board member deletes archived `DF-3-CHAT` in the archive modal and confirms
- **THEN** `DF-3-CHAT` is no longer listed in the archive modal and opening `/chats/DF-3-CHAT` shows it as deleted

### Requirement: Deleted chat page
A deleted chat SHALL show its topic, persona, key, archive date and deletion date and deleter, and the text that its transcript, messages and artifacts were removed. It SHALL offer no actions.

#### Scenario: Link to a deleted chat
- **WHEN** a board member follows a link to deleted `DF-3-CHAT`
- **THEN** the page shows the chat's topic and "Deleted on <date> by <deleter>; transcript, messages and artifacts were removed" and no Unarchive action
