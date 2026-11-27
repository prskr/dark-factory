# Spec Delta

## MODIFIED Requirements

### Requirement: Chat instructions
Every run of a chat SHALL receive the persona's system prompt followed by fixed chat instructions from the control plane. The instructions SHALL state that the agent talks to a board member, that the checkout is read-only, that work needing a branch goes into tasks created with `create_task`, that facts later runs need belong in memory (`remember`) unless memory was off for the project at the run's first `Assign`, and that the chat is never completed.

#### Scenario: System prompt of a chat run
- **WHEN** a run of `DF-25-CHAT` with persona Grace connects
- **THEN** its system prompt is Grace's system prompt followed by the chat instructions

#### Scenario: Task runs unchanged
- **WHEN** a run of task `DF-20` with persona Grace connects
- **THEN** its system prompt contains no chat instructions

#### Scenario: Memory named in the instructions
- **WHEN** memory is on for `DF` when a run of `DF-25-CHAT` gets its first `Assign`
- **THEN** the chat instructions name `remember` for facts that later runs need, also in every later `Assign` of that run

#### Scenario: Memory off
- **WHEN** memory is off for `DF` when a run of `DF-25-CHAT` gets its first `Assign`
- **THEN** the chat instructions do not name `remember`, also when memory is switched on before a reconnect of that run

### Requirement: Tools of a chat run
A chat run SHALL be offered `set_task_title`, `create_task`, `list_personas`, `send_message`, `report_progress`, `ask_question`, `request_approval`, `write_artifact`, `list_artifacts`, `read_artifact`, `list_issues`, `create_issue` and `search_history`, plus `remember`, `recall` and `forget` if the project had memory on at the run's first `Assign`, and SHALL NOT be offered `complete_task`, `update_pull_request` or `comment_on_issue`. Other tool calls SHALL get a tool error and change nothing.

#### Scenario: Delegation from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task` for persona Linus
- **THEN** a task is created whose parent is `DF-12-CHAT`

#### Scenario: Completing a chat
- **WHEN** the agent of `DF-12-CHAT` calls `complete_task`
- **THEN** the call returns a tool error and the chat's status is unchanged

#### Scenario: Issues from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `list_issues`
- **THEN** the call returns the open issues of the project's repository

#### Scenario: Question from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `ask_question` with kind `single_choice` and two options
- **THEN** a pending question of `DF-12-CHAT` exists, its card is on the chat timeline and the inbox lists it with a link to `/chats/DF-12-CHAT`

#### Scenario: Memory in a chat
- **WHEN** memory is on for `DF` both at the run's first `Assign` and at the call, and the agent of `DF-12-CHAT` calls `remember` with scope `project`
- **THEN** the memory is stored with `DF-12-CHAT` and its run as author

#### Scenario: History search in a chat
- **WHEN** a run of `DF-12-CHAT` connects and its agent calls `search_history`
- **THEN** `Assign.tools` contains `search_history` and the call returns hits of project `DF`
