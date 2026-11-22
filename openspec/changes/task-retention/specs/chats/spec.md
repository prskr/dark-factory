# Spec Delta

## REMOVED Requirements

### Requirement: Chats are never deleted
**Reason**: Chats hold transcripts and artifacts like tasks and must not keep them forever. An archived chat is deleted like a closed task (`task-retention`): the chat row stays as a tombstone, so the parent references of its subtasks stay valid.
**Migration**: None. Pre-MVP; no installation depends on the old behavior.
