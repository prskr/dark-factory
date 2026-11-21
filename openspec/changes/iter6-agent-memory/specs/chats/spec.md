# Spec Delta

## MODIFIED Requirements

### Requirement: Chat instructions
Every run of a chat SHALL receive the persona's system prompt followed by fixed chat instructions from the control plane. The instructions SHALL state that the agent talks to a board member, that the checkout is read-only, that work needing a branch goes into tasks created with `create_task`, that facts later runs need belong in memory (`remember`), and that the chat is never completed.

#### Scenario: System prompt of a chat run
- **WHEN** a run of `DF-25-CHAT` with persona Grace connects
- **THEN** its system prompt is Grace's system prompt followed by the chat instructions

#### Scenario: Task runs unchanged
- **WHEN** a run of task `DF-20` with persona Grace connects
- **THEN** its system prompt contains no chat instructions

#### Scenario: Memory named in the instructions
- **WHEN** a run of `DF-25-CHAT` connects
- **THEN** the chat instructions name `remember` for facts that later runs need

### Requirement: Tools of a chat run
A chat run SHALL be offered `set_task_title`, `create_task`, `list_personas`, `send_message`, `report_progress`, `request_approval`, `write_artifact`, `list_artifacts`, `read_artifact` and `search_history`, plus `remember`, `recall` and `forget` while memory is on for the project, and SHALL NOT be offered `complete_task`, `ask_question`, `update_pull_request` or `comment_on_issue`. A call to a tool not offered SHALL be answered with a tool error and change nothing.

#### Scenario: Delegation from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task` for persona Linus
- **THEN** a task is created whose parent is `DF-12-CHAT`

#### Scenario: Completing a chat
- **WHEN** the agent of `DF-12-CHAT` calls `complete_task`
- **THEN** the call returns a tool error and the chat's status is unchanged

#### Scenario: Memory in a chat
- **WHEN** the agent of `DF-12-CHAT` calls `remember` with scope `project`
- **THEN** the memory is stored with `DF-12-CHAT` and its run as author
