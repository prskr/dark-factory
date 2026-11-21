# Spec Delta

## MODIFIED Requirements

### Requirement: Tools of a chat run
A chat run SHALL be offered `set_task_title`, `create_task`, `list_personas`, `send_message`, `report_progress`, `request_approval`, `write_artifact`, `list_artifacts`, `read_artifact`, `remember`, `recall`, `forget` and `search_history`, and SHALL NOT be offered `complete_task`, `ask_question`, `update_pull_request` or `comment_on_issue`. A call to a tool not offered SHALL be answered with a tool error and change nothing.

#### Scenario: Delegation from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task` for persona Linus
- **THEN** a task is created whose parent is `DF-12-CHAT`

#### Scenario: Completing a chat
- **WHEN** the agent of `DF-12-CHAT` calls `complete_task`
- **THEN** the call returns a tool error and the chat's status is unchanged

#### Scenario: Memory in a chat
- **WHEN** the agent of `DF-12-CHAT` calls `remember` with scope `project`
- **THEN** the memory is stored with `DF-12-CHAT` and its run as author
