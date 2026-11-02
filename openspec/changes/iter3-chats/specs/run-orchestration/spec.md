# Spec Delta

## ADDED Requirements

### Requirement: Assign for chat runs
`Assign` of a chat run SHALL contain the base branch and no task or WIP branch, mark the workspace read-only, contain only the tools a chat run is offered, and carry the persona's system prompt followed by the chat instructions.

#### Scenario: Assign for DF-12-CHAT
- **WHEN** a run of `DF-12-CHAT` connects
- **THEN** `Assign.workspace` names the base branch, has no task or WIP branch and is read-only, and `Assign.tools` contains `create_task` but not `complete_task`

### Requirement: Chat runs start under their own limit
The start of queued runs SHALL apply `runs.maxConcurrent` to tasks of kind `task` and `runs.maxConcurrentChats` to chats, and SHALL NOT start runs of archived chats.

#### Scenario: Archived chat with a queued message
- **WHEN** archived `DF-12-CHAT` has a queued child-ended message and a chat slot is free
- **THEN** no run of `DF-12-CHAT` starts

### Requirement: Compaction notice
When a run seeds a new harness session with the transcript summary instead of resuming the previous session, the runtime SHALL emit a notice with code `context_compacted` and the summary as its message before the first prompt, for tasks and chats alike.

#### Scenario: Session not resumable
- **WHEN** run 4 of `DF-12-CHAT` can't resume the session of run 3 and starts with the summary
- **THEN** the transcript gets one `context_compacted` notice before the run's first event

#### Scenario: Session resumed
- **WHEN** run 4 resumes the session of run 3
- **THEN** no `context_compacted` notice is written
