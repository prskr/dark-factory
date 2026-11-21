# Spec Delta

## MODIFIED Requirements

### Requirement: Session state on the task volume
OpenCode's session state SHALL live below `HOME`, in `.local/share/opencode`. For a task of kind `task` it is on the task volume; for a chat it is carried to the next run as the chat's session state. In both cases the next run resumes the session with `session/load`.

#### Scenario: Resume after a run
- **WHEN** run 1 of an `opencode` task ended and run 2 starts with run 1's harness session id
- **THEN** `session/load` succeeds and replays run 1's conversation

#### Scenario: Resume a chat in a new pod
- **WHEN** run 1 of an `opencode` chat ended and run 2 starts in a new pod with run 1's session state and harness session id
- **THEN** `.local/share/opencode` is restored before OpenCode starts and `session/load` replays run 1's conversation
